//! 3D view of the world drawn with egui primitives: floor grid, world axes,
//! every device as a box with its local axes, skeleton links, and an orbit
//! camera. No rendering dependency is needed for a line drawing.

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, Vec2};
use vrdummy_core::rig::Rig;
use vrdummy_core::world::{Quat, World, quat_axis_angle, quat_mul, quat_rotate};
use vrdummy_protocol::{Pose, device_ids};

/// Orbit camera around a target point.
pub struct View3d {
    pub tool: Tool,
    pub constraint: Constraint,
    pub ik: bool,
    drag: Option<Drag>,
    /// Yaw of the camera around +Y in radians (0 looks toward -Z from +Z).
    pub yaw: f32,
    /// Pitch in radians, positive looks down.
    pub pitch: f32,
    /// Distance from the target in meters.
    pub distance: f32,
    /// Point the camera looks at.
    pub target: [f32; 3],
}

impl Default for View3d {
    fn default() -> Self {
        Self {
            tool: Tool::Move,
            constraint: Constraint::Screen,
            ik: false,
            drag: None,
            yaw: 0.6,
            pitch: 0.35,
            distance: 3.2,
            target: [0.0, 0.9, 0.0],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Move,
    Rotate,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Constraint {
    Screen,
    X,
    Y,
    Z,
    XY,
    XZ,
    YZ,
}
struct Drag {
    id: u16,
    origin: Pos2,
    position: [f64; 3],
    rotation: Quat,
    constraint: Constraint,
}
pub struct ViewResult {
    pub selected: Option<u16>,
    pub commands: Vec<String>,
}

fn snap_translation(position: [f64; 3], origin: [f64; 3], constraint: Constraint) -> [f64; 3] {
    let delta: [f64; 3] = std::array::from_fn(|i| position[i] - origin[i]);
    if constraint == Constraint::Screen {
        // Quantize distance along the drag, preserving the camera plane.
        let length = delta.iter().map(|v| v * v).sum::<f64>().sqrt();
        let scale = if length > 1e-9 {
            (length / 0.05).round() * 0.05 / length
        } else {
            0.0
        };
        std::array::from_fn(|i| origin[i] + delta[i] * scale)
    } else {
        std::array::from_fn(|i| origin[i] + (delta[i] / 0.05).round() * 0.05)
    }
}

impl View3d {
    pub fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.tool, Tool::Move, "Move");
            ui.selectable_value(&mut self.tool, Tool::Rotate, "Rotate");
            if self.tool == Tool::Rotate
                && !matches!(
                    self.constraint,
                    Constraint::X | Constraint::Y | Constraint::Z
                )
            {
                self.constraint = Constraint::Y;
            }
            egui::ComboBox::from_id_salt("constraint")
                .selected_text(format!("{:?}", self.constraint))
                .show_ui(ui, |ui| {
                    for c in [
                        Constraint::Screen,
                        Constraint::X,
                        Constraint::Y,
                        Constraint::Z,
                        Constraint::XY,
                        Constraint::XZ,
                        Constraint::YZ,
                    ] {
                        if self.tool == Tool::Rotate
                            && !matches!(c, Constraint::X | Constraint::Y | Constraint::Z)
                        {
                            continue;
                        }
                        ui.selectable_value(&mut self.constraint, c, format!("{c:?}"));
                    }
                });
            ui.checkbox(&mut self.ik, "Hand/foot IK");
        });
    }
}

/// Which device the view highlights and reports clicks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Selection(pub Option<u16>);

struct Camera {
    rect: Rect,
    focal: f32,
    // Camera-space transform: rotation rows then translation.
    rot: [[f32; 3]; 3],
    eye: [f32; 3],
}

