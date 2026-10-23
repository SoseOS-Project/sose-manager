// svm: command line client for svmd, like qvm-start/qvm-shutdown in qubes.
//   svm list
//   svm start <name>
//   svm stop <name>
//   svm status <name>
//   svm autostart set|remove <name> / svm autostart list
//   svm prefs <name> <key> <value>
// it doesn't check the command itself, just forwards it and svmd decides.
// svmd answers in plain text, this file only makes it look nice.
// exits with 1 when something failed, so scripts can tell

use std::env;
use std::process::ExitCode;

use svm_client::svm_request;
use utils::{Cell, Style, paint, table};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("No arguments supplied...");
        return ExitCode::FAILURE;
    }

    // args[0] is the program itself, so skip it and glue the rest back into one line
    let cmd = args[1..].join(" ");
    let reply = match svm_request(&cmd) {
        Ok(reply) => reply,
        // usually svmd isn't running, or svm wasn't run as root (socket is 0600)
        Err(e) => {
            eprintln!("{}", paint(&format!("can't reach svmd: {e}"), Style::Warn));
            return ExitCode::FAILURE;
        }
    };

    if print_reply(&args[1], &reply) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

// running -> green, stopped -> dim red
fn state_style(state: &str) -> Style {
    match state {
        "running" => Style::On,
        "stopped" => Style::Off,
        _ => Style::Plain,
    }
}

// svmd's reply is some lines of info, then "ok" or "err ..." as the last line.
// returns true if it ended in ok
fn print_reply(cmd: &str, reply: &str) -> bool {
    let lines: Vec<&str> = reply.lines().collect();
    let Some((last, body)) = lines.split_last() else {
        // svmd hung up without answering
        return false;
    };

    // body lines are "word rest", like "test running" or "memory 128 MiB"
    let pairs: Vec<(&str, &str)> = body
        .iter()
        .map(|l| l.split_once(' ').unwrap_or((l, "")))
        .collect();

    if !pairs.is_empty() {
        let out = match cmd {
            "list" => {
                let rows: Vec<Vec<Cell>> = pairs
                    .iter()
                    .map(|(name, state)| {
                        vec![
                            Cell::new(name, Style::Plain),
                            Cell::new(state, state_style(state)),
                        ]
                    })
                    .collect();
                table(&["NAME", "STATE"], &rows)
            }
            "status" => {
                let rows: Vec<Vec<Cell>> = pairs
                    .iter()
                    .map(|(key, value)| {
                        vec![
                            Cell::new(key, Style::Dim),
                            Cell::new(value, state_style(value)),
                        ]
                    })
                    .collect();
                table(&["FIELD", "VALUE"], &rows)
            }
            // anything else, just print what svmd said
            _ => body.join("\n") + "\n",
        };

        println!();
        print!("{out}");
        println!();
    }

    // last line: ok in dim green, errors in yellow
    if *last == "ok" {
        println!("{}", paint("ok", Style::Ok));
        true
    } else {
        println!("{}", paint(last, Style::Warn));
        false
    }
}
