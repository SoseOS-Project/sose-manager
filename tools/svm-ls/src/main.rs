// svm-ls: lists the running domains and their names, basically a tiny `xl list`.
// talks straight to xenstored for now, later this should ask svmd instead

use std::os::unix::net::UnixStream;
use xenstore::{xs_request, XENSTORED_SOCKET, XS_DIRECTORY, XS_READ};

fn main() -> std::io::Result<()> {

    // one connection, reused for every request
    let mut stream = UnixStream::connect(XENSTORED_SOCKET)?;

    // /local/domain has one child per domain id, reply looks like "0\01\0"
    let payload = xs_request(&mut stream,XS_DIRECTORY, "/local/domain", &[])?;

    // keep the String alive here, split() borrows from it
    let text = String::from_utf8_lossy(&payload);
    let ids = text.split('\0');


    for id in ids {
        // the last NUL leaves an empty piece at the end
        if id.is_empty() { continue; }

        let path = format!("/local/domain/{}/name", id);
        let name = xs_request(&mut stream,XS_READ, &path, &[])?;

        println!("{:?} Name -> {:?}", id, String::from_utf8_lossy(&name))
    }

    Ok(())
}
