//! Desktop input ownership, fail-safe release and pose editing commands.

use crate::engine::{EngineError, Reply};
use crate::rig::{Rig, add, sub};
use crate::world::{quat_from_euler_deg, quat_mul, quat_rotate, quat_to_euler_deg};
use crate::{Command, Engine};
use serde::Serialize;
use std::time::{Duration, Instant};
use vrdummy_protocol::{Pose, controller_components as c};

/// Desktop control mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Send locomotion inputs to the VR application.
    Walk,
    /// Move every tracked device together in tracking space.
    Rig,
}

/// Commands shared by keyboard/mouse controls and agents.
#[derive(Clone, Debug, PartialEq)]
pub enum InteractionCommand {
    /// Acquire a 500 ms renewable input lease.
    Begin(String, Mode),
    /// Owner, strafe/forward/up, yaw/pitch delta, turn, jump/use/grip, delta seconds.
    Frame(String, [f64; 10]),
    /// Release the input lease and its controlled inputs.
    Stop,
    /// Translate every existing device, in meters.
    Translate([f64; 3]),
    /// Rotate every device around the HMD's vertical axis, in degrees.
    Yaw(f64),
    /// Move a limb target through IK.
    Ik(u16, [f64; 3]),
    /// Upper yaw/pitch and bend angle for FK.
    Fk(u16, [f64; 3]),
    /// Configure arm (true) or leg (false) lengths.
    Lengths(bool, [f64; 2]),
    /// Shoulder width, hip width, shoulder height from hip.
    Proportions([f64; 3]),
    /// Save poses and rig parameters for undo.
    Checkpoint,
    /// Restore the previous pose checkpoint.
    Undo,
}

fn numbers<const N: usize>(args: &[&str]) -> Result<[f64; N], String> {
    if args.len() != N {
        return Err(format!("expected {N} finite numbers"));
    }
    let mut values = [0.0_f64; N];
    for (dest, arg) in values.iter_mut().zip(args) {
        *dest = arg.parse().map_err(|_| format!("invalid number: {arg}"))?;
        if !dest.is_finite() || dest.abs() > 1_000_000.0 {
            return Err("all values must be finite and within +/-1000000".to_owned());
        }
    }
    Ok(values)
}

#[cfg(test)]
#[allow(clippy::float_cmp)] // Exact stored values and explicit release-to-zero are the contract.
mod tests {
    use super::*;
    use crate::{Config, World};

