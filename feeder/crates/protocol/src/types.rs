//! Message types. Field order follows `docs/protocol.md`.

/// Frame type carried in the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum MsgType {
    /// Feeder to driver handshake.
    Hello = 1,
    /// Driver to feeder handshake reply.
    HelloAck = 2,
    /// Pose of one device.
    PoseUpdate = 3,
    /// One input component of one device.
    InputUpdate = 4,
    /// HMD optical parameters.
    DisplayUpdate = 5,
    /// Log line forwarded by the driver.
    LogRecord = 6,
    /// Orderly shutdown, either direction.
    Goodbye = 7,
}

impl MsgType {
    /// Converts a header value to a known type.
    pub fn from_u16(value: u16) -> Option<Self> {
        match value {
            1 => Some(Self::Hello),
            2 => Some(Self::HelloAck),
            3 => Some(Self::PoseUpdate),
            4 => Some(Self::InputUpdate),
            5 => Some(Self::DisplayUpdate),
            6 => Some(Self::LogRecord),
            7 => Some(Self::Goodbye),
            _ => None,
        }
    }
}

/// Class of a device exposed by the driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceClass {
    /// Head-mounted display.
    Hmd = 1,
    /// Left hand controller.
    ControllerLeft = 2,
    /// Right hand controller.
    ControllerRight = 3,
    /// Generic tracker.
    Tracker = 4,
}

impl DeviceClass {
    /// Converts a wire value to a known class.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Hmd),
            2 => Some(Self::ControllerLeft),
            3 => Some(Self::ControllerRight),
            4 => Some(Self::Tracker),
            _ => None,
        }
    }
}

/// Handshake sent by the feeder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hello {
    /// Protocol version the feeder speaks.
    pub protocol_version: u32,
    /// Reserved, must be zero.
    pub client_flags: u32,
}

/// One device listed in the handshake reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Driver-assigned id.
    pub device_id: u16,
    /// Device class.
    pub device_class: DeviceClass,
    /// Serial number, at most 35 bytes of UTF-8.
    pub serial: String,
}

/// Handshake reply sent by the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HelloAck {
    /// Protocol version the driver speaks.
    pub protocol_version: u32,
    /// Devices the driver registered with SteamVR.
    pub devices: Vec<DeviceInfo>,
}

/// Flags of a pose update.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PoseFlags(pub u8);

impl PoseFlags {
    /// The pose fields are valid.
    pub const POSE_VALID: u8 = 1 << 0;
    /// The device is connected.
    pub const CONNECTED: u8 = 1 << 1;

    /// Valid and connected.
    pub const fn valid_connected() -> Self {
        Self(Self::POSE_VALID | Self::CONNECTED)
    }

    /// Whether the pose fields are valid.
    pub const fn pose_valid(self) -> bool {
        self.0 & Self::POSE_VALID != 0
    }

    /// Whether the device is connected.
    pub const fn connected(self) -> bool {
        self.0 & Self::CONNECTED != 0
    }
}

/// Tracking state, mapped by the driver onto `vr::ETrackingResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TrackingResult {
    /// Not yet tracking.
    Uninitialized = 0,
    /// Tracking normally.
    RunningOk = 1,
    /// Tracking lost.
    RunningOutOfRange = 2,
    /// Calibration in progress.
    Calibrating = 3,
    /// Rotation only.
    FallbackRotationOnly = 4,
}

impl TrackingResult {
    /// Converts a wire value to a known state.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Uninitialized),
            1 => Some(Self::RunningOk),
            2 => Some(Self::RunningOutOfRange),
            3 => Some(Self::Calibrating),
            4 => Some(Self::FallbackRotationOnly),
            _ => None,
        }
    }
}

/// Pose of one device in the SteamVR coordinate system.
#[derive(Clone, Debug, PartialEq)]
pub struct Pose {
    /// Driver-assigned device id.
    pub device_id: u16,
    /// Validity flags.
    pub flags: PoseFlags,
    /// Tracking state.
    pub tracking_result: TrackingResult,
    /// Feeder monotonic time in microseconds.
    pub timestamp_us: u64,
    /// Position in meters.
    pub position: [f64; 3],
    /// Unit quaternion as (w, x, y, z).
    pub rotation: [f64; 4],
    /// Linear velocity in m/s.
    pub linear_velocity: [f64; 3],
    /// Angular velocity in rad/s.
    pub angular_velocity: [f64; 3],
}

