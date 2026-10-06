//! Graphical front end of the vrdummy feeder (eframe/egui).
//!
//! Layout: device list and editor on the left, a 3D view of the body in the
//! middle, display parameters, recording and the console on the right.
//! Every widget change is turned into a text command and executed through
//! the shared engine, so the GUI, the CLI and the control pipe stay
//! equivalent. Widget values are read back from the engine's world every
//! frame, so changes made elsewhere are shown.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod view3d;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, Color32, RichText};
use view3d::{Selection, View3d, device_label};
use vrdummy_core::command::{controller_component_is_scalar, controller_component_names};
use vrdummy_core::control::{ControlServer, SharedEngine};
use vrdummy_core::engine::Reply;
use vrdummy_core::world::{World, quat_to_euler_deg};
use vrdummy_core::{Config, Engine, logging, presets};
use vrdummy_protocol::{controller_components, device_ids};

const DRAG_SPEED: f64 = 0.005;

fn is_controller(id: u16) -> bool {
    id == device_ids::CONTROLLER_LEFT || id == device_ids::CONTROLLER_RIGHT
}

/// State of the selected device as shown in the editor.
#[derive(Default)]
struct DeviceEditor {
    position: [f64; 3],
    euler: [f64; 3],
    valid: bool,
    buttons: Vec<(&'static str, u16, bool)>,
    joystick: [f32; 2],
    trigger: f32,
    grip: f32,
}

struct App {
    engine: SharedEngine,
    _control: Option<ControlServer>,
    quit: Arc<AtomicBool>,
    selected: Option<u16>,
    device_ids: Vec<u16>,
    editor: DeviceEditor,
    view: View3d,
    ipd_mm: f32,
    projection: [[f32; 4]; 2],
    swap_eyes: bool,
    preset_height: f64,
    command_line: String,
    output: Vec<String>,
    record_path: String,
    recording: bool,
}

impl App {
    fn new(engine: SharedEngine, control: Option<ControlServer>, quit: Arc<AtomicBool>) -> Self {
        let buttons = controller_component_names()
            .filter(|(_, id)| !controller_component_is_scalar(*id))
            .map(|(name, id)| (name, id, false))
            .collect();
        let mut app = Self {
            engine,
            _control: control,
            quit,
            selected: Some(device_ids::HMD),
            device_ids: Vec::new(),
            editor: DeviceEditor {
                buttons,
                ..DeviceEditor::default()
            },
            view: View3d::default(),
            ipd_mm: 64.0,
            projection: [[-1.0, 1.0, -1.0, 1.0]; 2],
            swap_eyes: false,
            preset_height: presets::DEFAULT_HEIGHT_M,
            command_line: String::new(),
            output: Vec::new(),
            record_path: "vrdummy-recording.txt".to_owned(),
            recording: false,
        };
        app.refresh_from_world();
        app
    }

    fn execute(&mut self, line: &str) {
        let result = self.engine.lock().map(|mut e| e.execute_line(line));
        let text = match result {
            Ok(Ok(Reply::Quit)) => {
                self.quit.store(true, Ordering::SeqCst);
                "quit".to_owned()
            }
            Ok(Ok(Reply::Ok(s))) if s.is_empty() => format!("> {line}"),
            Ok(Ok(Reply::Ok(s))) => format!("> {line}\n{s}"),
            Ok(Err(e)) => format!("> {line}\nerror: {e}"),
            Err(_) => "error: engine lock poisoned".to_owned(),
        };
        self.output.push(text);
        if self.output.len() > 200 {
            self.output.drain(..100);
        }
    }

    fn execute_all(&mut self, commands: Vec<String>) {
        for c in commands {
            self.execute(&c);
        }
    }

    /// Copies the engine's world into the editor and display widgets.
    fn refresh_from_world(&mut self) {
        let world = self.engine.lock().expect("engine lock").world().clone();
        self.refresh_widgets(&world);
    }

    fn refresh_widgets(&mut self, w: &World) {
        self.device_ids = w.device_ids().collect();
        if !self
            .selected
            .is_some_and(|id| self.device_ids.contains(&id))
        {
            self.selected = self.device_ids.first().copied();
        }
        if let Some(p) = self.selected.and_then(|id| w.pose(id)) {
            self.editor.position = p.position;
            self.editor.euler = quat_to_euler_deg(p.rotation);
            self.editor.valid = p.flags.pose_valid();
        } else {
            self.editor.position = [0.0; 3];
            self.editor.euler = [0.0; 3];
            self.editor.valid = false;
        }
        if let Some(id) = self.selected.filter(|id| is_controller(*id)) {
            let input = |component: u16| w.inputs.get(&(id, component)).copied().unwrap_or(0.0);
            for (_, component, state) in &mut self.editor.buttons {
                *state = input(*component) != 0.0;
            }
            self.editor.joystick = [
                input(controller_components::JOYSTICK_X),
                input(controller_components::JOYSTICK_Y),
            ];
            self.editor.trigger = input(controller_components::TRIGGER_VALUE);
            self.editor.grip = input(controller_components::GRIP_VALUE);
        }
        self.ipd_mm = w.display.ipd_m * 1000.0;
        self.projection = [w.display.projection_left, w.display.projection_right];
        self.swap_eyes = w.display.swap_eyes;
    }