impl Camera {
    fn plane_point(&self, screen: Pos2, point: [f64; 3], normal: [f64; 3]) -> Option<[f64; 3]> {
        let x = f64::from((screen.x - self.rect.center().x) / self.focal);
        let y = f64::from((self.rect.center().y - screen.y) / self.focal);
        let ray: [f64; 3] = std::array::from_fn(|i| {
            f64::from(self.rot[0][i]) * x + f64::from(self.rot[1][i]) * y
                - f64::from(self.rot[2][i])
        });
        let dot = |a: [f64; 3], b: [f64; 3]| (0..3).map(|i| a[i] * b[i]).sum::<f64>();
        let denominator = dot(ray, normal);
        if denominator.abs() < 1e-7 {
            return None;
        }
        let eye = self.eye.map(f64::from);
        let t = dot(std::array::from_fn(|i| point[i] - eye[i]), normal) / denominator;
        (t > 0.0).then(|| std::array::from_fn(|i| eye[i] + t * ray[i]))
    }

    fn drag_position(&self, drag: &Drag, pointer: Pos2) -> Option<[f64; 3]> {
        let axis = match drag.constraint {
            Constraint::X => Some([1.0, 0.0, 0.0]),
            Constraint::Y => Some([0.0, 1.0, 0.0]),
            Constraint::Z => Some([0.0, 0.0, 1.0]),
            _ => None,
        };
        let back = self.rot[2].map(f64::from);
        let normal = match drag.constraint {
            Constraint::XY => [0.0, 0.0, 1.0],
            Constraint::XZ => [0.0, 1.0, 0.0],
            Constraint::YZ => [1.0, 0.0, 0.0],
            _ => axis.map_or(back, |a| {
                let d = (0..3).map(|i| a[i] * back[i]).sum::<f64>();
                std::array::from_fn(|i| back[i] - a[i] * d)
            }),
        };
        let start = self.plane_point(drag.origin, drag.position, normal)?;
        let end = self.plane_point(pointer, drag.position, normal)?;
        let mut delta: [f64; 3] = std::array::from_fn(|i| end[i] - start[i]);
        if let Some(a) = axis {
            let d = (0..3).map(|i| delta[i] * a[i]).sum::<f64>();
            delta = a.map(|v| v * d);
        }
        Some(std::array::from_fn(|i| drag.position[i] + delta[i]))
    }
    fn new(view: &View3d, rect: Rect) -> Self {
        let (sy, cy) = view.yaw.sin_cos();
        let (sp, cp) = view.pitch.sin_cos();
        // Camera position on the orbit sphere.
        let offset = [
            view.distance * cp * sy,
            view.distance * sp,
            view.distance * cp * cy,
        ];
        let eye = [
            view.target[0] + offset[0],
            view.target[1] + offset[1],
            view.target[2] + offset[2],
        ];
        // Basis: right, up, back (camera looks along -back).
        let back = normalize(offset);
        let right = normalize(cross([0.0, 1.0, 0.0], back));
        let up = cross(back, right);
        Self {
            rect,
            focal: rect.height().min(rect.width()) * 0.9,
            rot: [right, up, back],
            eye,
        }
    }

