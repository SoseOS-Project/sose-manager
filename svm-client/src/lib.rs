// svm-client: what svmd and the svm tools share.
// svmd listens on this socket, tools send it one text command per line.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

pub const SVM_SOCK_PATH: &str = "/run/svmd.sock";

// sends one command to svmd and gives back its whole reply (can be a few lines for list)
pub fn svm_request(line: &str) -> std::io::Result<String> {
    let stream = UnixStream::connect(SVM_SOCK_PATH)?;
    // writeln adds the \n, that's how svmd knows the command is done
    writeln!(&stream, "{line}")?;

    let mut reply = String::new();
    for l in BufReader::new(&stream).lines() {
        let l = l?;
        // last line is always ok or err, have to stop there ourselves
        // since svmd keeps the connection open and lines() would just wait.
        // "err " with the space: a vm called "errand" in a list isn't the end
        let done = l == "ok" || l.starts_with("err ");
        reply.push_str(&l);
        reply.push('\n');
        if done {
            break;
        };
    }
    Ok(reply)
}
