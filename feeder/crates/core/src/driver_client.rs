//! Client side of the driver pipe (`docs/protocol.md`).
//!
//! The pipe is opened as an ordinary file and driven from one thread. Reads
//! are issued only for bytes that `PeekNamedPipe` reports as available, so a
//! read never blocks and never stalls a concurrent write on the same handle
//! (synchronous pipe handles serialise their operations).

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use vrdummy_protocol::{
    DecodeError, Decoder, DeviceInfo, Hello, Message, PROTOCOL_VERSION, encode,
};

use crate::win32::{pipe_bytes_available, pipe_path};

/// Connected driver.
pub struct DriverClient {
    file: File,
    decoder: Decoder,
    pending: VecDeque<Message>,
    devices: Vec<DeviceInfo>,
    alive: bool,
}

impl DriverClient {
    /// Opens the pipe and performs the handshake.
    pub fn connect(pipe_name: &str, handshake_timeout: Duration) -> io::Result<Self> {
        let path = pipe_path(pipe_name);
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let mut client = Self {
            file,
            decoder: Decoder::new(),
            pending: VecDeque::new(),
            devices: Vec::new(),
            alive: true,
        };
        client.send(&Message::Hello(Hello {
            protocol_version: u32::from(PROTOCOL_VERSION),
            client_flags: 0,
        }))?;
        let deadline = Instant::now() + handshake_timeout;
        loop {
            client.pump()?;
            if let Some(msg) = client.pending.pop_front() {
                match msg {
                    Message::HelloAck(ack) => {
                        if ack.protocol_version != u32::from(PROTOCOL_VERSION) {
                            log::warn!(
                                "driver protocol version {} differs from {}",
                                ack.protocol_version,
                                PROTOCOL_VERSION
                            );
                        }
                        client.devices = ack.devices;
                        log::info!(
                            "connected to driver on {path} ({} devices)",
                            client.devices.len()
                        );
                        return Ok(client);
                    }
                    other => {
                        return Err(io::Error::other(format!(
                            "unexpected message during handshake: {:?}",
                            other.msg_type()
                        )));
                    }
                }
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "driver did not answer the handshake",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Devices reported by the driver in the handshake.
    pub fn devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    /// Sends one message.
    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        let frame = encode(message);
        log::trace!(
            "pipe: writing {} bytes ({:?})",
            frame.len(),
            message.msg_type()
        );
        let result = self.file.write_all(&frame).and_then(|()| self.file.flush());
        log::trace!("pipe: write finished ({:?})", result.as_ref().err());
        if result.is_err() {
            self.alive = false;
        }
        result
    }

    /// Reads every byte already available and decodes complete frames.
    fn pump(&mut self) -> io::Result<()> {
        loop {
            let available = match pipe_bytes_available(&self.file) {
                Ok(n) => n,
                Err(e) => {
                    self.alive = false;
                    return Err(e);
                }
            };
            if available == 0 {
                return Ok(());
            }
            log::trace!("pipe: {available} bytes available");
            let mut buf = vec![0u8; available.min(64 * 1024)];
            let n = match self.file.read(&mut buf) {
                Ok(0) => {
                    self.alive = false;
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "driver closed the pipe",
                    ));
                }
                Ok(n) => n,
                Err(e) => {
                    self.alive = false;
                    return Err(e);
                }
            };
            self.decoder.push(&buf[..n]);
            while let Some(frame) = self.decoder.next_frame() {
                match frame {
                    Ok(msg) => self.pending.push_back(msg),
                    Err(DecodeError::PayloadTooLarge(size)) => {
                        self.alive = false;
                        return Err(io::Error::other(format!(
                            "driver sent an oversized frame ({size} bytes)"
                        )));
                    }
                    Err(e) => log::warn!("skipped frame from driver: {e}"),
                }
            }
        }
    }

    /// Returns the next message received from the driver, if any.
    /// Transport errors mark the connection dead (see [`Self::alive`]).
    pub fn try_recv(&mut self) -> Option<Message> {
        if self.alive
            && self.pending.is_empty()
            && let Err(e) = self.pump()
        {
            log::debug!("driver pipe read ended: {e}");
        }
        self.pending.pop_front()
    }

    /// Whether the connection is still usable.
    pub fn alive(&self) -> bool {
        self.alive
    }
}

impl Drop for DriverClient {
    fn drop(&mut self) {
        if self.alive {
            let _ = self.send(&Message::Goodbye);
        }
    }
}