    /// Projects a world point; `None` when behind the camera.
    fn project(&self, point: [f64; 3]) -> Option<Pos2> {
        let delta = [
            point[0] as f32 - self.eye[0],
            point[1] as f32 - self.eye[1],
            point[2] as f32 - self.eye[2],
        ];
        let cam_x = dot(self.rot[0], delta);
        let cam_y = dot(self.rot[1], delta);
        let cam_z = dot(self.rot[2], delta); // positive = behind the camera
        if cam_z > -0.05 {
            return None;
        }
        let depth = -cam_z;
        Some(Pos2::new(
            self.rect.center().x + self.focal * cam_x / depth,
            self.rect.center().y - self.focal * cam_y / depth,
        ))
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let n = dot(v, v).sqrt();
    if n == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / n, v[1] / n, v[2] / n]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constrained_drag_and_snap_preserve_orthogonal_coordinates() {
        let view = View3d::default();
        let cam = Camera::new(
            &view,
            Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 600.0)),
        );
        let position = [0.173, 1.237, -0.313];
        let origin = cam.project(position).unwrap();
        for (constraint, fixed) in [
            (Constraint::X, vec![1, 2]),
            (Constraint::Y, vec![0, 2]),
            (Constraint::Z, vec![0, 1]),
            (Constraint::XY, vec![2]),
            (Constraint::XZ, vec![1]),
            (Constraint::YZ, vec![0]),
        ] {
            let drag = Drag {
                id: 1,
                origin,
                position,
                rotation: [1.0, 0.0, 0.0, 0.0],
                constraint,
            };
            let moved = cam
                .drag_position(&drag, origin + Vec2::new(35.0, -23.0))
                .unwrap();
            let snapped = snap_translation(moved, position, constraint);
            for i in fixed {
                assert!((moved[i] - position[i]).abs() < 1e-6);
                assert!((snapped[i] - position[i]).abs() < 1e-6);
            }
        }
        let drag = Drag {
            id: 1,
            origin,
            position,
            rotation: [1.0, 0.0, 0.0, 0.0],
            constraint: Constraint::Screen,
        };
        let moved = cam
            .drag_position(&drag, origin + Vec2::new(35.0, -23.0))
            .unwrap();
        let snapped = snap_translation(moved, position, Constraint::Screen);
        let depth: f64 = (0..3)
            .map(|i| (snapped[i] - position[i]) * f64::from(cam.rot[2][i]))
            .sum();
        assert!(depth.abs() < 1e-6);
    }
}

/// Edges of a box whose corners are indexed by bits (x, y, z).
const BOX_EDGES: [(usize, usize); 12] = [
    (0, 1),
    (2, 3),
    (4, 5),
    (6, 7),
    (0, 2),
    (1, 3),
    (4, 6),
    (5, 7),
    (0, 4),
    (1, 5),
    (2, 6),
    (3, 7),
];

struct Painter<'a> {
    cam: &'a Camera,
    painter: &'a egui::Painter,
}

impl Painter<'_> {
    fn line(&self, a: [f64; 3], b: [f64; 3], stroke: Stroke) {
        if let (Some(pa), Some(pb)) = (self.cam.project(a), self.cam.project(b)) {
            self.painter.line_segment([pa, pb], stroke);
        }
    }

    fn axes(&self, pose: &Pose, len: f64) {
        let o = pose.position;
        for (axis, color) in [
            ([len, 0.0, 0.0], Color32::from_rgb(230, 70, 70)),
            ([0.0, len, 0.0], Color32::from_rgb(70, 220, 70)),
            ([0.0, 0.0, len], Color32::from_rgb(90, 130, 255)),
        ] {
            let d = quat_rotate(pose.rotation, axis);
            self.line(
                o,
                [o[0] + d[0], o[1] + d[1], o[2] + d[2]],
                Stroke::new(1.5, color),
            );
        }
    }

    fn boxed(&self, position: [f64; 3], rotation: Quat, half: [f64; 3], stroke: Stroke) {
        let mut corners = [[0.0; 3]; 8];
        for (i, c) in corners.iter_mut().enumerate() {
            let local = [
                if i & 1 != 0 { half[0] } else { -half[0] },
                if i & 2 != 0 { half[1] } else { -half[1] },
                if i & 4 != 0 { half[2] } else { -half[2] },
            ];
            let r = quat_rotate(rotation, local);
            *c = [position[0] + r[0], position[1] + r[1], position[2] + r[2]];
        }
        for (a, b) in BOX_EDGES {
            self.line(corners[a], corners[b], stroke);
        }
    }
}

/// Name shown next to a device.
pub fn device_label(id: u16) -> String {
    match id {
        device_ids::HMD => "hmd".to_owned(),
        device_ids::CONTROLLER_LEFT => "ctrl-l".to_owned(),
        device_ids::CONTROLLER_RIGHT => "ctrl-r".to_owned(),
        n => format!("trk{}", n - device_ids::TRACKER_BASE),
    }
}

