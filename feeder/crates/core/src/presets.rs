//! Body pose presets. Positions are derived from a body height so that the
//! same preset works for avatars of different sizes.

use vrdummy_protocol::device_ids;

use crate::world::{World, quat_from_euler_deg};

/// Default body height in meters used when a preset gives none.
pub const DEFAULT_HEIGHT_M: f64 = 1.7;

/// Names of the available presets.
pub const NAMES: &[&str] = &["stand", "tpose", "sit"];

/// Applies a preset to the HMD, both controllers and the first three
/// trackers (hip, left foot, right foot). Returns `false` for unknown names.
pub fn apply(world: &mut World, name: &str, height_m: Option<f64>) -> bool {
    let h = height_m.unwrap_or(DEFAULT_HEIGHT_M);
    let eye = h * 0.93;
    let shoulder = h * 0.82;
    let hip = h * 0.53;
    let hand_span = h * 0.41;
    let hmd = device_ids::HMD;
    let (l, r) = (device_ids::CONTROLLER_LEFT, device_ids::CONTROLLER_RIGHT);
    let (t_hip, t_lf, t_rf) = (
        device_ids::TRACKER_BASE,
        device_ids::TRACKER_BASE + 1,
        device_ids::TRACKER_BASE + 2,
    );
    let identity = [1.0, 0.0, 0.0, 0.0];
    match name {
        "stand" => {
            world.set_position(hmd, [0.0, eye, 0.0]);
            world.set_rotation(hmd, identity);
            world.set_position(l, [-0.2, hip + 0.15, -0.3]);
            world.set_position(r, [0.2, hip + 0.15, -0.3]);
            world.set_rotation(l, identity);
            world.set_rotation(r, identity);
            world.set_position(t_hip, [0.0, hip, 0.0]);
            world.set_position(t_lf, [-0.1, 0.05, 0.0]);
            world.set_position(t_rf, [0.1, 0.05, 0.0]);
        }
        "tpose" => {
            world.set_position(hmd, [0.0, eye, 0.0]);
            world.set_rotation(hmd, identity);
            world.set_position(l, [-hand_span, shoulder, 0.0]);
            world.set_position(r, [hand_span, shoulder, 0.0]);
            world.set_rotation(l, quat_from_euler_deg(0.0, 0.0, 90.0));
            world.set_rotation(r, quat_from_euler_deg(0.0, 0.0, -90.0));
            world.set_position(t_hip, [0.0, hip, 0.0]);
            world.set_position(t_lf, [-0.1, 0.05, 0.0]);
            world.set_position(t_rf, [0.1, 0.05, 0.0]);
        }
        "sit" => {
            let seat = h * 0.26;
            world.set_position(hmd, [0.0, seat + (eye - hip), 0.0]);
            world.set_rotation(hmd, identity);
            world.set_position(l, [-0.2, seat + 0.1, -0.3]);
            world.set_position(r, [0.2, seat + 0.1, -0.3]);
            world.set_rotation(l, identity);
            world.set_rotation(r, identity);
            world.set_position(t_hip, [0.0, seat, 0.0]);
            world.set_position(t_lf, [-0.1, 0.05, -0.4]);
            world.set_position(t_rf, [0.1, 0.05, -0.4]);
        }
        _ => return false,
    }
    for id in [t_hip, t_lf, t_rf] {
        world.set_rotation(id, identity);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tpose_spreads_hands_at_shoulder_height() {
        let mut w = World::new(3);
        assert!(apply(&mut w, "tpose", Some(2.0)));
        let l = w.pose(device_ids::CONTROLLER_LEFT).expect("left");
        let r = w.pose(device_ids::CONTROLLER_RIGHT).expect("right");
        assert!(l.position[0] < -0.8);
        assert!(r.position[0] > 0.8);
        assert!((l.position[1] - 1.64).abs() < 1e-9);
    }

    #[test]
    fn unknown_preset_is_rejected() {
        let mut w = World::new(3);
        assert!(!apply(&mut w, "fly", None));
    }

    #[test]
    fn sit_lowers_the_head() {
        let mut w = World::new(3);
        apply(&mut w, "stand", None);
        let standing = w.pose(device_ids::HMD).expect("hmd").position[1];
        apply(&mut w, "sit", None);
        let sitting = w.pose(device_ids::HMD).expect("hmd").position[1];
        assert!(sitting < standing);
    }
}
