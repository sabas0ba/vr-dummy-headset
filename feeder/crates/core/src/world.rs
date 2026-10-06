//! State of every virtual device as last commanded.

use std::collections::BTreeMap;

use vrdummy_protocol::{
    DeviceInfo, Display, DisplayFlags, Pose, PoseFlags, TrackingResult, device_ids,
};

/// Quaternion as (w, x, y, z).
pub type Quat = [f64; 4];

/// Multiplies two quaternions (w, x, y, z).
pub fn quat_mul(a: Quat, b: Quat) -> Quat {
    let [aw, ax, ay, az] = a;
    let [bw, bx, by, bz] = b;
    [
        aw * bw - ax * bx - ay * by - az * bz,
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
    ]
}

/// Quaternion for a rotation of `angle_rad` about a unit axis.
pub fn quat_axis_angle(axis: [f64; 3], angle_rad: f64) -> Quat {
    let half = angle_rad * 0.5;
    let s = half.sin();
    [half.cos(), axis[0] * s, axis[1] * s, axis[2] * s]
}

/// Quaternion from yaw (about +Y), pitch (about +X) and roll (about +Z) in
/// degrees, applied in that order in the SteamVR coordinate system.
pub fn quat_from_euler_deg(yaw: f64, pitch: f64, roll: f64) -> Quat {
    let qy = quat_axis_angle([0.0, 1.0, 0.0], yaw.to_radians());
    let qx = quat_axis_angle([1.0, 0.0, 0.0], pitch.to_radians());
    let qz = quat_axis_angle([0.0, 0.0, 1.0], roll.to_radians());
    quat_mul(quat_mul(qy, qx), qz)
}

/// Yaw, pitch and roll in degrees of a quaternion, inverse of
/// [`quat_from_euler_deg`] (Y-X-Z intrinsic order, see docs/coordinates.md).
pub fn quat_to_euler_deg(q: Quat) -> [f64; 3] {
    let [qw, qx, qy, qz] = quat_normalize(q);
    // Rotation matrix elements needed for the Y-X-Z decomposition.
    let m02 = 2.0 * (qx * qz + qw * qy);
    let m22 = 1.0 - 2.0 * (qx * qx + qy * qy);
    let m12 = 2.0 * (qy * qz - qw * qx);
    let m10 = 2.0 * (qx * qy + qw * qz);
    let m11 = 1.0 - 2.0 * (qx * qx + qz * qz);
    let yaw = m02.atan2(m22).to_degrees();
    let pitch = (-m12).clamp(-1.0, 1.0).asin().to_degrees();
    let roll = m10.atan2(m11).to_degrees();
    [yaw, pitch, roll]
}

/// Rotates a vector by a unit quaternion (w, x, y, z).
pub fn quat_rotate(q: Quat, v: [f64; 3]) -> [f64; 3] {
    let [qw, qx, qy, qz] = q;
    // v' = v + 2 qw (axis x v) + 2 (axis x (axis x v)) with axis = (qx, qy, qz)
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let axis = [qx, qy, qz];
    let uv = cross(axis, v);
    let uuv = cross(axis, uv);
    [
        v[0] + 2.0 * (qw * uv[0] + uuv[0]),
        v[1] + 2.0 * (qw * uv[1] + uuv[1]),
        v[2] + 2.0 * (qw * uv[2] + uuv[2]),
    ]
}

/// Normalises a quaternion; identity if the norm is zero.
pub fn quat_normalize(q: Quat) -> Quat {
    let n = (q[0] * q[0] + q[1] * q[1] + q[2] * q[2] + q[3] * q[3]).sqrt();
    if n == 0.0 {
        [1.0, 0.0, 0.0, 0.0]
    } else {
        [q[0] / n, q[1] / n, q[2] / n, q[3] / n]
    }
}

/// HMD optical parameters as last commanded.
#[derive(Clone, Debug, PartialEq)]
pub struct DisplayState {
    /// Interpupillary distance in meters.
    pub ipd_m: f32,
    /// Left eye projection tangents (left, right, top, bottom).
    pub projection_left: [f32; 4],
    /// Right eye projection tangents.
    pub projection_right: [f32; 4],
    /// Eye outputs swapped.
    pub swap_eyes: bool,
}