fn device_color(id: u16) -> Color32 {
    match id {
        device_ids::HMD => Color32::from_rgb(235, 235, 235),
        device_ids::CONTROLLER_LEFT => Color32::from_rgb(80, 190, 255),
        device_ids::CONTROLLER_RIGHT => Color32::from_rgb(255, 160, 80),
        _ => Color32::from_rgb(220, 130, 255),
    }
}

fn device_half_size(id: u16) -> [f64; 3] {
    match id {
        device_ids::HMD => [0.09, 0.05, 0.06],
        device_ids::CONTROLLER_LEFT | device_ids::CONTROLLER_RIGHT => [0.03, 0.02, 0.08],
        _ => [0.04, 0.015, 0.04],
    }
}

impl View3d {
    /// Draws the world and handles orbit, zoom and click-to-select.
    /// Returns the device id clicked, if any.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        world: &World,
        rig: &Rig,
        selected: Selection,
    ) -> ViewResult {
        let size = ui.available_size();
        let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 4.0, Color32::from_rgb(18, 18, 24));

        if response.dragged_by(egui::PointerButton::Secondary) {
            let d = response.drag_delta();
            self.yaw -= d.x * 0.01;
            self.pitch = (self.pitch + d.y * 0.01).clamp(-1.5, 1.5);
        }
        if response.dragged_by(egui::PointerButton::Middle) {
            let d = response.drag_delta();
            self.target[1] = (self.target[1] + d.y * 0.004 * self.distance).clamp(0.0, 2.5);
        }
        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.distance = (self.distance * (1.0 - scroll * 0.001)).clamp(0.5, 10.0);
            }
        }

        let cam = Camera::new(self, rect);
        let mut result = ViewResult {
            selected: None,
            commands: Vec::new(),
        };
        let pick = |point: Pos2| {
            world
                .device_ids()
                .filter_map(|id| {
                    let screen = cam.project(world.pose(id)?.position)?;
                    let distance = screen.distance(point);
                    (distance < 20.0).then_some((id, distance))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|p| p.0)
        };
        if response.drag_started_by(egui::PointerButton::Primary)
            && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            && let Some(id) = pick(origin)
            && let Some(pose) = world.pose(id)
        {
            self.drag = Some(Drag {
                id,
                origin,
                position: pose.position,
                rotation: pose.rotation,
                constraint: self.constraint,
            });
            result.selected = Some(id);
            result.commands.push("edit checkpoint".to_owned());
        }
        if let Some(drag) = &self.drag {
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                result.commands.push("edit undo".to_owned());
                self.drag = None;
            } else if let Some(pointer) = response.interact_pointer_pos() {
                match self.tool {
                    Tool::Move => {
                        if let Some(mut position) = cam.drag_position(drag, pointer) {
                            if ui.input(|i| i.modifiers.ctrl) {
                                position =
                                    snap_translation(position, drag.position, drag.constraint);
                            }
                            let verb = if self.ik && Rig::supports(drag.id) {
                                "ik"
                            } else {
                                "pos"
                            };
                            result.commands.push(format!(
                                "{verb} {} {} {} {}",
                                drag.id, position[0], position[1], position[2]
                            ));
                        }
                    }
                    Tool::Rotate => {
                        let delta = pointer - drag.origin;
                        let angle = f64::from(delta.x - delta.y) * 0.01;
                        let axis = match drag.constraint {
                            Constraint::X => [1.0, 0.0, 0.0],
                            Constraint::Z => [0.0, 0.0, 1.0],
                            _ => [0.0, 1.0, 0.0],
                        };
                        let q = quat_mul(quat_axis_angle(axis, angle), drag.rotation);
                        result.commands.push(format!(
                            "rot {} {} {} {} {}",
                            drag.id, q[0], q[1], q[2], q[3]
                        ));
                    }
                }
            }
        }
        if response.drag_stopped_by(egui::PointerButton::Primary) || !ui.input(|i| i.focused) {
            self.drag = None;
        }
        let p = Painter {
            cam: &cam,
            painter: &painter,
        };

        // Floor grid: 1 m lines bright, 0.5 m lines dim, over 3 m x 3 m.
        let half: f64 = 1.5;
        let mut v: f64 = -half;
        while v <= half + 1e-6 {
            let major = (v * 2.0).round() % 2.0 == 0.0;
            let color = if major {
                Color32::from_gray(95)
            } else {
                Color32::from_gray(55)
            };
            p.line([v, 0.0, -half], [v, 0.0, half], Stroke::new(1.0, color));
            p.line([-half, 0.0, v], [half, 0.0, v], Stroke::new(1.0, color));
            v += 0.5;
        }
        // World axes at the origin.
        let origin = Pose::identity(0);
        p.axes(&origin, 0.5);

        // Skeleton links: hmd-hip, hip-feet, hmd-hands.
        let pos = |id: u16| {
            world
                .pose(id)
                .filter(|q| q.flags.pose_valid())
                .map(|q| q.position)
        };
        let link = Stroke::new(1.0, Color32::from_gray(120));
        let hip = pos(device_ids::TRACKER_BASE);
        if let (Some(h), Some(hp)) = (pos(device_ids::HMD), hip) {
            p.line(h, hp, link);
        }
        for foot in [device_ids::TRACKER_BASE + 1, device_ids::TRACKER_BASE + 2] {
            if let (Some(hp), Some(f)) = (hip, pos(foot)) {
                p.line(hp, f, link);
            }
        }
        for hand in [device_ids::CONTROLLER_LEFT, device_ids::CONTROLLER_RIGHT] {
            if let (Some(h), Some(hd)) = (pos(device_ids::HMD), pos(hand)) {
                p.line(h, hd, link);
            }
        }
        for chain in rig.chains.values() {
            p.line(chain[0], chain[1], Stroke::new(2.0, Color32::LIGHT_GREEN));
            p.line(chain[1], chain[2], Stroke::new(2.0, Color32::LIGHT_GREEN));
            if let Some(joint) = cam.project(chain[1]) {
                painter.circle_filled(joint, 4.0, Color32::LIGHT_GREEN);
            }
        }

        // Devices.
        let click_pos = response
            .interact_pointer_pos()
            .filter(|_| response.clicked());
        let mut best = (14.0f32, None);
        for id in world.device_ids() {
            let Some(pose) = world.pose(id) else { continue };
            let valid = pose.flags.pose_valid();
            let base = device_color(id);
            let color = if valid {
                base
            } else {
                base.gamma_multiply(0.4)
            };
            let width = if selected.0 == Some(id) { 2.5 } else { 1.0 };
            p.boxed(
                pose.position,
                pose.rotation,
                device_half_size(id),
                Stroke::new(width, color),
            );
            p.axes(pose, 0.12);
            if let Some(screen) = cam.project(pose.position) {
                painter.text(
                    screen + Vec2::new(8.0, -8.0),
                    egui::Align2::LEFT_BOTTOM,
                    device_label(id),
                    egui::FontId::proportional(12.0),
                    color,
                );
                if let Some(cp) = click_pos {
                    let d = cp.distance(screen);
                    if d < best.0 {
                        best = (d, Some(id));
                    }
                }
            }
        }
        if click_pos.is_some() {
            result.selected = best.1;
        }

        painter.text(
            rect.left_top() + Vec2::new(8.0, 6.0),
            egui::Align2::LEFT_TOP,
            "device drag: edit · right-drag: orbit · middle: height · wheel: zoom · Ctrl: snap · Esc: cancel",
            egui::FontId::proportional(11.0),
            Color32::from_gray(140),
        );
        result
    }
}
