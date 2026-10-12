//! Explicitly captured keyboard/mouse controls; no system-wide hooks.

use eframe::egui::{self, Key};
use vrdummy_core::{Engine, interaction::Mode};

pub struct Desktop {
    pub active: bool,
    mode: Mode,
    sensitivity: f64,
    error: Option<String>,
    mouse_armed: bool,
}

impl Default for Desktop {
    fn default() -> Self {
        Self {
            active: false,
            mode: Mode::Walk,
            sensitivity: 0.12,
            error: None,
            mouse_armed: false,
        }
    }
}

impl Desktop {
    fn release_cursor(ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(egui::CursorGrab::None));
        ctx.send_viewport_cmd(egui::ViewportCommand::CursorVisible(true));
    }

    pub fn stop(&mut self, engine: &mut Engine, ctx: &egui::Context) {
        // Do not release an agent's newer lease if our lease already expired.
        if engine
            .control_owner()
            .is_some_and(|(owner, _)| owner == "gui")
        {
            let _ = engine.execute_line("control stop");
        }
        self.active = false;
        Self::release_cursor(ctx);
    }

    pub fn tick(&mut self, engine: &mut Engine, ctx: &egui::Context) {
        if !self.active {
            return;
        }
        let release = ctx.input(|i| {
            !i.raw.focused || i.key_pressed(Key::Escape) || i.viewport().close_requested()
        });
        if release
            || ctx.egui_wants_keyboard_input()
            || !engine
                .control_owner()
                .is_some_and(|(owner, _)| owner == "gui")
        {
            self.stop(engine, ctx);
            return;
        }
        let values = ctx.input(|i| {
            if !i.pointer.primary_down() && !i.pointer.secondary_down() {
                self.mouse_armed = true;
            }
            let down = |key| f64::from(u8::from(i.key_down(key)));
            let mouse = i.pointer.motion().unwrap_or_else(|| i.pointer.delta());
            let speed = if i.modifiers.shift { 1.0 } else { 0.5 };
            [
                (down(Key::D) - down(Key::A)) * speed,
                (down(Key::W) - down(Key::S)) * speed,
                down(Key::R) - down(Key::F),
                (-f64::from(mouse.x) * self.sensitivity).clamp(-90.0, 90.0),
                (-f64::from(mouse.y) * self.sensitivity).clamp(-90.0, 90.0),
                down(Key::E) - down(Key::Q),
                down(Key::Space),
                f64::from(u8::from(self.mouse_armed && i.pointer.primary_down())),
                f64::from(u8::from(self.mouse_armed && i.pointer.secondary_down())),
                f64::from(i.stable_dt).clamp(0.0, 0.1),
            ]
        });
        let line = format!(
            "control frame gui {}",
            values.map(|v| v.to_string()).join(" ")
        );
        if let Err(e) = engine.execute_line(&line) {
            self.error = Some(e.to_string());
            self.stop(engine, ctx);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, engine: &mut Engine) {
        ui.horizontal_wrapped(|ui| {
            ui.strong("Desktop controls");
            ui.label(engine.control_owner().map_or_else(
                || "Owner: none".to_owned(),
                |(owner, mode)| format!("Owner: {owner} ({mode:?})"),
            ));
            ui.add_enabled_ui(engine.control_owner().is_none(), |ui| {
                ui.selectable_value(&mut self.mode, Mode::Walk, "Walk (VRChat input)");
                ui.selectable_value(&mut self.mode, Mode::Rig, "Rig (tracking poses)");
                ui.add(egui::Slider::new(&mut self.sensitivity, 0.02..=0.5).text("mouse °/px"));
                if ui.button("Capture keyboard / mouse").clicked() {
                    let mode = if self.mode == Mode::Walk {
                        "walk"
                    } else {
                        "rig"
                    };
                    match engine.execute_line(&format!("control begin gui {mode}")) {
                        Ok(_) => {
                            self.active = true;
                            self.mouse_armed = false;
                            self.error = None;
                            ui.ctx().memory_mut(|m| {
                                if let Some(id) = m.focused() {
                                    m.surrender_focus(id);
                                }
                            });
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::CursorGrab(
                                    egui::CursorGrab::Locked,
                                ));
                            ui.ctx()
                                .send_viewport_cmd(egui::ViewportCommand::CursorVisible(false));
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            });
            if ui.button("Release inputs (Esc)").clicked() {
                let _ = engine.execute_line("control stop");
                self.stop(engine, ui.ctx());
            }
        });
        ui.small("WASD: move · mouse: look · Q/E: turn · Space: jump · LMB: use · RMB: grab · Shift: full speed · Rig R/F: up/down. Esc or focus loss releases inputs.");
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrdummy_core::Config;
    use vrdummy_protocol::controller_components as c;

    fn active() -> (Desktop, Engine, egui::Context) {
        let mut engine = Engine::new(Config::default());
        engine.execute_line("control begin gui walk").unwrap();
        engine
            .execute_line("control frame gui 0 1 0 0 0 0 1 1 1 0.05")
            .unwrap();
        (
            Desktop {
                active: true,
                ..Desktop::default()
            },
            engine,
            egui::Context::default(),
        )
    }

    #[test]
    fn focus_loss_in_logic_only_frames_releases_inputs_and_cursor() {
        let (mut desktop, mut engine, ctx) = active();
        ctx.begin_pass(egui::RawInput {
            focused: true,
            ..Default::default()
        });
        ctx.end_pass().textures_delta.clear();
        // Occluded/minimized windows run logic without updating InputState.focused.
        let output = ctx.run_logic(
            &egui::RawInput {
                focused: false,
                ..Default::default()
            },
            |ctx| desktop.tick(&mut engine, ctx),
        );
        assert!(!desktop.active);
        assert!(engine.control_owner().is_none());
        assert_eq!(engine.world().inputs.get(&(1, c::JOYSTICK_Y)), Some(&0.0));
        assert_eq!(
            engine.world().inputs.get(&(2, c::TRIGGER_VALUE)),
            Some(&0.0)
        );
        assert!(
            output
                .viewport_commands
                .values()
                .flatten()
                .any(|c| matches!(c, egui::ViewportCommand::CursorVisible(true)))
        );
    }

    #[test]
    fn escape_releases_and_does_not_resume_without_capture() {
        let (mut desktop, mut engine, ctx) = active();
        ctx.begin_pass(egui::RawInput {
            focused: true,
            events: vec![egui::Event::Key {
                key: Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        });
        desktop.tick(&mut engine, &ctx);
        ctx.end_pass().textures_delta.clear();
        assert!(!desktop.active);
        assert!(engine.control_owner().is_none());
        ctx.begin_pass(egui::RawInput {
            focused: true,
            ..Default::default()
        });
        desktop.tick(&mut engine, &ctx);
        ctx.end_pass().textures_delta.clear();
        assert!(engine.control_owner().is_none());
    }
}
