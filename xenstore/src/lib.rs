// xenstore client, talks to xenstored over its unix socket.
//
// every message is a 16 byte header (type, req_id, tx_id, len, all little endian u32)
// and then len bytes of payload. layout is in xen/include/public/io/xs_wire.h

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;

// message types, the number is the position in enum xsd_sockmsg_type
pub const XS_READ: u32 = 2;
pub const XS_DIRECTORY: u32 = 1;
pub const XS_WRITE: u32 = 11;
pub const XS_WATCH: u32 = 4;
pub const XS_WATCH_EVENT: u32 = 15;

const XS_ERROR: u32 = 16;

// only one request in flight at a time so req_id can just stay 0.
// tx_id 0 = not in a transaction
const REQ_ID: u32 = 0;
const TX_ID: u32 = 0;

// only exists in dom0 once xencommons has started xenstored
pub const XENSTORED_SOCKET: &str = "/run/xenstored/socket";

// header, then path + NUL, then the value (no NUL after the value, len covers it)
fn build_request(msg_type: u32, path: &str, value: &[u8]) -> Vec<u8> {
    let mut msg = Vec::new();

    msg.extend_from_slice(&msg_type.to_le_bytes());
    msg.extend_from_slice(&REQ_ID.to_le_bytes());
    msg.extend_from_slice(&TX_ID.to_le_bytes());

    let len = (path.len() + 1 + value.len()) as u32;
    msg.extend_from_slice(&len.to_le_bytes());
    msg.extend_from_slice(path.as_bytes());
    msg.push(0);
    msg.extend_from_slice(value);

    msg
}

// sends one request and gives back the reply payload.
// if xenstored says no you get an Err with its error name (ENOENT etc)
pub fn xs_request(
    stream: &mut UnixStream,
    msg_type: u32,
    path: &str,
    value: &[u8],
) -> std::io::Result<Vec<u8>> {
    let msg = build_request(msg_type, path, value);
    stream.write_all(&msg)?;

    // header first, its len field says how much payload is coming after
    let mut header = [0u8; 16];
    stream.read_exact(&mut header)?;

    let len = u32::from_le_bytes([header[12], header[13], header[14], header[15]]) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;

    let reply_type = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);

    // error replies are just the error name as text, like "ENOENT\0"
    if reply_type == XS_ERROR {
        let msg = String::from_utf8_lossy(&payload)
            .trim_end_matches('\0')
            .to_string();
        return Err(std::io::Error::other(msg));
    }

    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    // worked these bytes out by hand
    #[test]
    fn write_request() {
        let msg = build_request(XS_WRITE, "a", b"hi");
        assert_eq!(
            msg,
            vec![
                11, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 0, 0, 97, 0, 104, 105
            ]
        );
    }
}
