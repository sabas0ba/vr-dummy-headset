//! Registration UI. File access and vrpathreg run off the GUI thread.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use eframe::egui;
use vrdummy_core::installation::{self, Action, Options, Status};

type OperationResult = (Action, Result<Status, String>);

pub struct InstallationPanel {
    driver_dir: String,
    steamvr_dir: String,
    status: Option<Status>,
    error: Option<String>,
    pending: Option<Receiver<OperationResult>>,
    operation_error: Option<String>,
    next_refresh: Instant,
    restart_notice: bool,
}

impl Default for InstallationPanel {
    fn default() -> Self {
        Self {
            driver_dir: String::new(),
            steamvr_dir: String::new(),
            status: None,
            error: None,
            pending: None,
            operation_error: None,
            next_refresh: Instant::now(),
            restart_notice: false,
        }
    }
}

impl InstallationPanel {
    fn start(&mut self, action: Action, ctx: &egui::Context) {
        if action != Action::Status {
            self.operation_error = None;
        }
        let options = Options {
            driver_dir: (!self.driver_dir.trim().is_empty())
                .then(|| PathBuf::from(&self.driver_dir)),
            steamvr_dir: (!self.steamvr_dir.trim().is_empty())
                .then(|| PathBuf::from(&self.steamvr_dir)),
        };
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        match std::thread::Builder::new()
            .name("vrdummy-registration".to_owned())
            .spawn(move || {
                let result = installation::execute(action, &options, Duration::from_secs(5));
                let _ = tx.send((action, result));
                ctx.request_repaint();
            }) {
            Ok(_) => self.pending = Some(rx),
            Err(e) => self.error = Some(e.to_string()),
        }
        self.next_refresh = Instant::now() + Duration::from_secs(3);
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.pending {
            let result = match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Disconnected) => Some((
                    Action::Status,
                    Err("Registration worker stopped".to_owned()),
                )),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some((action, result)) = result {
                self.pending = None;
                match result {
                    Ok(status) => {
                        self.restart_notice |= status.restart_required;
                        if self.driver_dir.is_empty() {
                            self.driver_dir = status.driver_dir.display().to_string();
                        }
                        self.status = Some(status);
                        self.error = None;
                    }
                    Err(e) => {
                        if action != Action::Status {
                            self.operation_error = Some(e.clone());
                        }
                        self.error = Some(e);
                        self.status = None;
                    }
                }
            }
        }
        if self.pending.is_none() && Instant::now() >= self.next_refresh {
            self.start(Action::Status, ctx);
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("SteamVR driver registration");
        if let Some(status) = &self.status {
            ui.label(if status.registered {
                "Registered"
            } else {
                "Not registered"
            });
            if !status.package_valid {
                ui.label("Driver package missing or invalid");
            }
            if !status.tool_available {
                ui.label("SteamVR registration tool not found");
            }
        } else {
            ui.label("Registration status unavailable");
        }
        let busy = self.pending.is_some();
        let mut action = None;
        ui.add_enabled_ui(!busy, |ui| {
            ui.label("Driver folder (contains driver.vrdrivermanifest)");
            if ui.text_edit_singleline(&mut self.driver_dir).changed() {
                self.status = None;
                self.next_refresh = Instant::now() + Duration::from_millis(500);
            }
            ui.collapsing("SteamVR location", |ui| {
                ui.label("Empty: use the registered SteamVR runtime");
                if ui.text_edit_singleline(&mut self.steamvr_dir).changed() {
                    self.status = None;
                    self.next_refresh = Instant::now() + Duration::from_millis(500);
                }
            });
            ui.horizontal(|ui| {
                let register = self
                    .status
                    .as_ref()
                    .is_some_and(|s| !s.registered && s.package_valid && s.tool_available);
                let unregister = self
                    .status
                    .as_ref()
                    .is_some_and(|s| s.registered && s.tool_available);
                if ui
                    .add_enabled(register, egui::Button::new("Register"))
                    .clicked()
                {
                    action = Some(Action::Register);
                }
                if ui
                    .add_enabled(unregister, egui::Button::new("Unregister"))
                    .clicked()
                {
                    action = Some(Action::Unregister);
                }
                if ui.button("Refresh").clicked() {
                    action = Some(Action::Status);
                }
            });
        });
        if busy {
            ui.spinner();
        }
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some(error) = &self.operation_error {
            ui.colored_label(egui::Color32::LIGHT_RED, format!("Last operation: {error}"));
        }
        if self.restart_notice {
            ui.label("Registration changed. Start or restart SteamVR to apply it.");
        }
        ui.small("Registration and the live driver connection are separate states.");
        if let Some(action) = action {
            self.start(action, ui.ctx());
        }
    }
}
