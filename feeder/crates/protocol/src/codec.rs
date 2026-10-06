//! Frame encoding and decoding.

use crate::types::{
    DeviceClass, DeviceInfo, Display, DisplayFlags, Hello, HelloAck, Input, InputKind, LogLevel,
    LogRecord, Message, MsgType, Pose, PoseFlags, TrackingResult,
};
use crate::{HEADER_LEN, MAX_PAYLOAD_LEN, PROTOCOL_VERSION, sizes};

/// Reason a frame could not be decoded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// More bytes are needed to complete the frame.
    Incomplete,
    /// The header version differs from `PROTOCOL_VERSION`. The frame was skipped.
    VersionMismatch(u16),
    /// The header type is not known. The frame was skipped.
    UnknownType(u16),
    /// The payload length exceeds `MAX_PAYLOAD_LEN`. The connection must be closed.
    PayloadTooLarge(u32),
    /// The payload does not match the layout of its type. The frame was skipped.
    Malformed(&'static str),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Incomplete => write!(f, "incomplete frame"),
            Self::VersionMismatch(v) => write!(f, "protocol version mismatch: {v}"),
            Self::UnknownType(t) => write!(f, "unknown message type: {t}"),
            Self::PayloadTooLarge(n) => write!(f, "payload too large: {n} bytes"),
            Self::Malformed(what) => write!(f, "malformed payload: {what}"),
        }
    }
}

impl std::error::Error for DecodeError {}

struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn with_capacity(n: usize) -> Self {
        Self {
            buf: Vec::with_capacity(n),
        }
    }
    fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    fn u16(&mut self, v: u16) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }
    fn zeros(&mut self, n: usize) {
        self.buf.resize(self.buf.len() + n, 0);
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(DecodeError::Malformed("length overflow"))?;
        if end > self.buf.len() {
            return Err(DecodeError::Malformed("payload shorter than layout"));
        }
        let slice = &self.buf[self.pos..end];
        self.pos = end;
        Ok(slice)
    }
    fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    fn u64(&mut self) -> Result<u64, DecodeError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }
    fn f32(&mut self) -> Result<f32, DecodeError> {
        Ok(f32::from_bits(self.u32()?))
    }
    fn f64(&mut self) -> Result<f64, DecodeError> {
        Ok(f64::from_bits(self.u64()?))
    }
    fn f32x4(&mut self) -> Result<[f32; 4], DecodeError> {
        Ok([self.f32()?, self.f32()?, self.f32()?, self.f32()?])
    }
    fn f64x3(&mut self) -> Result<[f64; 3], DecodeError> {
        Ok([self.f64()?, self.f64()?, self.f64()?])
    }
    fn f64x4(&mut self) -> Result<[f64; 4], DecodeError> {
        Ok([self.f64()?, self.f64()?, self.f64()?, self.f64()?])
    }
    fn skip(&mut self, n: usize) -> Result<(), DecodeError> {
        self.take(n).map(|_| ())
    }
    fn remaining(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }
}

/// Encodes a message as a complete frame (header plus payload).
///
/// # Panics
///
/// Panics if a `HelloAck` lists more than `u16::MAX` devices or a serial longer
/// than `sizes::SERIAL - 1` bytes, or if a log text exceeds `MAX_PAYLOAD_LEN`.
/// Both are programming errors on the sending side.
pub fn encode(msg: &Message) -> Vec<u8> {
    let mut w = Writer::with_capacity(HEADER_LEN + 128);
    w.zeros(HEADER_LEN);
    match msg {
        Message::Hello(h) => {
            w.u32(h.protocol_version);
            w.u32(h.client_flags);
        }
        Message::HelloAck(a) => {
            w.u32(a.protocol_version);
            let count = u16::try_from(a.devices.len()).expect("device count fits in u16");
            w.u16(count);
            w.u16(0);
            for d in &a.devices {
                w.u16(d.device_id);
                w.u8(d.device_class as u8);
                w.u8(0);
                let bytes = d.serial.as_bytes();
                assert!(
                    bytes.len() < sizes::SERIAL,
                    "serial must be shorter than {} bytes",
                    sizes::SERIAL
                );
                w.buf.extend_from_slice(bytes);
                w.zeros(sizes::SERIAL - bytes.len());
            }
        }
        Message::Pose(p) => {
            w.u16(p.device_id);
            w.u8(p.flags.0);
            w.u8(p.tracking_result as u8);
            w.u32(0);
            w.u64(p.timestamp_us);
            for v in p.position {
                w.f64(v);
            }
            for v in p.rotation {
                w.f64(v);
            }
            for v in p.linear_velocity {
                w.f64(v);
            }
            for v in p.angular_velocity {
                w.f64(v);
            }
        }
        Message::Input(i) => {
            w.u16(i.device_id);
            w.u16(i.component_id);
            w.u8(i.kind as u8);
            w.zeros(3);
            w.f32(i.value);
            w.f64(i.time_offset_s);
        }
        Message::Display(d) => {
            w.u32(d.flags.0);
            w.f32(d.ipd_m);
            for v in d.projection_left {
                w.f32(v);
            }
            for v in d.projection_right {
                w.f32(v);
            }
            w.u8(u8::from(d.swap_eyes));
            w.zeros(3);
        }
        Message::Log(l) => {
            w.u8(l.level as u8);
            w.zeros(3);
            w.u64(l.timestamp_us);
            w.buf.extend_from_slice(l.text.as_bytes());
        }
        Message::Goodbye => {}
    }
    let payload_len = w.buf.len() - HEADER_LEN;
    assert!(payload_len <= MAX_PAYLOAD_LEN, "payload exceeds MAX_PAYLOAD_LEN");
    let len = u32::try_from(payload_len).expect("payload length fits in u32");
    w.buf[0..4].copy_from_slice(&len.to_le_bytes());
    w.buf[4..6].copy_from_slice(&(msg.msg_type() as u16).to_le_bytes());
    w.buf[6..8].copy_from_slice(&PROTOCOL_VERSION.to_le_bytes());
    w.buf
}