    fn engine() -> Engine {
        Engine::new(Config::default())
    }
    fn active() -> Engine {
        let mut e = engine();
        e.execute_line("input ctrl-l trigger/value 0.75").unwrap();
        e.execute_line("control begin agent walk").unwrap();
        e.execute_line("control frame agent 1 1 0 15 10 1 1 1 1 0.05")
            .unwrap();
        e
    }
    fn assert_released(e: &Engine) {
        assert!(e.control_owner().is_none());
        for channel in CHANNELS {
            assert_eq!(e.world.inputs[&channel], 0.0);
        }
        assert_eq!(e.world.inputs[&(1, c::TRIGGER_VALUE)], 0.75);
    }
    #[test]
    fn lease_blocks_conflicts_and_normalizes_diagonal_motion() {
        let mut e = active();
        let before = e.world.clone();
        for line in [
            "control begin gui walk",
            "control frame gui 0 0 0 0 0 0 0 0 0 0.05",
            "pos hmd 0 2 0",
            "input ctrl-l joystick/x 0",
            "edit undo",
        ] {
            assert!(e.execute_line(line).is_err(), "{line}");
            assert_eq!(e.world, before);
        }
        let x = e.world.inputs[&(1, c::JOYSTICK_X)];
        let y = e.world.inputs[&(1, c::JOYSTICK_Y)];
        assert!((x.hypot(y) - 1.0).abs() < 1e-6);
        let angles = quat_to_euler_deg(e.world.pose(0).unwrap().rotation);
        assert!((angles[0] - 15.0).abs() < 1e-8);
        assert!((angles[1] - 10.0).abs() < 1e-8);
        e.execute_line("status --json").unwrap();
    }
    #[test]
    fn explicit_stop_quit_disconnect_and_expiry_release_only_owned_inputs() {
        for action in ["control stop", "quit", "disconnect", "expire"] {
            let mut e = active();
            match action {
                "disconnect" => e.disconnect(),
                "expire" => {
                    e.interaction.last_frame = Instant::now().checked_sub(LEASE);
                    e.expire_control();
                }
                _ => {
                    e.execute_line(action).unwrap();
                }
            }
            assert_released(&e);
            assert!(
                e.execute_line("control frame agent 0 1 0 0 0 0 0 0 0 0.05")
                    .is_err()
            );
            assert_released(&e);
        }
    }
    #[test]
    fn rig_motion_preserves_relative_device_positions() {
        let mut e = engine();
        let before = e.world.clone();
        e.execute_line("rig translate 2 1 -3").unwrap();
        for id in before.device_ids() {
            assert_eq!(
                e.world.pose(id).unwrap().position,
                add(before.pose(id).unwrap().position, [2.0, 1.0, -3.0])
            );
        }
        let pivot = e.world.pose(0).unwrap().position;
        let left = e.world.pose(1).unwrap().position;
        e.execute_line("rig yaw 90").unwrap();
        let expected = add(
            pivot,
            quat_rotate(quat_from_euler_deg(90.0, 0.0, 0.0), sub(left, pivot)),
        );
        assert!(
            sub(e.world.pose(1).unwrap().position, expected)
                .iter()
                .all(|v| v.abs() < 1e-8)
        );
        let before = e.world.clone();
        e.execute_line("control begin agent rig").unwrap();
        e.execute_line("control frame agent 0 1 0 0 0 0 0 0 0 0.1")
            .unwrap();
        for id in before.device_ids() {
            let delta = sub(
                e.world.pose(id).unwrap().position,
                before.pose(id).unwrap().position,
            );
            assert!(
                (delta[0] + 0.1).abs() < 1e-8 && delta[1].abs() < 1e-8 && delta[2].abs() < 1e-8
            );
        }
    }
    #[test]
    fn undo_restores_poses_and_dimensions_but_not_inputs_or_missing_devices() {
        let mut e = engine();
        let original = e.world.clone();
        e.execute_line("edit checkpoint").unwrap();
        e.execute_line("rig lengths arm 0.4 0.35").unwrap();
        e.execute_line("fk ctrl-l 15 30 90").unwrap();
        assert!(e.rig().chains.contains_key(&1));
        e.execute_line("input ctrl-l trigger/value 0.5").unwrap();
        e.execute_line("edit undo").unwrap();
        assert_eq!(e.world.pose(1), original.pose(1));
        assert_eq!(e.rig().arm, [0.30, 0.26]);
        assert_eq!(e.world.inputs[&(1, c::TRIGGER_VALUE)], 0.5);
        assert!(e.execute_line("edit undo").is_err());
        e.execute_line("edit checkpoint").unwrap();
        e.world = World::new(0);
        e.execute_line("edit undo").unwrap();
        assert!(e.world.pose(3).is_none());
    }
    #[test]
    fn invalid_control_values_are_rejected_without_mutation() {
        let mut e = engine();
        let before = e.world.clone();
        for line in [
            "rig translate NaN 0 0",
            "ik ctrl-l 1e308 0 0",
            "rig lengths leg 0 1",
            "rig proportions 0 1 1",
            "fk hmd 0 0 0",
            "fk ctrl-l 181 0 0",
            "control begin unknown walk",
            "control frame agent 0 0 0 0 0 0 0 0 0 0.2",
        ] {
            assert!(e.execute_line(line).is_err(), "{line}");
            assert_eq!(e.world, before);
        }
    }
}