impl Pose {
    /// Identity pose at the origin, valid and connected.
    pub fn identity(device_id: u16) -> Self {
        Self {
            device_id,
            flags: PoseFlags::valid_connected(),
            tracking_result: TrackingResult::RunningOk,
            timestamp_us: 0,
            position: [0.0; 3],
            rotation: [1.0, 0.0, 0.0, 0.0],
            linear_velocity: [0.0; 3],
            angular_velocity: [0.0; 3],
        }
    }
}

/// Kind of an input component.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InputKind {
    /// Boolean component; value is 0.0 or 1.0.
    Bool = 0,
    /// Scalar component.
    Scalar = 1,
}

impl InputKind {
    /// Converts a wire value to a known kind.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Bool),
            1 => Some(Self::Scalar),
            _ => None,
        }
    }
}

/// Update of one input component.
#[derive(Clone, Debug, PartialEq)]
pub struct Input {
    /// Driver-assigned device id.
    pub device_id: u16,
    /// Component id, see `docs/protocol.md` 3.4.1.
    pub component_id: u16,
    /// Component kind.
    pub kind: InputKind,
    /// New value.
    pub value: f32,
    /// Passed to `IVRDriverInput` as the time offset in seconds.
    pub time_offset_s: f64,
}

/// Flags selecting which `Display` fields are applied.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DisplayFlags(pub u32);

impl DisplayFlags {
    /// `ipd_m` is valid.
    pub const IPD: u32 = 1 << 0;
    /// `projection_left` and `projection_right` are valid.
    pub const PROJECTION: u32 = 1 << 1;
    /// `swap_eyes` is valid.
    pub const SWAP_EYES: u32 = 1 << 2;
}

/// HMD optical parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct Display {
    /// Which fields are valid.
    pub flags: DisplayFlags,
    /// Interpupillary distance in meters.
    pub ipd_m: f32,
    /// Left eye projection as (left, right, top, bottom) tangents.
    pub projection_left: [f32; 4],
    /// Right eye projection as (left, right, top, bottom) tangents.
    pub projection_right: [f32; 4],
    /// Swap the eye outputs.
    pub swap_eyes: bool,
}

/// Severity of a forwarded log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LogLevel {
    /// Error.
    Error = 0,
    /// Warning.
    Warn = 1,
    /// Informational.
    Info = 2,
    /// Debug.
    Debug = 3,
    /// Trace.
    Trace = 4,
}

impl LogLevel {
    /// Converts a wire value to a known level.
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Error),
            1 => Some(Self::Warn),
            2 => Some(Self::Info),
            3 => Some(Self::Debug),
            4 => Some(Self::Trace),
            _ => None,
        }
    }
}

/// Log line forwarded by the driver.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogRecord {
    /// Severity.
    pub level: LogLevel,
    /// Driver monotonic time in microseconds.
    pub timestamp_us: u64,
    /// UTF-8 text without terminator.
    pub text: String,
}

/// Any message of the protocol.
#[derive(Clone, Debug, PartialEq)]
pub enum Message {
    /// `MsgType::Hello`
    Hello(Hello),
    /// `MsgType::HelloAck`
    HelloAck(HelloAck),
    /// `MsgType::PoseUpdate`
    Pose(Pose),
    /// `MsgType::InputUpdate`
    Input(Input),
    /// `MsgType::DisplayUpdate`
    Display(Display),
    /// `MsgType::LogRecord`
    Log(LogRecord),
    /// `MsgType::Goodbye`
    Goodbye,
}

impl Message {
    /// Header type of this message.
    pub fn msg_type(&self) -> MsgType {
        match self {
            Self::Hello(_) => MsgType::Hello,
            Self::HelloAck(_) => MsgType::HelloAck,
            Self::Pose(_) => MsgType::PoseUpdate,
            Self::Input(_) => MsgType::InputUpdate,
            Self::Display(_) => MsgType::DisplayUpdate,
            Self::Log(_) => MsgType::LogRecord,
            Self::Goodbye => MsgType::Goodbye,
        }
    }
}