    // ---- panels -------------------------------------------------------

    fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        let (connected, devices, status) = {
            let e = self.engine.lock().expect("engine lock");
            (e.connected(), e.driver_devices().to_vec(), e.status_line())
        };
        ui.horizontal(|ui| {
            let (color, text) = if connected {
                (Color32::from_rgb(0x2e, 0xa0, 0x43), "driver connected")
            } else {
                (Color32::from_rgb(0xc0, 0x39, 0x2b), "driver disconnected")
            };
            ui.colored_label(color, text);
            if ui.small_button("sync").clicked() {
                self.execute("sync");
            }
            ui.separator();
            ui.small(status);
            if !devices.is_empty() {
                ui.separator();
                ui.menu_button("driver devices", |ui| {
                    for d in &devices {
                        ui.monospace(format!(
                            "{:>2} {:?} {}",
                            d.device_id, d.device_class, d.serial
                        ));
                    }
                });
            }
        });
    }

    fn show_device_list(&mut self, ui: &mut egui::Ui) {
        ui.heading("Devices");
        let valid_ids: Vec<(u16, bool)> = {
            let e = self.engine.lock().expect("engine lock");
            let w = e.world();
            self.device_ids
                .iter()
                .map(|id| (*id, w.pose(*id).is_some_and(|p| p.flags.pose_valid())))
                .collect()
        };
        for (id, valid) in valid_ids {
            let label = device_label(id);
            let text = if valid {
                RichText::new(label)
            } else {
                RichText::new(format!("{label}  (lost)")).color(Color32::from_gray(130))
            };
            if ui
                .selectable_label(self.selected == Some(id), text)
                .clicked()
            {
                self.selected = Some(id);
                self.refresh_from_world();
            }
        }
    }