impl InteractionCommand {
    /// Parse a complete interaction command.
    pub fn parse(parts: &[&str]) -> Result<Self, String> {
        use crate::command::DeviceRef;
        match parts {
            [
                "control",
                "begin",
                owner @ ("gui" | "agent"),
                mode @ ("walk" | "rig"),
            ] => Ok(Self::Begin(
                (*owner).to_owned(),
                if *mode == "walk" {
                    Mode::Walk
                } else {
                    Mode::Rig
                },
            )),
            ["control", "frame", owner @ ("gui" | "agent"), rest @ ..] => {
                let v = numbers::<10>(rest)?;
                if v[..3].iter().any(|v| v.abs() > 1.0)
                    || v[3..5].iter().any(|v| v.abs() > 90.0)
                    || v[5].abs() > 1.0
                    || v[6..9].iter().any(|v| !(0.0..=1.0).contains(v))
                    || !(0.0..=0.1).contains(&v[9])
                {
                    return Err("frame axes must be -1..1, actions 0..1, look delta -90..90 degrees, dt 0..0.1 seconds".to_owned());
                }
                Ok(Self::Frame((*owner).to_owned(), v))
            }
            ["control", "stop"] => Ok(Self::Stop),
            ["rig", "translate", rest @ ..] => Ok(Self::Translate(numbers(rest)?)),
            ["rig", "yaw", angle] => Ok(Self::Yaw(numbers::<1>(&[angle])?[0])),
            ["rig", "lengths", limb @ ("arm" | "leg"), rest @ ..] => {
                let v = numbers::<2>(rest)?;
                if v.iter().any(|v| !(0.05..=2.0).contains(v)) {
                    return Err("bone lengths must be 0.05..2 meters".to_owned());
                }
                Ok(Self::Lengths(*limb == "arm", v))
            }
            ["rig", "proportions", rest @ ..] => {
                let v = numbers::<3>(rest)?;
                if v.iter().any(|v| !(0.05..=2.0).contains(v)) {
                    return Err("rig dimensions must be 0.05..2 meters".to_owned());
                }
                Ok(Self::Proportions(v))
            }
            [verb @ ("ik" | "fk"), device, rest @ ..] => {
                let id = DeviceRef::parse(device)
                    .map_err(|e| e.to_string())?
                    .device_id();
                if !Rig::supports(id) {
                    return Err("IK/FK supports ctrl-l, ctrl-r, trk1, trk2".to_owned());
                }
                let v = numbers::<3>(rest)?;
                if *verb == "fk" && (v.iter().any(|v| v.abs() > 180.0)) {
                    return Err("FK angles must be -180..180 degrees".to_owned());
                }
                Ok(if *verb == "ik" {
                    Self::Ik(id, v)
                } else {
                    Self::Fk(id, v)
                })
            }
            ["edit", "checkpoint"] => Ok(Self::Checkpoint),
            ["edit", "undo"] => Ok(Self::Undo),
            _ => Err("invalid control/rig/ik/fk/edit command; see docs/control.md".to_owned()),
        }
    }
}

pub(crate) const LEASE: Duration = Duration::from_millis(500);

#[derive(Default)]
pub(crate) struct InteractionState {
    pub owner: Option<String>,
    pub mode: Option<Mode>,
    pub last_frame: Option<Instant>,
    pub rig: Rig,
    history: Vec<(Vec<Pose>, Rig)>,
}

// Only these channels are owned/released by desktop control.
const CHANNELS: [(u16, u16); 9] = [
    (1, c::JOYSTICK_X),
    (1, c::JOYSTICK_Y),
    (2, c::JOYSTICK_X),
    (2, c::A_CLICK),
    (2, c::TRIGGER_VALUE),
    (2, c::TRIGGER_CLICK),
    (2, c::GRIP_VALUE),
    (2, c::GRIP_CLICK),
    (2, c::JOYSTICK_Y),
];