/// Parsed frame header.
#[derive(Clone, Copy, Debug)]
struct Header {
    payload_len: usize,
    msg_type: u16,
    version: u16,
}

fn read_header(buf: &[u8]) -> Result<Header, DecodeError> {
    if buf.len() < HEADER_LEN {
        return Err(DecodeError::Incomplete);
    }
    let payload_len = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let msg_type = u16::from_le_bytes([buf[4], buf[5]]);
    let version = u16::from_le_bytes([buf[6], buf[7]]);
    if payload_len as usize > MAX_PAYLOAD_LEN {
        return Err(DecodeError::PayloadTooLarge(payload_len));
    }
    Ok(Header {
        payload_len: payload_len as usize,
        msg_type,
        version,
    })
}

/// Decodes one frame from the start of `buf`.
///
/// Returns the message and the number of bytes consumed. On
/// `VersionMismatch`, `UnknownType` and `Malformed` the frame is complete and
/// the caller should skip `HEADER_LEN + payload_len` bytes; `Decoder` does
/// this automatically.
pub fn decode(buf: &[u8]) -> Result<(Message, usize), DecodeError> {
    let header = read_header(buf)?;
    let frame_len = HEADER_LEN + header.payload_len;
    if buf.len() < frame_len {
        return Err(DecodeError::Incomplete);
    }
    if header.version != PROTOCOL_VERSION {
        return Err(DecodeError::VersionMismatch(header.version));
    }
    let msg_type = MsgType::from_u16(header.msg_type)
        .ok_or(DecodeError::UnknownType(header.msg_type))?;
    let payload = &buf[HEADER_LEN..frame_len];
    let msg = decode_payload(msg_type, payload)?;
    Ok((msg, frame_len))
}

fn decode_payload(msg_type: MsgType, payload: &[u8]) -> Result<Message, DecodeError> {
    let mut r = Reader::new(payload);
    match msg_type {
        MsgType::Hello => {
            expect_len(payload, sizes::HELLO)?;
            Ok(Message::Hello(Hello {
                protocol_version: r.u32()?,
                client_flags: r.u32()?,
            }))
        }
        MsgType::HelloAck => decode_hello_ack(&mut r, payload.len()).map(Message::HelloAck),
        MsgType::PoseUpdate => {
            expect_len(payload, sizes::POSE)?;
            decode_pose(&mut r).map(Message::Pose)
        }
        MsgType::InputUpdate => {
            expect_len(payload, sizes::INPUT)?;
            decode_input(&mut r).map(Message::Input)
        }
        MsgType::DisplayUpdate => {
            expect_len(payload, sizes::DISPLAY)?;
            decode_display(&mut r).map(Message::Display)
        }
        MsgType::LogRecord => decode_log(&mut r, payload.len()).map(Message::Log),
        MsgType::Goodbye => {
            expect_len(payload, 0)?;
            Ok(Message::Goodbye)
        }
    }
}

