//! IPC wire format between `driver_vrdummy` and the feeder.
//!
//! The format is specified in `docs/protocol.md`. This crate has no
//! dependencies and performs explicit little-endian encoding so that the
//! layout never depends on struct padding.

mod codec;
mod types;

pub use codec::{DecodeError, Decoder, decode, encode};
pub use types::{
    DeviceClass, DeviceInfo, Display, DisplayFlags, Hello, HelloAck, Input, InputKind, LogLevel,
    LogRecord, Message, MsgType, Pose, PoseFlags, TrackingResult,
};

/// Protocol version carried in every frame header.
pub const PROTOCOL_VERSION: u16 = 1;
/// Length of the fixed frame header in bytes.
pub const HEADER_LEN: usize = 8;
/// Upper bound of a payload; larger frames terminate the connection.
pub const MAX_PAYLOAD_LEN: usize = 65_536;
/// Default pipe name without the `\\.\pipe\` prefix.
pub const DEFAULT_PIPE_NAME: &str = "vrdummy-driver";

/// Fixed payload sizes in bytes.
pub mod sizes {
    /// `Hello` payload.
    pub const HELLO: usize = 8;
    /// `HelloAck` payload before the device array.
    pub const HELLO_ACK_HEAD: usize = 8;
    /// One `DeviceInfo` entry.
    pub const DEVICE_INFO: usize = 40;
    /// Serial field inside `DeviceInfo`.
    pub const SERIAL: usize = 36;
    /// `Pose` payload.
    pub const POSE: usize = 120;
    /// `Input` payload.
    pub const INPUT: usize = 20;
    /// `Display` payload.
    pub const DISPLAY: usize = 44;
    /// `LogRecord` payload before the text.
    pub const LOG_HEAD: usize = 12;
}

/// Device ids assigned by the driver (see `docs/protocol.md` 3.2).
pub mod device_ids {
    /// Head-mounted display.
    pub const HMD: u16 = 0;
    /// Left hand controller.
    pub const CONTROLLER_LEFT: u16 = 1;
    /// Right hand controller.
    pub const CONTROLLER_RIGHT: u16 = 2;
    /// First tracker; subsequent trackers follow consecutively.
    pub const TRACKER_BASE: u16 = 3;
}

/// Input component ids for the HMD (see `docs/protocol.md` 3.4.1).
pub mod hmd_components {
    /// `/input/system/click`
    pub const SYSTEM_CLICK: u16 = 0;
    /// `/proximity`
    pub const PROXIMITY: u16 = 1;
}

/// Input component ids for controllers (see `docs/protocol.md` 3.4.1).
pub mod controller_components {
    /// `/input/system/click`
    pub const SYSTEM_CLICK: u16 = 0;
    /// `/input/a/click`
    pub const A_CLICK: u16 = 1;
    /// `/input/a/touch`
    pub const A_TOUCH: u16 = 2;
    /// `/input/b/click`
    pub const B_CLICK: u16 = 3;
    /// `/input/b/touch`
    pub const B_TOUCH: u16 = 4;
    /// `/input/trigger/value`
    pub const TRIGGER_VALUE: u16 = 5;
    /// `/input/trigger/click`
    pub const TRIGGER_CLICK: u16 = 6;
    /// `/input/trigger/touch`
    pub const TRIGGER_TOUCH: u16 = 7;
    /// `/input/grip/value`
    pub const GRIP_VALUE: u16 = 8;
    /// `/input/grip/click`
    pub const GRIP_CLICK: u16 = 9;
    /// `/input/grip/touch`
    pub const GRIP_TOUCH: u16 = 10;
    /// `/input/joystick/x`
    pub const JOYSTICK_X: u16 = 11;
    /// `/input/joystick/y`
    pub const JOYSTICK_Y: u16 = 12;
    /// `/input/joystick/click`
    pub const JOYSTICK_CLICK: u16 = 13;
    /// `/input/joystick/touch`
    pub const JOYSTICK_TOUCH: u16 = 14;
    /// Number of controller components.
    pub const COUNT: u16 = 15;
}
