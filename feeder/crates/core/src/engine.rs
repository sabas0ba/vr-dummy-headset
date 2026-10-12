//! Executes commands against the world and forwards changes to the driver.

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::time::{Duration, Instant};

use vrdummy_protocol::{Input, InputKind, Message, device_ids};

use crate::command::{Command, CommandError, DeviceRef, controller_component_is_scalar};
use crate::config::Config;
use crate::driver_client::DriverClient;
use crate::osc::OscSender;
use crate::presets;
use crate::world::{World, quat_from_euler_deg};
use crate::{logging, world};

/// Outcome of a command.
#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    /// Success with an optional message.
    Ok(String),
    /// The caller should stop.
    Quit,
}

impl Reply {
    /// Single-line wire form used by the control pipe: `ok [text]` or `quit`.
    pub fn to_line(&self) -> String {
        match self {
            Self::Ok(s) if s.is_empty() => "ok".to_owned(),
            Self::Ok(s) => format!("ok {s}"),
            Self::Quit => "quit".to_owned(),
        }
    }
}

/// Execution error.
#[derive(Debug)]
pub enum EngineError {
    /// Desktop control ownership or rig editing error.
    Interaction(String),
    /// The command line could not be parsed.
    Command(CommandError),
    /// Unknown preset name.
    UnknownPreset(String),
    /// Not connected to the driver, and the command needed it.
    NotConnected,
    /// Pipe or socket failure.
    Io(std::io::Error),
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interaction(message) => f.write_str(message),
            Self::Command(e) => write!(f, "{e}"),
            Self::UnknownPreset(n) => write!(
                f,
                "unknown preset: {n} (available: {})",
                presets::NAMES.join(", ")
            ),
            Self::NotConnected => write!(f, "not connected to driver"),
            Self::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<CommandError> for EngineError {
    fn from(e: CommandError) -> Self {
        Self::Command(e)
    }
}

impl From<std::io::Error> for EngineError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

struct Recorder {
    file: File,
    start: Instant,
}

/// Feeder state machine: world, driver connection, OSC sender, recorder.
pub struct Engine {
    config: Config,
    pub(crate) world: World,
    pub(crate) interaction: crate::interaction::InteractionState,
    window: crate::headset_window::Manager,
    client: Option<DriverClient>,
    last_connect_attempt: Option<Instant>,
    osc: Option<OscSender>,
    recorder: Option<Recorder>,
}

impl Engine {
    /// Creates an engine with the default world. No connection is made.
    pub fn new(config: Config) -> Self {
        Self {
            config,
            world: World::default(),
            interaction: crate::interaction::InteractionState::default(),
            window: crate::headset_window::Manager::default(),
            client: None,
            last_connect_attempt: None,
            osc: None,
            recorder: None,
        }
    }

    /// Configuration in use.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Current world.
    pub fn world(&self) -> &World {
        &self.world
    }

    /// Actual Headset Window placement and saved preferences.
    pub fn headset_window_status(&self) -> Result<crate::headset_window::Status, String> {
        self.window.status()
    }

    /// Whether the driver pipe is open.
    pub fn connected(&self) -> bool {
        self.client.as_ref().is_some_and(DriverClient::alive)
    }

    /// Devices reported by the driver, empty when not connected.
    pub fn driver_devices(&self) -> &[vrdummy_protocol::DeviceInfo] {
        self.client.as_ref().map_or(&[], DriverClient::devices)
    }

    /// Tries to connect once. Errors are logged, not returned, because the
    /// driver may simply not be running yet.
    pub fn try_connect(&mut self) -> bool {
        self.last_connect_attempt = Some(Instant::now());
        let timeout = Duration::from_millis(self.config.driver.handshake_timeout_ms);
        match DriverClient::connect(&self.config.driver.pipe_name, timeout) {
            Ok(client) => {
                let _ = self.stop_control();
                self.interaction.rig.invalidate();
                self.world.reconcile_devices(client.devices());
                self.client = Some(client);
                log::debug!("initial sync");
                if let Err(e) = self.sync_all() {
                    log::warn!("initial sync failed: {e}");
                }
                true
            }
            Err(e) => {
                log::debug!("driver connect failed: {e}");
                false
            }
        }
    }

    /// Closes the driver connection (sending Goodbye). `poll` reconnects
    /// after `reconnect_interval_ms`; state is kept and resynchronised then.
    pub fn disconnect(&mut self) {
        let _ = self.stop_control();
        if self.client.take().is_some() {
            log::info!("disconnected from driver");
        }
        self.last_connect_attempt = Some(Instant::now());
    }