fn decode_hello_ack(r: &mut Reader<'_>, payload_len: usize) -> Result<HelloAck, DecodeError> {
    let protocol_version = r.u32()?;
    let count = r.u16()? as usize;
    r.skip(2)?;
    if payload_len != sizes::HELLO_ACK_HEAD + count * sizes::DEVICE_INFO {
        return Err(DecodeError::Malformed("payload length differs from layout"));
    }
    let mut devices = Vec::with_capacity(count);
    for _ in 0..count {
        let device_id = r.u16()?;
        let class = r.u8()?;
        r.skip(1)?;
        let raw = r.take(sizes::SERIAL)?;
        let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
        let serial = std::str::from_utf8(&raw[..end])
            .map_err(|_| DecodeError::Malformed("serial is not UTF-8"))?
            .to_owned();
        devices.push(DeviceInfo {
            device_id,
            device_class: DeviceClass::from_u8(class)
                .ok_or(DecodeError::Malformed("unknown device class"))?,
            serial,
        });
    }
    Ok(HelloAck {
        protocol_version,
        devices,
    })
}

fn decode_pose(r: &mut Reader<'_>) -> Result<Pose, DecodeError> {
    let device_id = r.u16()?;
    let flags = PoseFlags(r.u8()?);
    let tracking_result = TrackingResult::from_u8(r.u8()?)
        .ok_or(DecodeError::Malformed("unknown tracking result"))?;
    r.skip(4)?;
    Ok(Pose {
        device_id,
        flags,
        tracking_result,
        timestamp_us: r.u64()?,
        position: r.f64x3()?,
        rotation: r.f64x4()?,
        linear_velocity: r.f64x3()?,
        angular_velocity: r.f64x3()?,
    })
}

fn decode_input(r: &mut Reader<'_>) -> Result<Input, DecodeError> {
    let device_id = r.u16()?;
    let component_id = r.u16()?;
    let kind = InputKind::from_u8(r.u8()?).ok_or(DecodeError::Malformed("unknown input kind"))?;
    r.skip(3)?;
    Ok(Input {
        device_id,
        component_id,
        kind,
        value: r.f32()?,
        time_offset_s: r.f64()?,
    })
}

fn decode_display(r: &mut Reader<'_>) -> Result<Display, DecodeError> {
    let flags = DisplayFlags(r.u32()?);
    let ipd_m = r.f32()?;
    let projection_left = r.f32x4()?;
    let projection_right = r.f32x4()?;
    let swap_eyes = r.u8()? != 0;
    Ok(Display {
        flags,
        ipd_m,
        projection_left,
        projection_right,
        swap_eyes,
    })
}

fn decode_log(r: &mut Reader<'_>, payload_len: usize) -> Result<LogRecord, DecodeError> {
    if payload_len < sizes::LOG_HEAD {
        return Err(DecodeError::Malformed("payload shorter than layout"));
    }
    let level = LogLevel::from_u8(r.u8()?).ok_or(DecodeError::Malformed("unknown log level"))?;
    r.skip(3)?;
    let timestamp_us = r.u64()?;
    let text = std::str::from_utf8(r.remaining())
        .map_err(|_| DecodeError::Malformed("log text is not UTF-8"))?
        .to_owned();
    Ok(LogRecord {
        level,
        timestamp_us,
        text,
    })
}

fn expect_len(payload: &[u8], expected: usize) -> Result<(), DecodeError> {
    if payload.len() == expected {
        Ok(())
    } else {
        Err(DecodeError::Malformed("payload length differs from layout"))
    }
}

/// Incremental frame reader over a byte stream.
#[derive(Default, Debug)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    /// Creates an empty decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends bytes received from the transport.
    pub fn push(&mut self, bytes: &[u8]) {
        self.buf.extend_from_slice(bytes);
    }

    /// Returns the next complete frame, or `None` when more bytes are needed.
    ///
    /// Frames with a bad version, unknown type or malformed payload are
    /// consumed and reported as errors so that the stream stays in sync.
    /// `PayloadTooLarge` is not recoverable; the caller must drop the
    /// connection.
    pub fn next_frame(&mut self) -> Option<Result<Message, DecodeError>> {
        let header = match read_header(&self.buf) {
            Ok(h) => h,
            Err(DecodeError::Incomplete) => return None,
            Err(e) => return Some(Err(e)),
        };
        let frame_len = HEADER_LEN + header.payload_len;
        if self.buf.len() < frame_len {
            return None;
        }
        let result = decode(&self.buf[..frame_len]).map(|(msg, _)| msg);
        self.buf.drain(..frame_len);
        Some(result)
    }
}
