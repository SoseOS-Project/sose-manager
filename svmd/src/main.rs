// svmd: the vm manager daemon. tools send it text commands over /run/svmd.sock,
// one per line, and it starts/stops vms with xl.
//
//   list          -> vm names, then "ok"
//   start <name>  -> xl create /etc/sose/vms/<name>.cfg
//   stop <name>   -> xl destroy <name>
//
// every reply ends with a line that's "ok" or starts with "err"

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::thread;

use xenstore::SVM_SOCK_PATH;

// one xl config per vm. the name = "..." inside has to match the file name
const VM_DIR: &str = "/etc/sose/vms";

// names go into file paths and xl args, so no "/" or ".." or anything weird
fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// runs xl, if it fails you get xl's error text back
fn xl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("xl").args(args).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

// every <name>.cfg in VM_DIR, without the .cfg
fn defined_vms() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(VM_DIR) {
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().to_string();
            if let Some(name) = file.strip_suffix(".cfg") {
                names.push(name.to_string());
            }
        }
    }
    names.sort();
    names
}

// one command line in, reply out
fn handle(line: &str) -> String {
    let (cmd, arg) = line.split_once(' ').unwrap_or((line, ""));

    let result = match cmd {
        "list" => {
            let mut reply = String::new();
            for name in defined_vms() {
                reply.push_str(&name);
                reply.push('\n');
            }
            return reply + "ok";
        }
        "start" | "stop" if !valid_name(arg) => Err("bad vm name".to_string()),
        "start" => xl(&["create", &format!("{VM_DIR}/{arg}.cfg")]),
        "stop" => xl(&["destroy", arg]),
        _ => Err("unknown command".to_string()),
    };

    match result {
        Ok(()) => "ok".to_string(),
        // xl errors can be a few lines long, keep the reply on one line
        Err(err) => format!("err {}", err.replace('\n', " ")),
    }
}

fn client_handler(stream: UnixStream){
    for line in BufReader::new(&stream).lines() {
        match line {
            Ok(line) => {
                // nc and socat can send \r\n, trim that off
                let line = line.trim();
                println!("{}", line);

                if let Err(err) = writeln!(&stream, "{}", handle(line)) {
                    eprintln!("svmd: write failed: {err}");
                    break;
                }
            }

            Err(err) => {
                eprintln!("svmd: read failed: {err}");
                break;
            }
        }
    }

}
fn main() -> std::io::Result<()> {
    // old socket file from a previous run makes bind() fail
    let _ = std::fs::remove_file(SVM_SOCK_PATH);
    let listener = UnixListener::bind(SVM_SOCK_PATH)?;

    for stream in listener.incoming() {
        match stream {
            Ok(stream ) => {
                thread::spawn(move || client_handler(stream));
            }
            Err(err) => {
                // one bad connection shouldn't kill the whole daemon
                eprintln!("svmd: accept failed: {err}");
            }
        }
    }

    Ok(())
}
