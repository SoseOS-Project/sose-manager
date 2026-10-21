// svm: command line client for svmd, like qvm-start/qvm-shutdown in qubes.
//   svm list
//   svm start <name>
//   svm stop <name>
//   svm status <name>
// it doesn't check the command itself, just forwards it and svmd decides.
// svmd answers in plain text, this file only makes it look nice.

use std::env;

use svm_client::svm_request;
use utils::{paint, table, Cell, Style};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("No arguments supplied...");
        return Ok(());
    }

    // args[0] is the program itself, so skip it and glue the rest back into one line
    let cmd = args[1..].join(" ");
    let reply = svm_request(&cmd)?;

    print_reply(&args[1], &reply);

    Ok(())
}

// running -> green, stopped -> dim red
fn state_style(state: &str) -> Style {
    match state {
        "running" => Style::On,
        "stopped" => Style::Off,
        _ => Style::Plain,
    }
}

// svmd's reply is some lines of info, then "ok" or "err ..." as the last line
fn print_reply(cmd: &str, reply: &str) {
    let lines: Vec<&str> = reply.lines().collect();
    let Some((last, body)) = lines.split_last() else {
        return;
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
                    .map(|(name, state)| vec![Cell::new(name, Style::Plain), Cell::new(state, state_style(state))])
                    .collect();
                table(&["NAME", "STATE"], &rows)
            }
            "status" => {
                let rows: Vec<Vec<Cell>> = pairs
                    .iter()
                    .map(|(key, value)| vec![Cell::new(key, Style::Dim), Cell::new(value, state_style(value))])
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
    } else {
        println!("{}", paint(last, Style::Warn));
    }
}
