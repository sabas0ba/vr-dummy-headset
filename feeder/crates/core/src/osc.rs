//! Minimal OSC 1.0 encoder and UDP sender (VRChat OSC input, tracking and
//! avatar parameters).

use std::io;
use std::net::{ToSocketAddrs, UdpSocket};

use crate::command::OscArg;

fn pad4(buf: &mut Vec<u8>) {
    while !buf.len().is_multiple_of(4) {
        buf.push(0);
    }
}

fn push_string(buf: &mut Vec<u8>, s: &str) {
    buf.extend_from_slice(s.as_bytes());
    buf.push(0);
    pad4(buf);
}

/// Encodes one OSC message (address plus typed arguments).
pub fn encode_message(address: &str, args: &[OscArg]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    push_string(&mut buf, address);
    let mut tags = String::with_capacity(args.len() + 1);
    tags.push(',');
    for a in args {
        tags.push(match a {
            OscArg::Int(_) => 'i',
            OscArg::Float(_) => 'f',
            OscArg::Str(_) => 's',
            OscArg::Bool(true) => 'T',
            OscArg::Bool(false) => 'F',
        });
    }
    push_string(&mut buf, &tags);
    for a in args {
        match a {
            OscArg::Int(v) => buf.extend_from_slice(&v.to_be_bytes()),
            OscArg::Float(v) => buf.extend_from_slice(&v.to_be_bytes()),
            OscArg::Str(s) => push_string(&mut buf, s),
            OscArg::Bool(_) => {}
        }
    }
    buf
}

/// UDP sender bound to an ephemeral local port.
pub struct OscSender {
    socket: UdpSocket,
    target: String,
}

impl OscSender {
    /// Creates a sender for `host:port`. The target is resolved on each send.
    pub fn new(host: &str, port: u16) -> io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        Ok(Self {
            socket,
            target: format!("{host}:{port}"),
        })
    }

    /// Target as `host:port`.
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Sends one message.
    pub fn send(&self, address: &str, args: &[OscArg]) -> io::Result<()> {
        let packet = encode_message(address, args);
        let addr =
            self.target.to_socket_addrs()?.next().ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "OSC target did not resolve")
            })?;
        self.socket.send_to(&packet, addr).map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn int_message_layout() {
        let bytes = encode_message("/input/Jump", &[OscArg::Int(1)]);
        let expected: Vec<u8> = [
            b"/input/Jump\0".as_slice(),
            b",i\0\0".as_slice(),
            &1i32.to_be_bytes(),
        ]
        .concat();
        assert_eq!(bytes, expected);
        assert_eq!(bytes.len() % 4, 0);
    }

    #[test]
    fn mixed_arguments() {
        let bytes = encode_message(
            "/a",
            &[
                OscArg::Float(0.5),
                OscArg::Str("hi".to_owned()),
                OscArg::Bool(true),
            ],
        );
        let expected: Vec<u8> = [
            b"/a\0\0".as_slice(),
            b",fsT\0\0\0\0".as_slice(),
            &0.5f32.to_be_bytes(),
            b"hi\0\0".as_slice(),
        ]
        .concat();
        assert_eq!(bytes, expected);
    }

    #[test]
    fn address_padding_when_already_aligned() {
        // "/abc" is 4 bytes; the terminator forces another 4-byte block.
        let bytes = encode_message("/abc", &[]);
        assert_eq!(&bytes[..8], b"/abc\0\0\0\0");
        assert_eq!(&bytes[8..], b",\0\0\0");
    }
}
