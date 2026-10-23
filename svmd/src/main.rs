// svmd: the vm manager daemon. tools send it text commands over /run/svmd.sock,
// one per line, and it starts/stops vms with xl.
//
//   list                     -> every vm and if it's running or stopped
//   start <name>             -> xl create /etc/sose/vms/<name>.cfg
//   stop <name>              -> xl destroy <name>
//   status <name>            -> memory and vcpus of one vm
//   autostart set <name>     -> start this vm at boot
//   autostart remove <name>  -> don't start it at boot anymore
//   autostart list           -> what starts at boot
//
// every reply ends with a line that's "ok" or starts with "err"

use std::fs::{Permissions, set_permissions};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::Command;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use svm_client::SVM_SOCK_PATH;
use xenstore::{XENSTORED_SOCKET, XS_DIRECTORY, XS_READ, xs_request};

// one xl config per vm. the name = "..." inside has to match the file name
const VM_DIR: &str = "/etc/sose/vms";

// vms to start at boot, one name per line.
// dom0 runs from RAM but stays persistent under /var/lib/sose
const AUTOSTART_FILE: &str = "/var/lib/sose/autostart";

// starts every vm listed in AUTOSTART_FILE, one name per line.
// no file = nothing to start, that's fine
fn autostart() {
    let Ok(text) = std::fs::read_to_string(AUTOSTART_FILE) else {
        return;
    };

    for name in text.lines() {
        let name = name.trim();

        // skip blank lines instead of stopping at them
        if name.is_empty() {
            continue;
        }

        // same path as "svm start <name>", so the name check still applies
        let reply = handle(&format!("start {name}"));

        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // goes to /var/log/svmd.log like every other command
        println!(
            "{time} autostart {name} -> {}",
            reply.lines().last().unwrap_or("")
        );
    }
}

// memory (KiB) and vcpu count for a running domain
fn domain_info(id: &str) -> std::io::Result<(String, usize)> {
    let mut stream = UnixStream::connect(XENSTORED_SOCKET)?;

    let mem = xs_request(
        &mut stream,
        XS_READ,
        &format!("/local/domain/{id}/memory/target"),
        &[],
    )?;

    // cpu/ has one child per vcpu (0, 1, ...), so counting them = vcpu count
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

// finds the domain id of a running vm by its name.
// None = no running domain with that name
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

        // first match wins, no need to look at the rest
        if String::from_utf8_lossy(&name_maybe) == name {
            return Ok(Some(id.to_string()));
        }
    }

    Ok(None)
}

// names of every running domain (Domain-0 included)
fn running_vms() -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();

    // every domain id is a folder under /local/domain, each one has a name key
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

// reply for "status <name>"
fn status(name: &str) -> String {
    // without this, any made up name would just show up as "stopped"
    if !defined_vms().iter().any(|vm| vm == name) {
        return "err unknown vm".to_string();
    }

    // not in xenstore = not running
    let id = match domid_of(name) {
        Ok(Some(id)) => id,
        Ok(None) => return "state stopped\nok".to_string(),
        Err(e) => return format!("err xenstore: {e}"),
    };

    let (mem, vcpus) = match domain_info(&id) {
        Ok(info) => info,
        Err(e) => return format!("err xenstore: {e}"),
    };

    // xenstore gives KiB, /1024 = MiB
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

        // every defined vm + running/stopped
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
        "start" => start_vm(arg),
        "stop" => xl(&["destroy", arg]),

        // these build their own full reply
        "status" => return status(arg),
        "autostart" => return autostart_cmd(arg),
        "prefs" => return prefs_cmd(arg),

        _ => Err("unknown command".to_string()),
    };

    match result {
        Ok(()) => "ok".to_string(),
        // xl errors can be a few lines long, keep the reply on one line
        Err(err) => format!("err {}", err.replace('\n', " ")),
    }
}