impl Engine {
    /// Current input owner and mode, if desktop control is active.
    pub fn control_owner(&self) -> Option<(&str, Mode)> {
        self.interaction.owner.as_deref().zip(self.interaction.mode)
    }

    /// Simple editing rig, independent of the VR application's avatar.
    pub fn rig(&self) -> &Rig {
        &self.interaction.rig
    }

    /// Status extension for both GUI and CUI users.
    pub fn interaction_json(&self) -> serde_json::Value {
        serde_json::json!({"owner":self.interaction.owner,"mode":self.interaction.mode,
            "lease_ms":500,"undo_count":self.interaction.history.len(),"rig":self.interaction.rig})
    }

    pub(crate) fn expire_control(&mut self) {
        if self
            .interaction
            .last_frame
            .is_some_and(|t| t.elapsed() >= LEASE)
        {
            let _ = self.stop_control();
        }
    }

    /// Releases only the channels held by the current desktop input owner.
    pub fn stop_control(&mut self) -> Result<(), EngineError> {
        if self.interaction.owner.take().is_none() {
            return Ok(());
        }
        self.interaction.mode = None;
        self.interaction.last_frame = None;
        // Update all local values before any potentially failing pipe writes.
        for (id, component) in CHANNELS {
            if self.world.pose(id).is_some() {
                self.world.inputs.insert((id, component), 0.0);
            }
        }
        let mut failure = None;
        for (id, component) in CHANNELS {
            if self.world.pose(id).is_some()
                && let Err(e) = self.send_input(id, component, 0.0)
            {
                failure = Some(e);
            }
        }
        failure.map_or(Ok(()), Err)
    }

    fn transform_rig(&mut self, delta: [f64; 3], yaw: f64) {
        let pivot = self.world.pose(0).map_or([0.0; 3], |p| p.position);
        let q = quat_from_euler_deg(yaw, 0.0, 0.0);
        let ids: Vec<_> = self.world.device_ids().collect();
        for id in ids {
            let p = self.world.pose_mut(id);
            p.position = add(add(pivot, quat_rotate(q, sub(p.position, pivot))), delta);
            p.rotation = quat_mul(q, p.rotation);
        }
        for chain in self.interaction.rig.chains.values_mut() {
            for p in chain {
                *p = add(add(pivot, quat_rotate(q, sub(*p, pivot))), delta);
            }
        }
    }