    /// Drives reconnection and drains driver log records. Call regularly.
    pub fn poll(&mut self) {
        self.expire_control();
        self.window.poll();
        if let Some(client) = self.client.as_mut() {
            while let Some(msg) = client.try_recv() {
                if let Message::Log(l) = msg {
                    logging::log_driver_record(
                        logging::level_from_protocol(l.level),
                        l.timestamp_us,
                        &l.text,
                    );
                }
            }
            if !client.alive() {
                log::warn!("driver connection lost");
                self.client = None;
                let _ = self.stop_control();
            }
        }
        if self.client.is_none() {
            let interval = Duration::from_millis(self.config.driver.reconnect_interval_ms);
            let due = self
                .last_connect_attempt
                .is_none_or(|t| t.elapsed() >= interval);
            if due {
                self.try_connect();
            }
        }
    }

    /// Starts writing every executed command to a scenario file.
    pub fn start_recording(&mut self, path: &Path) -> std::io::Result<()> {
        let mut file = File::create(path)?;
        writeln!(file, "# vrdummy recording")?;
        self.recorder = Some(Recorder {
            file,
            start: Instant::now(),
        });
        Ok(())
    }

    /// Stops recording.
    pub fn stop_recording(&mut self) {
        self.recorder = None;
    }

    /// Parses and executes one line.
    pub fn execute_line(&mut self, line: &str) -> Result<Reply, EngineError> {
        let command = Command::parse(line)?;
        let reply = self.execute(&command)?;
        if let Some(rec) = self.recorder.as_mut()
            && !matches!(
                command,
                Command::Status
                    | Command::StatusJson
                    | Command::Quit
                    | Command::Window(crate::headset_window::WindowCommand::Status)
            )
        {
            let t = rec.start.elapsed().as_secs_f64();
            let _ = writeln!(rec.file, "{t:.3} {}", line.trim());
        }
        Ok(reply)
    }

    /// Executes a parsed command.
    pub fn execute(&mut self, command: &Command) -> Result<Reply, EngineError> {
        self.expire_control();
        self.check_owner(command)?;
        log::debug!("execute: {command:?}");
        match command {
            Command::Window(command) => {
                let status = self
                    .window
                    .execute(command)
                    .map_err(EngineError::Interaction)?;
                return Ok(Reply::Ok(
                    serde_json::to_string(&status)
                        .map_err(|e| EngineError::Interaction(e.to_string()))?,
                ));
            }
            Command::Interaction(command) => return self.execute_interaction(command),
            Command::Position(d, p) => {
                self.interaction.rig.invalidate();
                self.world.set_position(d.device_id(), *p);
                self.send_pose(d.device_id())?;
            }
            Command::Move(d, delta) => {
                self.interaction.rig.invalidate();
                let id = d.device_id();
                let mut p = self.world.pose(id).map_or([0.0; 3], |p| p.position);
                for i in 0..3 {
                    p[i] += delta[i];
                }
                self.world.set_position(id, p);
                self.send_pose(id)?;
            }
            Command::Rotation(d, q) => {
                self.interaction.rig.invalidate();
                self.world.set_rotation(d.device_id(), *q);
                self.send_pose(d.device_id())?;
            }
            Command::Euler(d, [yaw, pitch, roll]) => {
                self.interaction.rig.invalidate();
                self.world
                    .set_rotation(d.device_id(), quat_from_euler_deg(*yaw, *pitch, *roll));
                self.send_pose(d.device_id())?;
            }
            Command::Valid(d, valid) => {
                self.world.set_valid(d.device_id(), *valid);
                self.send_pose(d.device_id())?;
            }
            Command::Input(d, component, value) => {
                let id = d.device_id();
                self.world.inputs.insert((id, *component), *value);
                self.send_input(id, *component, *value)?;
            }
            Command::Ipd(ipd) => {
                self.world.display.ipd_m = *ipd;
                self.send_display()?;
            }
            Command::Projection(left, right) => {
                self.world.display.projection_left = *left;
                self.world.display.projection_right = *right;
                self.send_display()?;
            }
            Command::SwapEyes(swap) => {
                self.world.display.swap_eyes = *swap;
                self.send_display()?;
            }
            Command::Preset(name, height) => {
                self.interaction.rig.invalidate();
                if !presets::apply(&mut self.world, name, *height) {
                    return Err(EngineError::UnknownPreset(name.clone()));
                }
                if let Some(client) = &self.client {
                    self.world.reconcile_devices(client.devices());
                }
                self.send_all_poses()?;
            }
            Command::Osc(address, args) => {
                if self.osc.is_none() {
                    self.osc = Some(OscSender::new(&self.config.osc.host, self.config.osc.port)?);
                }
                if let Some(osc) = &self.osc {
                    osc.send(address, args)?;
                }
            }
            Command::Sync => self.sync_all()?,
            Command::Status => return Ok(Reply::Ok(self.status_line())),
            Command::StatusJson => return Ok(Reply::Ok(self.status_json())),
            Command::Quit => {
                self.stop_control()?;
                return Ok(Reply::Quit);
            }
        }
        Ok(Reply::Ok(String::new()))
    }