    fn show_editor(&mut self, ui: &mut egui::Ui) {
        let Some(selected) = self.selected else {
            ui.label("No devices available");
            return;
        };
        let dev = device_label(selected);
        ui.heading(format!("Edit: {dev}"));
        let mut cmds = Vec::new();

        egui::Grid::new("pose_grid")
            .num_columns(4)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("pos [m]");
                let mut changed = false;
                for v in &mut self.editor.position {
                    changed |= ui
                        .add(egui::DragValue::new(v).speed(DRAG_SPEED).fixed_decimals(3))
                        .changed();
                }
                if changed {
                    let p = self.editor.position;
                    cmds.push(format!("pos {dev} {} {} {}", p[0], p[1], p[2]));
                }
                ui.end_row();
                ui.label("euler [°]");
                let mut changed = false;
                for v in &mut self.editor.euler {
                    changed |= ui
                        .add(egui::DragValue::new(v).speed(0.5).fixed_decimals(1))
                        .changed();
                }
                if changed {
                    let e = self.editor.euler;
                    cmds.push(format!("euler {dev} {} {} {}", e[0], e[1], e[2]));
                }
                ui.end_row();
            });
        ui.horizontal(|ui| {
            ui.small("yaw / pitch / roll");
            if ui
                .checkbox(&mut self.editor.valid, "tracking valid")
                .changed()
            {
                cmds.push(format!("valid {dev} {}", u8::from(self.editor.valid)));
            }
            if ui.small_button("reset rotation").clicked() {
                cmds.push(format!("euler {dev} 0 0 0"));
            }
        });

        if is_controller(selected) {
            ui.add_space(6.0);
            ui.label(RichText::new("Buttons").strong());
            egui::Grid::new("buttons")
                .num_columns(3)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    for (i, (name, _, state)) in self.editor.buttons.iter_mut().enumerate() {
                        if ui.toggle_value(state, *name).changed() {
                            cmds.push(format!("input {dev} {name} {}", u8::from(*state)));
                        }
                        if i % 3 == 2 {
                            ui.end_row();
                        }
                    }
                });
            ui.add_space(6.0);
            ui.label(RichText::new("Analog").strong());
            if ui
                .add(egui::Slider::new(&mut self.editor.trigger, 0.0..=1.0).text("trigger"))
                .changed()
            {
                cmds.push(format!("input {dev} trigger/value {}", self.editor.trigger));
            }
            if ui
                .add(egui::Slider::new(&mut self.editor.grip, 0.0..=1.0).text("grip"))
                .changed()
            {
                cmds.push(format!("input {dev} grip/value {}", self.editor.grip));
            }
            let mut js = false;
            js |= ui
                .add(egui::Slider::new(&mut self.editor.joystick[0], -1.0..=1.0).text("joystick x"))
                .changed();
            js |= ui
                .add(egui::Slider::new(&mut self.editor.joystick[1], -1.0..=1.0).text("joystick y"))
                .changed();
            if js {
                cmds.push(format!(
                    "input {dev} joystick/x {}",
                    self.editor.joystick[0]
                ));
                cmds.push(format!(
                    "input {dev} joystick/y {}",
                    self.editor.joystick[1]
                ));
            }
            if ui.small_button("center joystick").clicked() {
                cmds.push(format!("input {dev} joystick/x 0"));
                cmds.push(format!("input {dev} joystick/y 0"));
            }
        }
        self.execute_all(cmds);
    }

    fn show_presets(&mut self, ui: &mut egui::Ui) {
        ui.heading("Presets");
        let mut cmds = Vec::new();
        ui.horizontal(|ui| {
            ui.label("height");
            ui.add(
                egui::DragValue::new(&mut self.preset_height)
                    .speed(0.01)
                    .suffix(" m")
                    .range(0.5..=2.5),
            );
        });
        ui.horizontal(|ui| {
            for name in presets::NAMES {
                if ui.button(*name).clicked() {
                    cmds.push(format!("preset {name} {}", self.preset_height));
                }
            }
        });
        self.execute_all(cmds);
    }

    fn show_display(&mut self, ui: &mut egui::Ui) {
        ui.heading("Display");
        let mut cmds = Vec::new();
        if ui
            .add(egui::Slider::new(&mut self.ipd_mm, 50.0..=80.0).text("IPD mm"))
            .changed()
        {
            cmds.push(format!("ipd {}", self.ipd_mm / 1000.0));
        }
        let mut proj_changed = false;
        egui::Grid::new("proj")
            .num_columns(5)
            .spacing([6.0, 4.0])
            .show(ui, |ui| {
                ui.label("");
                for name in ["L", "R", "T", "B"] {
                    ui.small(name);
                }
                ui.end_row();
                for (eye, p) in ["left", "right"].iter().zip(self.projection.iter_mut()) {
                    ui.label(*eye);
                    for v in p.iter_mut() {
                        proj_changed |= ui
                            .add(egui::DragValue::new(v).speed(0.01).fixed_decimals(2))
                            .changed();
                    }
                    ui.end_row();
                }
            });
        ui.horizontal(|ui| {
            if ui.button("symmetric").clicked() {
                self.projection = [[-1.0, 1.0, -1.0, 1.0]; 2];
                proj_changed = true;
            }
            if ui.button("asymmetric test").clicked() {
                self.projection = [[-1.2, 0.8, -1.0, 1.0], [-0.8, 1.2, -1.0, 1.0]];
                proj_changed = true;
            }
            if ui.checkbox(&mut self.swap_eyes, "swap eyes").changed() {
                cmds.push(format!("swap {}", u8::from(self.swap_eyes)));
            }
        });
        if proj_changed {
            let [l, r] = self.projection;
            cmds.push(format!(
                "proj {} {} {} {} {} {} {} {}",
                l[0], l[1], l[2], l[3], r[0], r[1], r[2], r[3]
            ));
        }
        self.execute_all(cmds);
    }

    fn show_recording(&mut self, ui: &mut egui::Ui) {
        ui.heading("Recording");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.record_path).desired_width(220.0));
            let label = if self.recording { "stop" } else { "record" };
            if ui.button(label).clicked() {
                let mut e = self.engine.lock().expect("engine lock");
                if self.recording {
                    e.stop_recording();
                    self.recording = false;
                } else {
                    match e.start_recording(&PathBuf::from(&self.record_path)) {
                        Ok(()) => self.recording = true,
                        Err(err) => self.output.push(format!("record: {err}")),
                    }
                }
            }
        });
        if self.recording {
            ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), "recording");
        }
    }

    fn show_console(&mut self, ui: &mut egui::Ui) {
        ui.heading("Console");
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.command_line)
                    .hint_text("pos hmd 0 1.7 0")
                    .desired_width(260.0),
            );
            let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("run").clicked() || submitted) && !self.command_line.trim().is_empty() {
                let line = std::mem::take(&mut self.command_line);
                self.execute(&line);
                response.request_focus();
            }
            if ui.button("clear").clicked() {
                self.output.clear();
            }
        });
        egui::ScrollArea::vertical()
            .id_salt("console")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.output {
                    ui.monospace(line);
                }
            });
    }

    fn show_view(&mut self, ui: &mut egui::Ui) {
        let world = self.engine.lock().expect("engine lock").world().clone();
        if let Some(id) = self.view.show(ui, &world, Selection(self.selected)) {
            self.selected = Some(id);
            self.refresh_from_world();
        }
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(mut e) = self.engine.lock() {
            e.poll();
        }
        self.refresh_from_world();
        if self.quit.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            self.show_status_bar(ui);
            ui.separator();
            // The root ui is unbounded, so column sizes come from the viewport.
            let content = ui.ctx().content_rect();
            let left_width = 340.0;
            let right_width = 360.0;
            let gap = 16.0;
            let view_width =
                (content.width() - left_width - right_width - 3.0 * gap - 16.0).max(300.0);
            let column_height = (content.height() - ui.cursor().top() - 16.0).max(200.0);
            ui.horizontal_top(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_width(left_width);
                    ui.set_max_width(left_width);
                    egui::ScrollArea::vertical()
                        .id_salt("left")
                        .max_height(column_height)
                        .show(ui, |ui| {
                            self.show_device_list(ui);
                            ui.separator();
                            self.show_editor(ui);
                            ui.separator();
                            self.show_presets(ui);
                        });
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(view_width);
                    ui.set_max_width(view_width);
                    ui.set_min_height(column_height);
                    ui.set_max_height(column_height);
                    self.show_view(ui);
                });
                ui.separator();
                ui.vertical(|ui| {
                    ui.set_min_width(right_width);
                    ui.set_max_width(right_width);
                    self.show_display(ui);
                    ui.separator();
                    self.show_recording(ui);
                    ui.separator();
                    self.show_console(ui);
                });
            });
        });
    }
}