impl Default for DisplayState {
    fn default() -> Self {
        Self {
            ipd_m: 0.064,
            projection_left: [-1.0, 1.0, -1.0, 1.0],
            projection_right: [-1.0, 1.0, -1.0, 1.0],
            swap_eyes: false,
        }
    }
}

impl DisplayState {
    /// Message carrying every field.
    pub fn to_message(&self) -> Display {
        Display {
            flags: DisplayFlags(
                DisplayFlags::IPD | DisplayFlags::PROJECTION | DisplayFlags::SWAP_EYES,
            ),
            ipd_m: self.ipd_m,
            projection_left: self.projection_left,
            projection_right: self.projection_right,
            swap_eyes: self.swap_eyes,
        }
    }
}

/// Poses of all devices plus HMD display parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct World {
    poses: BTreeMap<u16, Pose>,
    /// Last commanded input values per (device, component).
    pub inputs: BTreeMap<(u16, u16), f32>,
    /// HMD display parameters.
    pub display: DisplayState,
}

impl Default for World {
    fn default() -> Self {
        Self::new(3)
    }
}

impl World {
    /// Default layout: HMD at standing height, controllers in front of the
    /// shoulders, trackers at hip and feet.
    pub fn new(tracker_count: u16) -> Self {
        let mut poses = BTreeMap::new();
        let mut hmd = Pose::identity(device_ids::HMD);
        hmd.position = [0.0, 1.6, 0.0];
        poses.insert(hmd.device_id, hmd);
        let mut left = Pose::identity(device_ids::CONTROLLER_LEFT);
        left.position = [-0.2, 1.2, -0.3];
        poses.insert(left.device_id, left);
        let mut right = Pose::identity(device_ids::CONTROLLER_RIGHT);
        right.position = [0.2, 1.2, -0.3];
        poses.insert(right.device_id, right);
        for i in 0..tracker_count {
            let id = device_ids::TRACKER_BASE + i;
            let mut t = Pose::identity(id);
            t.position = match i {
                0 => [0.0, 0.95, 0.0],
                1 => [-0.1, 0.05, 0.0],
                2 => [0.1, 0.05, 0.0],
                _ => [0.0, 1.2 - 0.1 * f64::from(i), 0.0],
            };
            poses.insert(id, t);
        }
        Self {
            poses,
            inputs: BTreeMap::new(),
            display: DisplayState::default(),
        }
    }

    /// Pose of a device, if it exists.
    ///
    /// Devices absent from the last handshake have no pose.
    pub fn pose(&self, device_id: u16) -> Option<&Pose> {
        self.poses.get(&device_id)
    }

    /// Matches the driver's device list, preserving state for retained ids.
    pub fn reconcile_devices(&mut self, devices: &[DeviceInfo]) {
        let ids: std::collections::BTreeSet<_> =
            devices.iter().map(|device| device.device_id).collect();
        self.poses.retain(|id, _| ids.contains(id));
        self.inputs.retain(|(id, _), _| ids.contains(id));
        for id in ids {
            self.poses.entry(id).or_insert_with(|| Pose::identity(id));
        }
    }

    /// Mutable pose of a device, created at the origin if it does not exist.
    pub fn pose_mut(&mut self, device_id: u16) -> &mut Pose {
        self.poses
            .entry(device_id)
            .or_insert_with(|| Pose::identity(device_id))
    }

