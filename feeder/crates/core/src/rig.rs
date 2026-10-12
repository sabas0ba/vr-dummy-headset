//! Configurable two-bone limbs for editing tracked hands and feet.
//! This rig is independent of an application's avatar skeleton.

use crate::world::{World, quat_from_euler_deg, quat_mul, quat_rotate};
use serde::Serialize;
use std::collections::BTreeMap;

/// Root, elbow/knee and end effector in tracking-space meters.
pub type Chain = [[f64; 3]; 3];

type LimbRoot = ([f64; 3], [f64; 4], [f64; 2]);

/// Simple humanoid dimensions and the most recently edited limb chains.
#[derive(Clone, Debug, Serialize)]
pub struct Rig {
    /// Upper arm and forearm lengths in meters.
    pub arm: [f64; 2],
    /// Thigh and shin lengths in meters.
    pub leg: [f64; 2],
    /// Shoulder width, hip width and shoulder height above the hip.
    pub proportions: [f64; 3],
    /// Edited chains indexed by the hand/foot device id.
    pub chains: BTreeMap<u16, Chain>,
    /// Last FK angles for each limb, absent after free translation or IK.
    pub fk_angles: BTreeMap<u16, [f64; 3]>,
}

impl Default for Rig {
    fn default() -> Self {
        Self {
            arm: [0.30, 0.26],
            leg: [0.45, 0.45],
            proportions: [0.38, 0.20, 0.45],
            chains: BTreeMap::new(),
            fk_angles: BTreeMap::new(),
        }
    }
}

pub(crate) fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] + b[i])
}
pub(crate) fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
pub(crate) fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    a.map(|v| v * s)
}
pub(crate) fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3).map(|i| a[i] * b[i]).sum()
}
fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// Solves a two-bone chain, clamping unreachable targets to its reach interval.
/// The pole supplies the preferred elbow/knee bend direction from the root.
fn solve(root: [f64; 3], target: [f64; 3], pole: [f64; 3], lengths: [f64; 2]) -> Chain {
    let delta = sub(target, root);
    let distance = norm(delta);
    let axis = if distance < 1e-9 {
        [0.0, -1.0, 0.0]
    } else {
        scale(delta, distance.recip())
    };
    let [a, b] = lengths;
    let reach = distance.clamp((a - b).abs() + 1e-7, a + b - 1e-7);
    let along = (a * a - b * b + reach * reach) / (2.0 * reach);
    let height = (a * a - along * along).max(0.0).sqrt();
    let mut bend = sub(pole, scale(axis, dot(pole, axis)));
    if norm(bend) < 1e-9 {
        let fallback = if axis[0].abs() < 0.8 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 0.0, 1.0]
        };
        bend = sub(fallback, scale(axis, dot(fallback, axis)));
    }
    bend = scale(bend, norm(bend).recip());
    [
        root,
        add(root, add(scale(axis, along), scale(bend, height))),
        add(root, scale(axis, reach)),
    ]
}

impl Rig {
    /// Discards cached limb geometry after direct pose or dimension changes.
    pub fn invalidate(&mut self) {
        self.chains.clear();
        self.fk_angles.clear();
    }
    /// Whether an id represents a hand or foot in this rig.
    pub fn supports(id: u16) -> bool {
        matches!(id, 1 | 2 | 4 | 5)
    }

    fn root(&self, world: &World, id: u16) -> Result<LimbRoot, String> {
        if !Self::supports(id) || world.pose(id).is_none() {
            return Err("select an existing ctrl-l/ctrl-r/trk1/trk2 limb".to_owned());
        }
        let hip = world
            .pose(3)
            .ok_or("rig editing requires the hip tracker (trk0)")?;
        let sign = if matches!(id, 1 | 4) { -1.0 } else { 1.0 };
        let arm = id <= 2;
        let offset = [
            sign * self.proportions[usize::from(!arm)] * 0.5,
            if arm { self.proportions[2] } else { 0.0 },
            0.0,
        ];
        Ok((
            add(hip.position, quat_rotate(hip.rotation, offset)),
            hip.rotation,
            if arm { self.arm } else { self.leg },
        ))
    }

    /// Moves a hand/foot to the closest reachable target; keeps its orientation.
    pub fn ik(&mut self, world: &mut World, id: u16, target: [f64; 3]) -> Result<(), String> {
        let (root, rotation, lengths) = self.root(world, id)?;
        let sign = if matches!(id, 1 | 4) { -1.0 } else { 1.0 };
        let pole = if id <= 2 {
            [sign, -0.2, 0.3]
        } else {
            [0.0, 0.0, -1.0]
        };
        let chain = solve(root, target, quat_rotate(rotation, pole), lengths);
        world.set_position(id, chain[2]);
        self.chains.insert(id, chain);
        self.fk_angles.remove(&id);
        Ok(())
    }

    /// Sets upper-bone yaw/pitch and elbow/knee bend in degrees.
    pub fn fk(&mut self, world: &mut World, id: u16, angles: [f64; 3]) -> Result<(), String> {
        let (root, basis, lengths) = self.root(world, id)?;
        let upper = quat_mul(basis, quat_from_euler_deg(angles[0], angles[1], 0.0));
        let lower = quat_mul(upper, quat_from_euler_deg(0.0, angles[2], 0.0));
        let joint = add(root, quat_rotate(upper, [0.0, -lengths[0], 0.0]));
        let end = add(joint, quat_rotate(lower, [0.0, -lengths[1], 0.0]));
        world.set_position(id, end);
        world.set_rotation(id, lower);
        self.chains.insert(id, [root, joint, end]);
        self.fk_angles.insert(id, angles);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ik_preserves_bone_lengths_including_unreachable_and_degenerate_targets() {
        for target in [
            [0.0; 3],
            [0.0, -0.1, 0.0],
            [4.0, 2.0, -1.0],
            [0.2, -0.3, 0.1],
        ] {
            let c = solve([0.0; 3], target, [0.0, -1.0, 0.0], [0.3, 0.26]);
            assert!((norm(sub(c[1], c[0])) - 0.3).abs() < 1e-8);
            assert!((norm(sub(c[2], c[1])) - 0.26).abs() < 1e-8);
            assert!(c.into_iter().flatten().all(f64::is_finite));
        }
    }
    #[test]
    fn fk_and_ik_use_the_hips_transformed_coordinate_frame() {
        let mut world = World::default();
        world.set_position(3, [2.0, 1.0, -3.0]);
        world.set_rotation(3, quat_from_euler_deg(90.0, 0.0, 0.0));
        let mut rig = Rig::default();
        rig.fk(&mut world, 1, [0.0, 20.0, 60.0]).unwrap();
        let end = world.pose(1).unwrap().position;
        rig.ik(&mut world, 1, end).unwrap();
        assert!(norm(sub(end, world.pose(1).unwrap().position)) < 1e-8);
        assert!(rig.ik(&mut world, 0, end).is_err());
    }
}
