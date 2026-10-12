//! Simple rig dimensions and FK controls, read from the shared engine.
use eframe::egui;
use vrdummy_core::rig::Rig;

pub fn show(ui: &mut egui::Ui, rig: &Rig, selected: Option<u16>) -> Vec<String> {
    let mut commands = Vec::new();
    egui::CollapsingHeader::new("FK / IK rig").show(ui, |ui| {
        ui.small("Editing rig anchored to trk0 (hip), independent of the VRChat avatar. Enable Hand/foot IK to drag within reach.");
        if let Some(id) = selected.filter(|id| Rig::supports(*id)) {
            let mut angles = rig.fk_angles.get(&id).copied().unwrap_or([0.0; 3]);
            for (i, label) in ["upper yaw", "upper pitch", "elbow / knee bend"].iter().enumerate() {
                let response = ui.add(egui::Slider::new(&mut angles[i], -180.0..=180.0).text(*label));
                if response.drag_started() || (response.changed() && !response.dragged()) { commands.push("edit checkpoint".to_owned()); }
                if response.changed() { commands.push(format!("fk {id} {} {} {}", angles[0], angles[1], angles[2])); }
            }
        } else { ui.label("Select a hand or foot for FK."); }
        for (name, mut lengths) in [("arm", rig.arm), ("leg", rig.leg)] {
            ui.horizontal(|ui| {
                ui.label(format!("{name} lengths (m)"));
                let a = ui.add(egui::DragValue::new(&mut lengths[0]).speed(0.005).range(0.05..=2.0));
                let b = ui.add(egui::DragValue::new(&mut lengths[1]).speed(0.005).range(0.05..=2.0));
                if a.drag_started() || b.drag_started() || ((a.changed() || b.changed()) && !a.dragged() && !b.dragged()) { commands.push("edit checkpoint".to_owned()); }
                if a.changed() || b.changed() { commands.push(format!("rig lengths {name} {} {}", lengths[0], lengths[1])); }
            });
        }
        let mut proportions = rig.proportions;
        for (i, label) in ["shoulder width", "hip width", "shoulder height from hip"].iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(*label);
                let r = ui.add(egui::DragValue::new(&mut proportions[i]).speed(0.005).range(0.05..=2.0).suffix(" m"));
                if r.drag_started() || (r.changed() && !r.dragged()) { commands.push("edit checkpoint".to_owned()); }
                if r.changed() { commands.push(format!("rig proportions {} {} {}", proportions[0], proportions[1], proportions[2])); }
            });
        }
    });
    commands
}
