//! Text command grammar shared by the CLI, the GUI, the control pipe and
//! scenario files. See `docs/control.md`.

use vrdummy_protocol::{controller_components, device_ids, hmd_components};

/// A device named in a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceRef {
    /// `hmd`
    Hmd,
    /// `ctrl-l`
    ControllerLeft,
    /// `ctrl-r`
    ControllerRight,
    /// `trk<n>` (zero-based)
    Tracker(u16),
    /// Raw device id.
    Id(u16),
}

impl DeviceRef {
    /// Parses a device name.
    pub fn parse(s: &str) -> Result<Self, CommandError> {
        match s {
            "hmd" => Ok(Self::Hmd),
            "ctrl-l" | "left" => Ok(Self::ControllerLeft),
            "ctrl-r" | "right" => Ok(Self::ControllerRight),
            _ => {
                if let Some(n) = s.strip_prefix("trk") {
                    n.parse()
                        .map(Self::Tracker)
                        .map_err(|_| CommandError::BadDevice(s.to_owned()))
                } else {
                    s.parse()
                        .map(Self::Id)
                        .map_err(|_| CommandError::BadDevice(s.to_owned()))
                }
            }
        }
    }

    /// Driver device id.
    pub fn device_id(self) -> u16 {
        match self {
            Self::Hmd => device_ids::HMD,
            Self::ControllerLeft => device_ids::CONTROLLER_LEFT,
            Self::ControllerRight => device_ids::CONTROLLER_RIGHT,
            Self::Tracker(n) => device_ids::TRACKER_BASE + n,
            Self::Id(id) => id,
        }
    }
}

/// Typed OSC argument.
#[derive(Clone, Debug, PartialEq)]
pub enum OscArg {
    /// `i:<n>`
    Int(i32),
    /// `f:<x>`
    Float(f32),
    /// `s:<text>`
    Str(String),
    /// `T` / `F`
    Bool(bool),
}

/// Parsed command.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// `pos <dev> <x> <y> <z>`
    Position(DeviceRef, [f64; 3]),
    /// `move <dev> <dx> <dy> <dz>`
    Move(DeviceRef, [f64; 3]),
    /// `rot <dev> <w> <x> <y> <z>`
    Rotation(DeviceRef, [f64; 4]),
    /// `euler <dev> <yaw> <pitch> <roll>` in degrees
    Euler(DeviceRef, [f64; 3]),
    /// `valid <dev> <0|1>`
    Valid(DeviceRef, bool),
    /// `input <dev> <component> <value>`
    Input(DeviceRef, u16, f32),
    /// `ipd <meters>`
    Ipd(f32),
    /// `proj <lL> <lR> <lT> <lB> <rL> <rR> <rT> <rB>`
    Projection([f32; 4], [f32; 4]),
    /// `swap <0|1>`
    SwapEyes(bool),
    /// `preset <name> [height_m]`
    Preset(String, Option<f64>),
    /// `osc <address> [args...]`
    Osc(String, Vec<OscArg>),
    /// `sync`: resend every pose, input and display parameter
    Sync,
    /// `status`
    Status,
    /// `status --json`: complete machine-readable feeder state.
    StatusJson,
    /// `quit`
    Quit,
}

/// Why a line could not be parsed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandError {
    /// Empty line or comment.
    Empty,
    /// Unknown verb.
    UnknownVerb(String),
    /// Wrong argument count for the verb.
    Arity(&'static str, usize),
    /// Unparsable number.
    BadNumber(String),
    /// Unknown device name.
    BadDevice(String),
    /// Unknown component name for the device.
    BadComponent(String),
    /// Malformed OSC argument.
    BadOscArg(String),
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty command"),
            Self::UnknownVerb(v) => write!(f, "unknown command: {v}"),
            Self::Arity(verb, n) => write!(f, "{verb}: expected {n} arguments"),
            Self::BadNumber(s) => write!(f, "not a number: {s}"),
            Self::BadDevice(s) => write!(f, "unknown device: {s}"),
            Self::BadComponent(s) => write!(f, "unknown component: {s}"),
            Self::BadOscArg(s) => write!(f, "bad OSC argument: {s} (use i:, f:, s:, T or F)"),
        }
    }
}

impl std::error::Error for CommandError {}

const CONTROLLER_COMPONENTS: &[(&str, u16)] = &[
    ("system/click", controller_components::SYSTEM_CLICK),
    ("a/click", controller_components::A_CLICK),
    ("a/touch", controller_components::A_TOUCH),
    ("b/click", controller_components::B_CLICK),
    ("b/touch", controller_components::B_TOUCH),
    ("trigger/value", controller_components::TRIGGER_VALUE),
    ("trigger/click", controller_components::TRIGGER_CLICK),
    ("trigger/touch", controller_components::TRIGGER_TOUCH),
    ("grip/value", controller_components::GRIP_VALUE),
    ("grip/click", controller_components::GRIP_CLICK),
    ("grip/touch", controller_components::GRIP_TOUCH),
    ("joystick/x", controller_components::JOYSTICK_X),
    ("joystick/y", controller_components::JOYSTICK_Y),
    ("joystick/click", controller_components::JOYSTICK_CLICK),
    ("joystick/touch", controller_components::JOYSTICK_TOUCH),
];