    /// One-line summary used by `status`.
    pub fn status_line(&self) -> String {
        let hmd = self.world.pose(device_ids::HMD).map_or_else(
            || "none".to_owned(),
            |p| {
                format!(
                    "[{:.3} {:.3} {:.3}]",
                    p.position[0], p.position[1], p.position[2]
                )
            },
        );
        format!(
            "connected={} devices={} hmd={} ipd={:.4} swap={}",
            self.connected(),
            self.world.device_ids().count(),
            hmd,
            self.world.display.ipd_m,
            self.world.display.swap_eyes
        )
    }

    fn send(&mut self, message: &Message) -> Result<(), EngineError> {
        let Some(client) = self.client.as_mut() else {
            // State is kept; it is pushed by sync_all after reconnecting.
            log::debug!("not connected; change kept locally");
            return Ok(());
        };
        if let Err(e) = client.send(message) {
            log::warn!("send to driver failed: {e}");
            self.client = None;
            return Err(EngineError::Io(e));
        }
        Ok(())
    }

    pub(crate) fn send_pose(&mut self, device_id: u16) -> Result<(), EngineError> {
        let Some(pose) = self.world.pose(device_id).cloned() else {
            return Ok(());
        };
        let mut pose = pose;
        pose.timestamp_us = logging::monotonic_micros();
        self.send(&Message::Pose(pose))
    }

    pub(crate) fn send_all_poses(&mut self) -> Result<(), EngineError> {
        let ids: Vec<u16> = self.world.device_ids().collect();
        for id in ids {
            self.send_pose(id)?;
        }
        Ok(())
    }

    pub(crate) fn send_input(
        &mut self,
        device_id: u16,
        component_id: u16,
        value: f32,
    ) -> Result<(), EngineError> {
        let kind = match DeviceRef::Id(device_id) {
            DeviceRef::Id(device_ids::CONTROLLER_LEFT | device_ids::CONTROLLER_RIGHT)
                if controller_component_is_scalar(component_id) =>
            {
                InputKind::Scalar
            }
            _ => InputKind::Bool,
        };
        self.send(&Message::Input(Input {
            device_id,
            component_id,
            kind,
            value,
            time_offset_s: 0.0,
        }))
    }

    fn send_display(&mut self) -> Result<(), EngineError> {
        if self.client.as_ref().is_some_and(|client| {
            !client
                .devices()
                .iter()
                .any(|device| device.device_class == vrdummy_protocol::DeviceClass::Hmd)
        }) {
            return Ok(());
        }
        let msg = Message::Display(self.world.display.to_message());
        self.send(&msg)
    }

    /// Resends every pose, input value and display parameter.
    pub fn sync_all(&mut self) -> Result<(), EngineError> {
        self.send_all_poses()?;
        let inputs: Vec<((u16, u16), f32)> =
            self.world.inputs.iter().map(|(k, v)| (*k, *v)).collect();
        for ((device, component), value) in inputs {
            self.send_input(device, component, value)?;
        }
        self.send_display()
    }
}

/// Re-export used by front ends that display the world.
pub use world::DisplayState;

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        Engine::new(Config::default())
    }

    #[test]
    fn commands_update_world_without_connection() {
        let mut e = engine();
        assert!(matches!(
            e.execute_line("pos hmd 0 1.7 0.5"),
            Ok(Reply::Ok(_))
        ));
        assert_eq!(e.world().pose(0).map(|p| p.position), Some([0.0, 1.7, 0.5]));
        e.execute_line("move hmd 1 0 0").expect("move");
        assert_eq!(e.world().pose(0).map(|p| p.position), Some([1.0, 1.7, 0.5]));
        e.execute_line("input ctrl-l a/click 1").expect("input");
        assert_eq!(e.world().inputs.get(&(1, 1)), Some(&1.0));
        e.execute_line("ipd 0.07").expect("ipd");
        assert!((e.world().display.ipd_m - 0.07).abs() < 1e-6);
    }

    #[test]
    fn quit_and_status() {
        let mut e = engine();
        assert_eq!(e.execute_line("quit").ok(), Some(Reply::Quit));
        match e.execute_line("status") {
            Ok(Reply::Ok(s)) => assert!(s.starts_with("connected=false")),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn parse_errors_surface() {
        let mut e = engine();
        assert!(matches!(
            e.execute_line("fly"),
            Err(EngineError::Command(_))
        ));
        assert!(matches!(
            e.execute_line("preset fly"),
            Err(EngineError::UnknownPreset(_))
        ));
    }

    #[test]
    fn recording_writes_scenario_lines() {
        let dir = std::env::temp_dir().join(format!("vrdummy-rec-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("tempdir");
        let path = dir.join("rec.txt");
        let mut e = engine();
        e.start_recording(&path).expect("record");
        e.execute_line("pos hmd 0 1 0").expect("pos");
        e.execute_line("status").expect("status");
        e.stop_recording();
        let text = std::fs::read_to_string(&path).expect("read");
        let steps = crate::scenario::parse(&text).expect("replayable");
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].line, "pos hmd 0 1 0");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
