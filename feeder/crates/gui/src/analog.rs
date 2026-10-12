//! Two-dimensional joystick input and visualization.
use eframe::egui::{self, Color32, Sense, Stroke, Vec2};

pub fn stick(ui: &mut egui::Ui, value: &mut [f32; 2], spring: bool, dragging: &mut bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(120.0), Sense::click_and_drag());
    let center = rect.center();
    let radius = 52.0;
    let mut changed = false;
    if (response.dragged() || response.clicked())
        && let Some(pos) = response.interact_pointer_pos()
    {
        let v = (pos - center) / radius;
        let scale = v.length().max(1.0);
        *value = [v.x / scale, -v.y / scale];
        changed = true;
        *dragging = true;
    }
    if spring
        && (response.drag_stopped()
            || (response.clicked() && !ui.input(|i| i.pointer.primary_down())))
    {
        *value = [0.0; 2];
        changed = true;
    }
    if response.drag_stopped() || !ui.input(|i| i.pointer.primary_down()) {
        *dragging = false;
    }
    let painter = ui.painter_at(rect);
    painter.circle_filled(center, radius, Color32::from_gray(28));
    painter.circle_stroke(center, radius, Stroke::new(1.5, Color32::GRAY));
    painter.line_segment(
        [
            center - Vec2::new(radius, 0.0),
            center + Vec2::new(radius, 0.0),
        ],
        Stroke::new(1.0, Color32::DARK_GRAY),
    );
    painter.line_segment(
        [
            center - Vec2::new(0.0, radius),
            center + Vec2::new(0.0, radius),
        ],
        Stroke::new(1.0, Color32::DARK_GRAY),
    );
    let point = center + Vec2::new(value[0], -value[1]) * radius;
    painter.line_segment([center, point], Stroke::new(2.0, Color32::LIGHT_BLUE));
    painter.circle_filled(point, 6.0, Color32::LIGHT_BLUE);
    response.on_hover_text("Drag to set direction and magnitude. Up is +Y.");
    changed
}
