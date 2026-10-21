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
use std::time::{SystemTime, UNIX_EPOCH};

use svm_client::SVM_SOCK_PATH;
use xenstore::{XENSTORED_SOCKET, XS_DIRECTORY, XS_READ, xs_request};

// one xl config per vm. the name = "..." inside has to match the file name
const VM_DIR: &str = "/etc/sose/vms";

// memory (KiB) and vcpu count for a running domain
fn domain_info(id: &str) -> std::io::Result<(String, usize)> {
    let mut stream = UnixStream::connect(XENSTORED_SOCKET)?;

    let mem = xs_request(
        &mut stream,
        XS_READ,
        &format!("/local/domain/{id}/memory/target"),
        &[],
    )?;
    let cpus = xs_request(
        &mut stream,
        XS_DIRECTORY,
        &format!("/local/domain/{id}/cpu"),
        &[],
    )?;

    let mem = String::from_utf8_lossy(&mem).into_owned();
    let vcpus = String::from_utf8_lossy(&cpus)
        .split('\0')
        .filter(|c| !c.is_empty())
        .count();

    Ok((mem, vcpus))
}

fn domid_of(name: &str) -> std::io::Result<Option<String>> {
    let mut stream = UnixStream::connect(XENSTORED_SOCKET)?;
    let payload = xs_request(&mut stream, XS_DIRECTORY, "/local/domain", &[])?;
    let text = String::from_utf8_lossy(&payload);

    for id in text.split('\0') {
        if id.is_empty() {
            continue;
        }

        let path = format!("/local/domain/{}/name", id);
        let name_maybe = xs_request(&mut stream, XS_READ, &path, &[])?;

        if String::from_utf8_lossy(&name_maybe) == name {
            return Ok(Some(id.to_string()));
        }
    }

    Ok(None)
}

fn running_vms() -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();

    // Reusing all of the same logic from svm-ls
    let mut stream = UnixStream::connect(XENSTORED_SOCKET)?;
    let payload = xs_request(&mut stream, XS_DIRECTORY, "/local/domain", &[])?;

    let text = String::from_utf8_lossy(&payload);
    let ids = text.split('\0');

    for id in ids {
        if id.is_empty() {
            continue;
        }

        let path = format!("/local/domain/{}/name", id);
        let name = xs_request(&mut stream, XS_READ, &path, &[])?;

        names.push(String::from_utf8_lossy(&name).into_owned());
    }

    Ok(names)
}

fn status(name: &str) -> String {
    // without this, any made up name would just show up as "stopped"
    if !defined_vms().iter().any(|vm| vm == name) {
        return "err unknown vm".to_string();
    }

    let id = match domid_of(name) {
        Ok(Some(id)) => id,
        Ok(None) => return "state stopped\nok".to_string(),
        Err(e) => return format!("err xenstore: {e}"),
    };

    let (mem, vcpus) = match domain_info(&id) {
        Ok(info) => info,
        Err(e) => return format!("err xenstore: {e}"),
    };
    let mem_trim = mem.trim().parse::<u64>().unwrap_or(0) / 1024;

    format!("memory {mem_trim} MiB\nvcpus {vcpus}\nok")
}

// names go into file paths and xl args, so no "/" or ".." or anything weird
fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

// runs xl, if it fails you get xl's error text back
fn xl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("xl")
        .args(args)
        .output()
        .map_err(|e| e.to_string())?;
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
        "list" if !arg.is_empty() => Err("list doesn't take a name".to_string()),
        "list" => {
            let running = match running_vms() {
                Ok(names) => names,
                Err(e) => return format!("err xenstore: {e}"),
            };

            let mut reply = String::new();
            for name in defined_vms() {
                let state = if running.contains(&name) {
                    "running"
                } else {
                    "stopped"
                };
                reply.push_str(&format!("{name} {state}\n"));
            }
            return reply + "ok";
        }
        "start" | "stop" | "status" if !valid_name(arg) => Err("bad vm name".to_string()),
        "start" => xl(&["create", &format!("{VM_DIR}/{arg}.cfg")]),
        "stop" => xl(&["destroy", arg]),
        "status" => return status(arg),
        _ => Err("unknown command".to_string()),
    };

    match result {
        Ok(()) => "ok".to_string(),
        // xl errors can be a few lines long, keep the reply on one line
        Err(err) => format!("err {}", err.replace('\n', " ")),
    }
}

fn client_handler(stream: UnixStream) {
    for line in BufReader::new(&stream).lines() {
        match line {
            Ok(line) => {
                // nc and socat can send \r\n, trim that off
                let line = line.trim();

                let reply = handle(line);
                let time = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                println!("{time} {} -> {}", line, reply.lines().last().unwrap_or(""));

                if let Err(err) = writeln!(&stream, "{}", reply) {
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
            Ok(stream) => {
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