const HMD_COMPONENTS: &[(&str, u16)] = &[
    ("system/click", hmd_components::SYSTEM_CLICK),
    ("proximity", hmd_components::PROXIMITY),
];

/// Resolves a component name (or numeric id) for a device.
pub fn component_id(device: DeviceRef, name: &str) -> Result<u16, CommandError> {
    if let Ok(id) = name.parse::<u16>() {
        return Ok(id);
    }
    let table: &[(&str, u16)] = match device {
        DeviceRef::Hmd | DeviceRef::Id(device_ids::HMD) => HMD_COMPONENTS,
        DeviceRef::ControllerLeft
        | DeviceRef::ControllerRight
        | DeviceRef::Id(device_ids::CONTROLLER_LEFT | device_ids::CONTROLLER_RIGHT) => {
            CONTROLLER_COMPONENTS
        }
        _ => &[],
    };
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, id)| *id)
        .ok_or_else(|| CommandError::BadComponent(name.to_owned()))
}

/// Whether a controller component is scalar (as opposed to boolean).
pub fn controller_component_is_scalar(id: u16) -> bool {
    matches!(
        id,
        controller_components::TRIGGER_VALUE
            | controller_components::GRIP_VALUE
            | controller_components::JOYSTICK_X
            | controller_components::JOYSTICK_Y
    )
}

/// Component names of a controller in id order.
pub fn controller_component_names() -> impl Iterator<Item = (&'static str, u16)> {
    CONTROLLER_COMPONENTS.iter().copied()
}

fn num<T: std::str::FromStr>(s: &str) -> Result<T, CommandError> {
    s.parse().map_err(|_| CommandError::BadNumber(s.to_owned()))
}

fn flag(s: &str) -> Result<bool, CommandError> {
    match s {
        "1" | "on" | "true" => Ok(true),
        "0" | "off" | "false" => Ok(false),
        _ => Err(CommandError::BadNumber(s.to_owned())),
    }
}

fn arity(verb: &'static str, args: &[&str], n: usize) -> Result<(), CommandError> {
    if args.len() == n {
        Ok(())
    } else {
        Err(CommandError::Arity(verb, n))
    }
}

fn vec3(args: &[&str]) -> Result<[f64; 3], CommandError> {
    Ok([num(args[0])?, num(args[1])?, num(args[2])?])
}

fn vec4f(args: &[&str]) -> Result<[f32; 4], CommandError> {
    Ok([num(args[0])?, num(args[1])?, num(args[2])?, num(args[3])?])
}

fn osc_arg(s: &str) -> Result<OscArg, CommandError> {
    if s == "T" {
        return Ok(OscArg::Bool(true));
    }
    if s == "F" {
        return Ok(OscArg::Bool(false));
    }
    let (kind, value) = s
        .split_once(':')
        .ok_or_else(|| CommandError::BadOscArg(s.to_owned()))?;
    match kind {
        "i" => value
            .parse()
            .map(OscArg::Int)
            .map_err(|_| CommandError::BadOscArg(s.to_owned())),
        "f" => value
            .parse()
            .map(OscArg::Float)
            .map_err(|_| CommandError::BadOscArg(s.to_owned())),
        "s" => Ok(OscArg::Str(value.to_owned())),
        _ => Err(CommandError::BadOscArg(s.to_owned())),
    }
}

