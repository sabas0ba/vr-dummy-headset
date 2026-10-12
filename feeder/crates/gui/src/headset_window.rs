//! Placement controls for the compositor's Headset Window.
use eframe::egui;
use std::time::{Duration, Instant};
use vrdummy_core::{Engine, headset_window::Status};

#[derive(Default)]
pub struct Panel {
    position: [i32; 2],
    status: Option<Status>,
    refreshed: Option<Instant>,
    error: Option<String>,
}

impl Panel {
    fn refresh(&mut self, engine: &Engine) {
        self.refreshed = Some(Instant::now());
        match engine.headset_window_status() {
            Ok(status) => {
                if self.status.is_none() {
                    self.position = status
                        .saved_position
                        .or_else(|| status.windows.first().map(|w| [w.bounds[0], w.bounds[1]]))
                        .unwrap_or([0, 0]);
                }
                self.status = Some(status);
            }
            Err(e) => self.error = Some(e),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui, engine: &mut Engine) {
        if self
            .refreshed
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(1))
        {
            self.refresh(engine);
        }
        let mut command = None;
        ui.horizontal_wrapped(|ui| {
            ui.label("Physical desktop pixels");
            ui.add(egui::DragValue::new(&mut self.position[0]).prefix("X "));
            ui.add(egui::DragValue::new(&mut self.position[1]).prefix("Y "));
            let [x, y] = self.position;
            if ui.button("Move now").clicked() {
                command = Some(format!("window move {x} {y}"));
            }
            if ui.button("Save placement").clicked() {
                command = Some(format!("window save {x} {y}"));
            }
            if ui.button("Forget placement").clicked() {
                command = Some("window forget".to_owned());
            }
            if ui.button("Refresh position").clicked() {
                self.refresh(engine);
            }
        });
        if let Some(status) = &self.status {
            for window in &status.windows {
                ui.label(format!(
                    "Current: X {} Y {} ({} x {})",
                    window.bounds[0], window.bounds[1], window.bounds[2], window.bounds[3]
                ));
            }
            if status.windows.is_empty() {
                ui.label("Headset Window is not open. Start SteamVR with the dummy HMD.");
            }
            ui.label(format!(
                "Saved: {:?}; applied when a window appears while the feeder is running.",
                status.saved_position
            ));
            ui.horizontal_wrapped(|ui| {
                for (index, monitor) in status.monitors.iter().enumerate() {
                    if ui
                        .button(format!("Monitor {index}: {}", monitor.name))
                        .clicked()
                    {
                        command = Some(format!("window monitor {index}"));
                    }
                }
            });
            if let Some(error) = &status.last_error {
                ui.colored_label(egui::Color32::LIGHT_RED, error);
            }
        }
        if let Some(command) = command {
            self.error = engine.execute_line(&command).err().map(|e| e.to_string());
            self.refresh(engine);
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
    }
}
