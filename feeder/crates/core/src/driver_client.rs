//! Client side of the driver pipe (`docs/protocol.md`).
//!
//! The pipe is opened as an ordinary file. A reader thread decodes frames and
//! forwards them over a channel; writes happen on the caller's thread.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

use vrdummy_protocol::{
    DecodeError, Decoder, DeviceInfo, Hello, Message, PROTOCOL_VERSION, encode,
};

use crate::win32::pipe_path;

/// Connected driver.
pub struct DriverClient {
    writer: File,
    rx: Receiver<Message>,
    reader: Option<JoinHandle<()>>,
    devices: Vec<DeviceInfo>,
}

impl DriverClient {
    /// Opens the pipe, performs the handshake and starts the reader thread.
    pub fn connect(pipe_name: &str, handshake_timeout: Duration) -> io::Result<Self> {
        let path = pipe_path(pipe_name);
        let file = OpenOptions::new().read(true).write(true).open(&path)?;
        let reader_file = file.try_clone()?;
        let (tx, rx) = mpsc::channel();
        let reader = std::thread::Builder::new()
            .name("vrdummy-driver-reader".to_owned())
            .spawn(move || reader_loop(reader_file, &tx))?;

        let mut client = Self {
            writer: file,
            rx,
            reader: Some(reader),
            devices: Vec::new(),
        };
        client.send(&Message::Hello(Hello {
            protocol_version: u32::from(PROTOCOL_VERSION),
            client_flags: 0,
        }))?;
        match client.rx.recv_timeout(handshake_timeout) {
            Ok(Message::HelloAck(ack)) => {
                if ack.protocol_version != u32::from(PROTOCOL_VERSION) {
                    log::warn!(
                        "driver protocol version {} differs from {}",
                        ack.protocol_version,
                        PROTOCOL_VERSION
                    );
                }
                client.devices = ack.devices;
                log::info!("connected to driver on {path} ({} devices)", client.devices.len());
                Ok(client)
            }
            Ok(other) => Err(io::Error::other(format!(
                "unexpected message during handshake: {:?}",
                other.msg_type()
            ))),
            Err(RecvTimeoutError::Timeout) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "driver did not answer the handshake",
            )),
            Err(RecvTimeoutError::Disconnected) => Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "driver closed the pipe during handshake",
            )),
        }
    }

    /// Devices reported by the driver in the handshake.
    pub fn devices(&self) -> &[DeviceInfo] {
        &self.devices
    }

    /// Sends one message.
    pub fn send(&mut self, message: &Message) -> io::Result<()> {
        let frame = encode(message);
        self.writer.write_all(&frame)?;
        self.writer.flush()
    }

    /// Returns a message received from the driver, if any is queued.
    pub fn try_recv(&self) -> Option<Message> {
        self.rx.try_recv().ok()
    }

    /// Whether the reader thread is still running.
    pub fn alive(&self) -> bool {
        self.reader.as_ref().is_some_and(|h| !h.is_finished())
    }
}

impl Drop for DriverClient {
    fn drop(&mut self) {
        let _ = self.send(&Message::Goodbye);
        // Closing the writer handle ends the reader's blocking read once the
        // server disconnects; do not join so that drop never blocks.
        self.reader.take();
    }
}

fn reader_loop(mut file: File, tx: &mpsc::Sender<Message>) {
    let mut decoder = Decoder::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = match file.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                log::debug!("driver pipe read ended: {e}");
                break;
            }
        };
        decoder.push(&buf[..n]);
        while let Some(frame) = decoder.next_frame() {
            match frame {
                Ok(msg) => {
                    if tx.send(msg).is_err() {
                        return;
                    }
                }
                Err(DecodeError::PayloadTooLarge(n)) => {
                    log::error!("driver sent an oversized frame ({n} bytes); closing");
                    return;
                }
                Err(e) => log::warn!("skipped frame from driver: {e}"),
            }
        }
    }
    log::info!("driver pipe closed");
}