    pub(crate) fn execute_interaction(
        &mut self,
        command: &InteractionCommand,
    ) -> Result<Reply, EngineError> {
        use InteractionCommand as I;
        let invalid = EngineError::Interaction;
        match command {
            I::Begin(owner, mode) => {
                self.expire_control();
                if self.interaction.owner.is_some() {
                    return Err(invalid(
                        "control is owned; send control stop before acquiring it".to_owned(),
                    ));
                }
                if self.world.pose(0).is_none()
                    || (*mode == Mode::Walk
                        && (self.world.pose(1).is_none() || self.world.pose(2).is_none()))
                {
                    return Err(invalid(
                        "control requires an HMD; Walk also requires both controllers".to_owned(),
                    ));
                }
                self.interaction.owner = Some(owner.clone());
                self.interaction.mode = Some(*mode);
                self.interaction.last_frame = Some(Instant::now());
            }
            I::Stop => self.stop_control()?,
            I::Frame(owner, v) => {
                self.expire_control();
                if self.interaction.owner.as_ref() != Some(owner) {
                    return Err(invalid(
                        "control lease absent or owned by another source".to_owned(),
                    ));
                }
                self.interaction.last_frame = Some(Instant::now());
                let [
                    x,
                    forward,
                    up,
                    yaw_delta,
                    pitch_delta,
                    turn,
                    jump,
                    trigger,
                    grip,
                    dt,
                ] = *v;
                let mut angles = quat_to_euler_deg(
                    self.world
                        .pose(0)
                        .ok_or_else(|| invalid("HMD unavailable".to_owned()))?
                        .rotation,
                );
                // Mouse look rotates the head, not the tracked hands or the rig root.
                angles[0] += yaw_delta;
                angles[1] = (angles[1] + pitch_delta).clamp(-85.0, 85.0);
                self.world
                    .set_rotation(0, quat_from_euler_deg(angles[0], angles[1], 0.0));
                let n = x.hypot(forward).max(1.0);
                let (x, forward) = (x / n, forward / n);
                if self.interaction.mode == Some(Mode::Rig) {
                    let delta = quat_rotate(
                        quat_from_euler_deg(angles[0], 0.0, 0.0),
                        [x * dt, up * dt, -forward * dt],
                    );
                    self.transform_rig(delta, -turn * 90.0 * dt);
                    self.send_all_poses()?;
                } else {
                    let values = [
                        x,
                        forward,
                        turn,
                        jump,
                        trigger,
                        f64::from(trigger >= 0.5),
                        grip,
                        f64::from(grip >= 0.5),
                        0.0,
                    ];
                    for ((id, component), value) in CHANNELS.into_iter().zip(values) {
                        self.world.inputs.insert((id, component), value as f32);
                        self.send_input(id, component, value as f32)?;
                    }
                    self.send_pose(0)?;
                }
            }
            I::Translate(delta) => {
                self.transform_rig(*delta, 0.0);
                self.send_all_poses()?;
            }
            I::Yaw(yaw) => {
                self.transform_rig([0.0; 3], *yaw);
                self.send_all_poses()?;
            }
            I::Ik(id, target) => {
                self.interaction
                    .rig
                    .ik(&mut self.world, *id, *target)
                    .map_err(invalid)?;
                self.send_pose(*id)?;
            }
            I::Fk(id, angles) => {
                self.interaction
                    .rig
                    .fk(&mut self.world, *id, *angles)
                    .map_err(invalid)?;
                self.send_pose(*id)?;
            }
            I::Lengths(arm, lengths) => {
                if *arm {
                    self.interaction.rig.arm = *lengths;
                } else {
                    self.interaction.rig.leg = *lengths;
                }
                self.interaction.rig.invalidate();
            }
            I::Proportions(v) => {
                self.interaction.rig.proportions = *v;
                self.interaction.rig.invalidate();
            }
            I::Checkpoint => {
                let poses = self
                    .world
                    .device_ids()
                    .filter_map(|id| self.world.pose(id).cloned())
                    .collect();
                self.interaction
                    .history
                    .push((poses, self.interaction.rig.clone()));
                if self.interaction.history.len() > 32 {
                    self.interaction.history.remove(0);
                }
            }
            I::Undo => {
                let (poses, rig) = self
                    .interaction
                    .history
                    .pop()
                    .ok_or_else(|| invalid("no pose checkpoint".to_owned()))?;
                for p in poses {
                    let id = p.device_id;
                    if self.world.pose(id).is_some() {
                        *self.world.pose_mut(id) = p;
                    }
                }
                self.interaction.rig = rig;
                self.send_all_poses()?;
            }
        }
        Ok(Reply::Ok(String::new()))
    }

    pub(crate) fn check_owner(&self, command: &Command) -> Result<(), EngineError> {
        if self.interaction.owner.is_some()
            && !matches!(
                command,
                Command::Interaction(
                    InteractionCommand::Frame(..)
                        | InteractionCommand::Stop
                        | InteractionCommand::Begin(..)
                ) | Command::Status
                    | Command::StatusJson
                    | Command::Quit
                    | Command::Sync
                    | Command::Window(_)
            )
        {
            return Err(EngineError::Interaction(
                "desktop input is active; send control stop before editing".to_owned(),
            ));
        }
        Ok(())
    }
}