// reply for "prefs <vm> <key> <value>", like qvm-prefs.
// overrides get saved on the settings disk in /var/lib/sose/prefs/<vm> as key=value
// lines, and start_vm adds them to `xl create` when the vm starts.
//
// todo: "prefs <vm>" with nothing after it should show the prefs
fn prefs_cmd(arg: &str) -> String {
    // name comes first here: "test memory 256" -> ("test", "memory 256")
    let (name, rest) = arg.split_once(' ').unwrap_or((arg, ""));

    // "memory 256" -> ("memory", "256"), a key without a value is a usage error
    let Some((key, value)) = rest.split_once(' ') else {
        return "err usage: prefs <vm> memory|vcpus <number> | netvm <vm|none>".to_string();
    };

    // the name ends up in a file path, so it has to be safe and a real vm.
    // checked before anything touches the disk
    if !valid_name(name) {
        return "err bad vm name".to_string();
    }
    if !defined_vms().iter().any(|vm| vm == name) {
        return "err unknown vm".to_string();
    }

    // each key has its own checks. anything not listed here is rejected,
    // so a pref can't point a vm at some other kernel or disk
    match key {
        // only plain numbers above 0, these go straight onto the xl command line
        "memory" | "vcpus" => {
            let Ok(n) = value.parse::<u32>() else {
                return "err value must be a number".to_string();
            };

            if n == 0 {
                return "err cannot use a value below 1".to_string();
            };
        }

        // which vm this vm gets its network from
        "netvm" => {
            if value.is_empty() {
                return "err cannot set null to netvm".to_string();
            } else if value == name {
                return "err cannot name to netvm".to_string();
            }

            if value != "none" && !defined_vms().iter().any(|vm| vm == value) {
                return "err not a real vm".to_string();
            }
        }

        _ => return "err unknown pref".to_string(),
    }

    // first time there's no prefs folder yet
    if let Err(e) = std::fs::create_dir_all("/var/lib/sose/prefs") {
        return format!("err {e}");
    }

    // no file yet just means no overrides
    let read_prefs =
        std::fs::read_to_string(format!("/var/lib/sose/prefs/{name}")).unwrap_or_default();
    let mut new_prefs: Vec<String> = Vec::new();

    // keep every line except the old one for this key
    for line in read_prefs.lines() {
        if !line.starts_with(&format!("{key}=")) {
            new_prefs.push(line.to_string());
        }
    }

    // then the new value goes at the end
    new_prefs.push(format!("{key}={value}"));

    if let Err(e) = std::fs::write(
        format!("/var/lib/sose/prefs/{name}"),
        new_prefs.join("\n") + "\n",
    ) {
        return format!("err {e}");
    }

    "ok".to_string()
}

// starts a vm with its prefs on top of the .cfg.
// xl lets key=value after the cfg path override that setting:
//   xl create /etc/sose/vms/test.cfg memory=256 vcpus=2
fn start_vm(name: &str) -> Result<(), String> {
    let mut args = vec!["create".to_string(), format!("{VM_DIR}/{name}.cfg")];

    // no prefs file means the vm just uses its .cfg
    let prefs_file =
        std::fs::read_to_string(format!("/var/lib/sose/prefs/{name}")).unwrap_or_default();

    // the file is key=value lines, most of them go to xl as they are
    for line in prefs_file.lines() {
        // except netvm, xl doesn't know that one. it becomes the vm's network card:
        // none = no card at all, otherwise a card served by that vm (sys-net),
        // plugged into its vmbr0 by vif-sose
        if let Some(netvm) = line.strip_prefix("netvm=") {
            if netvm == "none" {
                args.push("vif=[]".to_string())
            } else {
                args.push(format!(
                    "vif=['backend={netvm},bridge=vmbr0,script=vif-sose']"
                ))
            }
        } else {
            args.push(line.to_string());
        }
    }

    // xl() takes &str, so borrow each String (args still owns the text)
    let refs: Vec<&str> = args.iter().map(|s| s.as_str()).collect();

    xl(&refs)
}

// reply for "autostart set|remove <name>" and "autostart list"
fn autostart_cmd(arg: &str) -> String {
    let (action, name) = arg.split_once(' ').unwrap_or((arg, ""));

    match action {
        // add a vm to the autostart file (only once)
        "set" => {
            if !valid_name(name) {
                return "err bad vm name".to_string();
            }
            if !defined_vms().iter().any(|vm| vm == name) {
                return "err unknown vm".to_string();
            }

            let mut text = std::fs::read_to_string(AUTOSTART_FILE).unwrap_or_default();

            // already in there, nothing to do
            if text.lines().any(|line| line.trim() == name) {
                return "ok".to_string();
            }

            text.push_str(name);
            text.push('\n');

            if let Err(e) = std::fs::write(AUTOSTART_FILE, text) {
                return format!("err {e}");
            }
            "ok".to_string()
        }

        // rewrite the file with every line except this vm
        "remove" => {
            if !valid_name(name) {
                return "err bad vm name".to_string();
            }
            if !defined_vms().iter().any(|vm| vm == name) {
                return "err unknown vm".to_string();
            }

            let text = std::fs::read_to_string(AUTOSTART_FILE).unwrap_or_default();
            let others = text.lines().filter(|line| line.trim() != name);

            if let Err(e) =
                std::fs::write(AUTOSTART_FILE, others.collect::<Vec<_>>().join("\n") + "\n")
            {
                return format!("err {e}");
            }
            "ok".to_string()
        }

        // one name per line, then ok
        "list" => {
            let text = std::fs::read_to_string(AUTOSTART_FILE).unwrap_or_default();
            let mut reply = String::new();

            for line in text.lines() {
                reply.push_str(&format!("{line}\n"));
            }

            reply.push_str("ok");
            reply
        }

        _ => "err usage: autostart set|remove <name> | list".to_string(),
    }
}

// one thread per connected client, handles its commands until it disconnects
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

                // log: time, command, last line of the reply (ok / err ...)
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

    // Permission bindings so normal users cannot access the svm sock path.
    set_permissions(SVM_SOCK_PATH, Permissions::from_mode(0o600))?;

    // start the vms listed in the autostart file before taking commands
    autostart();

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