    /// All device ids in ascending order.
    pub fn device_ids(&self) -> impl Iterator<Item = u16> + '_ {
        self.poses.keys().copied()
    }

    /// Sets position and marks the pose valid.
    pub fn set_position(&mut self, device_id: u16, position: [f64; 3]) {
        let p = self.pose_mut(device_id);
        p.position = position;
        p.flags = PoseFlags::valid_connected();
        p.tracking_result = TrackingResult::RunningOk;
    }

    /// Sets rotation (normalised) and marks the pose valid.
    pub fn set_rotation(&mut self, device_id: u16, rotation: Quat) {
        let p = self.pose_mut(device_id);
        p.rotation = quat_normalize(rotation);
        p.flags = PoseFlags::valid_connected();
        p.tracking_result = TrackingResult::RunningOk;
    }

    /// Marks a pose valid or lost.
    pub fn set_valid(&mut self, device_id: u16, valid: bool) {
        let p = self.pose_mut(device_id);
        if valid {
            p.flags = PoseFlags::valid_connected();
            p.tracking_result = TrackingResult::RunningOk;
        } else {
            p.flags = PoseFlags(PoseFlags::CONNECTED);
            p.tracking_result = TrackingResult::RunningOutOfRange;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn approx(a: Quat, b: Quat) -> bool {
        a.iter().zip(b.iter()).all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn identity_multiplication() {
        let q = quat_normalize([0.5, 0.5, 0.5, 0.5]);
        assert!(approx(quat_mul(q, [1.0, 0.0, 0.0, 0.0]), q));
    }

    #[test]
    fn yaw_ninety_degrees() {
        let q = quat_from_euler_deg(90.0, 0.0, 0.0);
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert!(approx(q, [s, 0.0, s, 0.0]));
    }

    #[test]
    fn rotate_forward_by_yaw() {
        // Yaw +90 turns -Z (forward) into -X (left).
        let q = quat_from_euler_deg(90.0, 0.0, 0.0);
        let v = quat_rotate(q, [0.0, 0.0, -1.0]);
        assert!(
            (v[0] + 1.0).abs() < 1e-9 && v[1].abs() < 1e-9 && v[2].abs() < 1e-9,
            "{v:?}"
        );
    }

    #[test]
    fn euler_round_trip() {
        for (yaw, pitch, roll) in [
            (0.0, 0.0, 0.0),
            (90.0, 0.0, 0.0),
            (20.0, -15.0, 0.0),
            (-45.0, 30.0, 10.0),
        ] {
            let e = quat_to_euler_deg(quat_from_euler_deg(yaw, pitch, roll));
            assert!((e[0] - yaw).abs() < 1e-6, "yaw {yaw}: {e:?}");
            assert!((e[1] - pitch).abs() < 1e-6, "pitch {pitch}: {e:?}");
            assert!((e[2] - roll).abs() < 1e-6, "roll {roll}: {e:?}");
        }
    }

    #[test]
    fn default_layout_has_hmd_and_trackers() {
        let w = World::new(2);
        assert_eq!(w.device_ids().collect::<Vec<_>>(), vec![0, 1, 2, 3, 4]);
        assert_eq!(w.pose(0).map(|p| p.position[1]), Some(1.6));
    }

    #[test]
    fn reconciliation_adds_all_reported_trackers_and_preserves_state() {
        use vrdummy_protocol::DeviceClass;

        let mut world = World::default();
        world.set_position(3, [1.0, 2.0, 3.0]);
        world.inputs.insert((3, 0), 1.0);
        world.inputs.insert((1, 1), 1.0);
        let devices: Vec<_> = (3..9)
            .map(|id| DeviceInfo {
                device_id: id,
                device_class: DeviceClass::Tracker,
                serial: format!("TRACKER-{id}"),
            })
            .collect();
        world.reconcile_devices(&devices);
        assert_eq!(
            world.device_ids().collect::<Vec<_>>(),
            (3..9).collect::<Vec<_>>()
        );
        assert_eq!(world.pose(3).expect("retained").position, [1.0, 2.0, 3.0]);
        assert_eq!(world.inputs.get(&(3, 0)), Some(&1.0));
        assert!(!world.inputs.contains_key(&(1, 1)));
        for id in 6..9 {
            assert!(world.pose(id).expect("new tracker").flags.pose_valid());
        }
        world.reconcile_devices(&devices[..1]);
        assert_eq!(world.device_ids().collect::<Vec<_>>(), vec![3]);
        world.reconcile_devices(&[]);
        assert_eq!(world.device_ids().count(), 0);
        assert!(world.inputs.is_empty());
    }

    #[test]
    fn set_valid_false_marks_out_of_range() {
        let mut w = World::new(0);
        w.set_valid(0, false);
        let p = w.pose(0).expect("hmd");
        assert!(!p.flags.pose_valid());
        assert_eq!(p.tracking_result, TrackingResult::RunningOutOfRange);
    }
}