fn load_config() -> Config {
    let mut args = std::env::args().skip(1);
    let mut path = None;
    while let Some(a) = args.next() {
        if a == "--config" {
            path = args.next().map(PathBuf::from);
        }
    }
    match path {
        Some(p) => Config::load(&p).unwrap_or_else(|e| {
            eprintln!("config: {e}");
            std::process::exit(2);
        }),
        None => Config::default(),
    }
}

fn main() -> eframe::Result {
    let config = load_config();
    let level = logging::parse_level(&config.log.level).unwrap_or(log::LevelFilter::Info);
    let file = (!config.log.file.is_empty()).then(|| PathBuf::from(&config.log.file));
    if let Err(e) = logging::init(file.as_deref(), level, true) {
        eprintln!("log: {e}");
    }

    let mut engine = Engine::new(config);
    engine.try_connect();
    let engine: SharedEngine = Arc::new(Mutex::new(engine));
    let quit = Arc::new(AtomicBool::new(false));
    let pipe_name = engine
        .lock()
        .expect("engine lock")
        .config()
        .control
        .pipe_name
        .clone();
    let control = match ControlServer::start(&pipe_name, Arc::clone(&engine), Arc::clone(&quit)) {
        Ok(s) => Some(s),
        Err(e) => {
            log::error!("control pipe: {e}");
            None
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 860.0]),
        ..Default::default()
    };
    eframe::run_native(
        "vrdummy feeder",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(engine, control, quit)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use vrdummy_protocol::{DeviceClass, DeviceInfo};

    #[test]
    fn editor_follows_device_removal_and_empty_world() {
        let engine = Arc::new(Mutex::new(Engine::new(Config::default())));
        let mut app = App::new(engine, None, Arc::new(AtomicBool::new(false)));
        let mut world = World::default();
        let devices: Vec<_> = [3, 4]
            .into_iter()
            .map(|id| DeviceInfo {
                device_id: id,
                device_class: DeviceClass::Tracker,
                serial: format!("TRACKER-{id}"),
            })
            .collect();
        world.reconcile_devices(&devices);
        world.set_position(3, [1.0, 2.0, 3.0]);
        app.refresh_widgets(&world);
        assert_eq!(app.selected, Some(3));
        assert!(app.editor.valid);
        assert!((app.editor.position[1] - 2.0).abs() < 1e-9);

        app.selected = Some(4);
        app.refresh_widgets(&world);
        assert_eq!(app.selected, Some(4));
        world.reconcile_devices(&devices[..1]);
        app.refresh_widgets(&world);
        assert_eq!(app.selected, Some(3));

        world.reconcile_devices(&[]);
        app.refresh_widgets(&world);
        assert_eq!(app.selected, None);
        assert!(app.device_ids.is_empty());
        assert!(!app.editor.valid);
        world.reconcile_devices(&devices);
        app.refresh_widgets(&world);
        assert_eq!(app.selected, Some(3));
        assert!(app.editor.valid);
    }
}
