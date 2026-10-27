//! The client side of an nng `req0` socket over the `ipc://` transport, the one KiCad's API server listens on (`libs/kinng/src/kinng.cpp`:
//! `nng_rep0_open`, `nng_listener_create( "ipc://<socket>" )`; KiCad's own client, `common/api/api_client.cpp`, is `nng_req0_open` + `nng_dial`).
//!
//! This is the wire format of NNG's scalability protocols, written out rather than linked: the `nng` crate (approved for this) builds the C library with
//! CMake, which neither this box nor every Mac has, and a REQ client over a Unix socket is three framing rules:
//! * the connection opens with an 8-byte header from each side, `00 'S' 'P' 00`, the protocol number (big-endian 16 bits: `req0` is 0x30, `rep0` 0x31)
//!   and two zero bytes (nng `src/sp/transport/ipc/ipc.c`, `ipc_pipe_nego_cb`);
//! * every message is a 1-byte message type (1), its length as a big-endian 64-bit number, then the message (`ipc_pipe_send_start`);
//! * a request's message starts with its 32-bit request id, top bit set, which the reply starts with too (`src/sp/protocol/reqrep0/req.c`, the
//!   "backtrace" `rep0` copies back); a reply whose id is not the last request's is a stale one and is dropped, as `req0` drops it.
//!
//! One request is in flight at a time ([`ReqSocket::request`] waits for its reply), as KiCad's `API_CLIENT` sends them.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::time::Duration;

/// `NNG_REQ0_SELF` and `NNG_REP0_SELF`: the protocol numbers the headers carry.
const PROTO_REQ0: u16 = 0x30;
const PROTO_REP0: u16 = 0x31;

/// A `req0` socket dialled to one `rep0` listener.
pub struct ReqSocket {
    stream: UnixStream,
    next_id: u32,
}

fn header(proto: u16) -> [u8; 8] {
    let p = proto.to_be_bytes();
    [0, b'S', b'P', 0, p[0], p[1], 0, 0]
}

impl ReqSocket {
    /// Dial `socket` (the path, without `ipc://`) and exchange the headers; `timeout` bounds the exchange.
    pub fn connect(socket: &Path, timeout: Duration) -> std::io::Result<ReqSocket> {
        let mut stream = UnixStream::connect(socket)?;
        stream.set_read_timeout(Some(timeout))?;
        stream.set_write_timeout(Some(timeout))?;
        stream.write_all(&header(PROTO_REQ0))?;
        let mut theirs = [0u8; 8];
        stream.read_exact(&mut theirs)?;
        if theirs[..4] != [0, b'S', b'P', 0] || u16::from_be_bytes([theirs[4], theirs[5]]) != PROTO_REP0 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("not an nng rep0 socket (header {theirs:02x?})")));
        }
        // A request id starts anywhere (nng picks a random one); the process id keeps two clients' ids apart in a log.
        Ok(ReqSocket { stream, next_id: std::process::id() & 0x7fff_ffff })
    }

    /// Send `body` and wait up to `timeout` for its reply's body.
    pub fn request(&mut self, body: &[u8], timeout: Duration) -> std::io::Result<Vec<u8>> {
        self.next_id = self.next_id.wrapping_add(1) & 0x7fff_ffff;
        let id = self.next_id | 0x8000_0000;
        let mut frame = Vec::with_capacity(13 + body.len());
        frame.push(1);
        frame.extend_from_slice(&((body.len() + 4) as u64).to_be_bytes());
        frame.extend_from_slice(&id.to_be_bytes());
        frame.extend_from_slice(body);
        self.stream.set_write_timeout(Some(timeout))?;
        self.stream.write_all(&frame)?;
        self.stream.set_read_timeout(Some(timeout))?;
        loop {
            let mut head = [0u8; 9];
            self.stream.read_exact(&mut head)?;
            if head[0] != 1 {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("unknown nng ipc message type {}", head[0])));
            }
            let len = u64::from_be_bytes(head[1..9].try_into().unwrap());
            if !(4..=(1u64 << 32)).contains(&len) {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("nng ipc message of {len} bytes")));
            }
            let mut msg = vec![0u8; len as usize];
            self.stream.read_exact(&mut msg)?;
            if u32::from_be_bytes(msg[..4].try_into().unwrap()) == id {
                msg.drain(..4);
                return Ok(msg);
            }
            // The reply to an earlier request that timed out: `req0` drops it.
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixListener;

    /// A `rep0` peer in a thread: it answers each request with the body reversed, first sending a stale reply to an id that is not the request's.
    #[test]
    fn a_request_gets_its_own_reply_and_a_stale_one_is_dropped() {
        let dir = std::env::temp_dir().join(format!("eda-sp-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let sock = dir.join("rep.sock");
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock).unwrap();
        let server = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut h = [0u8; 8];
            s.read_exact(&mut h).unwrap();
            assert_eq!(h, header(PROTO_REQ0));
            s.write_all(&header(PROTO_REP0)).unwrap();
            for _ in 0..2 {
                let mut head = [0u8; 9];
                s.read_exact(&mut head).unwrap();
                let mut msg = vec![0u8; u64::from_be_bytes(head[1..9].try_into().unwrap()) as usize];
                s.read_exact(&mut msg).unwrap();
                let id = &msg[..4];
                assert!(id[0] & 0x80 != 0, "the request id has its top bit set");
                let send = |s: &mut UnixStream, id: &[u8], body: &[u8]| {
                    let mut f = vec![1u8];
                    f.extend_from_slice(&((body.len() + 4) as u64).to_be_bytes());
                    f.extend_from_slice(id);
                    f.extend_from_slice(body);
                    s.write_all(&f).unwrap();
                };
                send(&mut s, &[0x80, 0, 0, 0], b"stale");
                let mut body = msg[4..].to_vec();
                body.reverse();
                send(&mut s, id, &body);
            }
        });
        let mut req = ReqSocket::connect(&sock, Duration::from_secs(5)).unwrap();
        assert_eq!(req.request(b"abc", Duration::from_secs(5)).unwrap(), b"cba");
        assert_eq!(req.request(b"", Duration::from_secs(5)).unwrap(), b"");
        server.join().unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