impl Command {
    /// Parses one line. Text after `#` is ignored.
    pub fn parse(line: &str) -> Result<Self, CommandError> {
        let line = line.split('#').next().unwrap_or("").trim();
        let mut parts = line.split_whitespace();
        let verb = parts.next().ok_or(CommandError::Empty)?;
        let args: Vec<&str> = parts.collect();
        match verb {
            "pos" => {
                arity("pos", &args, 4)?;
                Ok(Self::Position(
                    DeviceRef::parse(args[0])?,
                    vec3(&args[1..])?,
                ))
            }
            "move" => {
                arity("move", &args, 4)?;
                Ok(Self::Move(DeviceRef::parse(args[0])?, vec3(&args[1..])?))
            }
            "rot" => {
                arity("rot", &args, 5)?;
                let d = DeviceRef::parse(args[0])?;
                Ok(Self::Rotation(
                    d,
                    [num(args[1])?, num(args[2])?, num(args[3])?, num(args[4])?],
                ))
            }
            "euler" => {
                arity("euler", &args, 4)?;
                Ok(Self::Euler(DeviceRef::parse(args[0])?, vec3(&args[1..])?))
            }
            "valid" => {
                arity("valid", &args, 2)?;
                Ok(Self::Valid(DeviceRef::parse(args[0])?, flag(args[1])?))
            }
            "input" => {
                arity("input", &args, 3)?;
                let d = DeviceRef::parse(args[0])?;
                let c = component_id(d, args[1])?;
                let v = flag(args[2])
                    .map_or_else(|_| num::<f32>(args[2]), |b| Ok(f32::from(u8::from(b))))?;
                Ok(Self::Input(d, c, v))
            }
            "ipd" => {
                arity("ipd", &args, 1)?;
                Ok(Self::Ipd(num(args[0])?))
            }
            "proj" => {
                arity("proj", &args, 8)?;
                Ok(Self::Projection(vec4f(&args[0..4])?, vec4f(&args[4..8])?))
            }
            "swap" => {
                arity("swap", &args, 1)?;
                Ok(Self::SwapEyes(flag(args[0])?))
            }
            "preset" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(CommandError::Arity("preset", 1));
                }
                let height = args.get(1).map(|h| num::<f64>(h)).transpose()?;
                Ok(Self::Preset(args[0].to_owned(), height))
            }
            "osc" => {
                if args.is_empty() {
                    return Err(CommandError::Arity("osc", 1));
                }
                let osc_args = args[1..]
                    .iter()
                    .map(|a| osc_arg(a))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Self::Osc(args[0].to_owned(), osc_args))
            }
            "sync" => Ok(Self::Sync),
            "status" if args == ["--json"] => Ok(Self::StatusJson),
            "status" => {
                arity("status", &args, 0)?;
                Ok(Self::Status)
            }
            "quit" | "exit" => Ok(Self::Quit),
            other => Err(CommandError::UnknownVerb(other.to_owned())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn devices() {
        assert_eq!(DeviceRef::parse("hmd"), Ok(DeviceRef::Hmd));
        assert_eq!(DeviceRef::parse("trk2").map(DeviceRef::device_id), Ok(5));
        assert_eq!(DeviceRef::parse("7"), Ok(DeviceRef::Id(7)));
        assert!(DeviceRef::parse("foot").is_err());
    }

    #[test]
    fn position_and_comments() {
        let c = Command::parse("pos hmd 0 1.7 -0.5  # standing").expect("parse");
        assert_eq!(c, Command::Position(DeviceRef::Hmd, [0.0, 1.7, -0.5]));
        assert_eq!(
            Command::parse("   # only comment"),
            Err(CommandError::Empty)
        );
    }

    #[test]
    fn input_by_name_and_id() {
        assert_eq!(
            Command::parse("input ctrl-r a/click 1"),
            Ok(Command::Input(
                DeviceRef::ControllerRight,
                controller_components::A_CLICK,
                1.0
            ))
        );
        assert_eq!(
            Command::parse("input ctrl-l joystick/x -0.5"),
            Ok(Command::Input(
                DeviceRef::ControllerLeft,
                controller_components::JOYSTICK_X,
                -0.5
            ))
        );
        assert_eq!(
            Command::parse("input hmd 1 on"),
            Ok(Command::Input(DeviceRef::Hmd, 1, 1.0))
        );
        assert!(matches!(
            Command::parse("input hmd a/click 1"),
            Err(CommandError::BadComponent(_))
        ));
    }

    #[test]
    fn projection_and_display() {
        assert_eq!(
            Command::parse("proj -1.2 0.8 -1 1 -0.8 1.2 -1 1"),
            Ok(Command::Projection(
                [-1.2, 0.8, -1.0, 1.0],
                [-0.8, 1.2, -1.0, 1.0]
            ))
        );
        assert_eq!(Command::parse("ipd 0.07"), Ok(Command::Ipd(0.07)));
        assert_eq!(Command::parse("swap on"), Ok(Command::SwapEyes(true)));
        assert_eq!(
            Command::parse("proj 1 2 3"),
            Err(CommandError::Arity("proj", 8))
        );
    }

    #[test]
    fn osc_arguments() {
        assert_eq!(
            Command::parse("osc /input/Jump i:1 f:0.5 s:hi T"),
            Ok(Command::Osc(
                "/input/Jump".to_owned(),
                vec![
                    OscArg::Int(1),
                    OscArg::Float(0.5),
                    OscArg::Str("hi".to_owned()),
                    OscArg::Bool(true)
                ]
            ))
        );
        assert!(matches!(
            Command::parse("osc /x 1"),
            Err(CommandError::BadOscArg(_))
        ));
    }

    #[test]
    fn preset_with_optional_height() {
        assert_eq!(
            Command::parse("preset tpose"),
            Ok(Command::Preset("tpose".to_owned(), None))
        );
        assert_eq!(
            Command::parse("preset sit 1.5"),
            Ok(Command::Preset("sit".to_owned(), Some(1.5)))
        );
    }

    #[test]
    fn unknown_verb() {
        assert_eq!(
            Command::parse("fly hmd"),
            Err(CommandError::UnknownVerb("fly".to_owned()))
        );
    }
}
