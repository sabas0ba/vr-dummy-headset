//! Graphical front end of the vrdummy feeder (eframe/egui).
//!
//! Every widget change is turned into a text command and executed through the
//! shared engine, so the GUI, the CLI and the control pipe stay equivalent.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use vrdummy_core::command::controller_component_names;
use vrdummy_core::control::{ControlServer, SharedEngine};
use vrdummy_core::engine::Reply;
use vrdummy_core::{Config, Engine, logging, presets};
use vrdummy_protocol::device_ids;

const DRAG_SPEED: f64 = 0.005;

struct Vec3Widget {
    name: &'static str,
    device: &'static str,
    value: [f64; 3],
}

impl Vec3Widget {
    fn new(name: &'static str, device: &'static str, value: [f64; 3]) -> Self {
        Self { name, device, value }
    }

    /// Draws the three drag values; returns a command when any changed.
    fn show(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label(self.name);
            for (axis, v) in ["x", "y", "z"].iter().zip(self.value.iter_mut()) {
                ui.label(*axis);
                changed |= ui.add(egui::DragValue::new(v).speed(DRAG_SPEED).fixed_decimals(3)).changed();
            }
        });
        changed.then(|| format!("pos {} {} {} {}", self.device, self.value[0], self.value[1], self.value[2]))
    }
}

struct EulerWidget {
    device: &'static str,
    value: [f64; 3],
}

impl EulerWidget {
    fn show(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("euler");
            for (axis, v) in ["yaw", "pitch", "roll"].iter().zip(self.value.iter_mut()) {
                ui.label(*axis);
                changed |= ui.add(egui::DragValue::new(v).speed(0.5).suffix("°")).changed();
            }
        });
        changed.then(|| format!("euler {} {} {} {}", self.device, self.value[0], self.value[1], self.value[2]))
    }
}

struct ControllerPanel {
    device: &'static str,
    position: Vec3Widget,
    euler: EulerWidget,
    buttons: Vec<(&'static str, u16, bool)>,
    joystick: [f32; 2],
    trigger: f32,
    grip: f32,
}

impl ControllerPanel {
    fn new(device: &'static str, position: [f64; 3]) -> Self {
        let buttons = controller_component_names()
            .filter(|(_, id)| !vrdummy_core::command::controller_component_is_scalar(*id))
            .map(|(name, id)| (name, id, false))
            .collect();
        Self {
            device,
            position: Vec3Widget::new("pos", device, position),
            euler: EulerWidget { device, value: [0.0; 3] },
            buttons,
            joystick: [0.0; 2],
            trigger: 0.0,
            grip: 0.0,
        }
    }

    fn show(&mut self, ui: &mut egui::Ui, out: &mut Vec<String>) {
        out.extend(self.position.show(ui));
        out.extend(self.euler.show(ui));
        ui.horizontal_wrapped(|ui| {
            for (name, _, state) in &mut self.buttons {
                if ui.toggle_value(state, *name).changed() {
                    out.push(format!("input {} {} {}", self.device, name, u8::from(*state)));
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("joystick");
            let mut changed = false;
            changed |= ui.add(egui::Slider::new(&mut self.joystick[0], -1.0..=1.0).text("x")).changed();
            changed |= ui.add(egui::Slider::new(&mut self.joystick[1], -1.0..=1.0).text("y")).changed();
            if changed {
                out.push(format!("input {} joystick/x {}", self.device, self.joystick[0]));
                out.push(format!("input {} joystick/y {}", self.device, self.joystick[1]));
            }
            if ui.button("center").clicked() {
                self.joystick = [0.0; 2];
                out.push(format!("input {} joystick/x 0", self.device));
                out.push(format!("input {} joystick/y 0", self.device));
            }
        });
        ui.horizontal(|ui| {
            if ui.add(egui::Slider::new(&mut self.trigger, 0.0..=1.0).text("trigger")).changed() {
                out.push(format!("input {} trigger/value {}", self.device, self.trigger));
            }
            if ui.add(egui::Slider::new(&mut self.grip, 0.0..=1.0).text("grip")).changed() {
                out.push(format!("input {} grip/value {}", self.device, self.grip));
            }
        });
    }
}

struct App {
    engine: SharedEngine,
    _control: Option<ControlServer>,
    quit: Arc<AtomicBool>,
    hmd_position: Vec3Widget,
    hmd_euler: EulerWidget,
    left: ControllerPanel,
    right: ControllerPanel,
    trackers: Vec<Vec3Widget>,
    tracker_names: Vec<String>,
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
        let (hmd, left, right, trackers, ipd) = {
            let e = engine.lock().expect("engine lock");
            let w = e.world();
            let pos = |id: u16| w.pose(id).map_or([0.0; 3], |p| p.position);
            let trackers: Vec<[f64; 3]> = w
                .device_ids()
                .filter(|id| *id >= device_ids::TRACKER_BASE)
                .map(pos)
                .collect();
            (
                pos(device_ids::HMD),
                pos(device_ids::CONTROLLER_LEFT),
                pos(device_ids::CONTROLLER_RIGHT),
                trackers,
                w.display.ipd_m,
            )
        };
        let tracker_names: Vec<String> = (0..trackers.len()).map(|i| format!("trk{i}")).collect();
        let tracker_widgets = tracker_names
            .iter()
            .zip(trackers)
            .map(|(name, p)| Vec3Widget::new("pos", Box::leak(name.clone().into_boxed_str()), p))
            .collect();
        Self {
            engine,
            _control: control,
            quit,
            hmd_position: Vec3Widget::new("pos", "hmd", hmd),
            hmd_euler: EulerWidget {
                device: "hmd",
                value: [0.0; 3],
            },
            left: ControllerPanel::new("ctrl-l", left),
            right: ControllerPanel::new("ctrl-r", right),
            trackers: tracker_widgets,
            tracker_names,
            ipd_mm: ipd * 1000.0,
            projection: [[-1.0, 1.0, -1.0, 1.0], [-1.0, 1.0, -1.0, 1.0]],
            swap_eyes: false,
            preset_height: presets::DEFAULT_HEIGHT_M,
            command_line: String::new(),
            output: Vec::new(),
            record_path: "vrdummy-recording.txt".to_owned(),
            recording: false,
        }
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

    fn display_commands(&self) -> String {
        let [l, r] = self.projection;
        format!(
            "proj {} {} {} {} {} {} {} {}",
            l[0], l[1], l[2], l[3], r[0], r[1], r[2], r[3]
        )
    }

    fn show_connection(&mut self, ui: &mut egui::Ui) {
        let (connected, devices, status) = {
            let e = self.engine.lock().expect("engine lock");
            (e.connected(), e.driver_devices().to_vec(), e.status_line())
        };
        ui.horizontal(|ui| {
            let (color, text) = if connected {
                (egui::Color32::from_rgb(0x2e, 0xa0, 0x43), "connected")
            } else {
                (egui::Color32::from_rgb(0xc0, 0x39, 0x2b), "disconnected")
            };
            ui.colored_label(color, text);
            if ui.button("sync").clicked() {
                self.execute("sync");
            }
        });
        ui.small(status);
        if !devices.is_empty() {
            ui.collapsing("driver devices", |ui| {
                for d in &devices {
                    ui.monospace(format!("{:>2} {:?} {}", d.device_id, d.device_class, d.serial));
                }
            });
        }
    }

    fn show_devices(&mut self, ui: &mut egui::Ui) {
        let mut cmds = Vec::new();
        ui.heading("HMD");
        cmds.extend(self.hmd_position.show(ui));
        cmds.extend(self.hmd_euler.show(ui));
        ui.separator();
        ui.heading("Left controller");
        self.left.show(ui, &mut cmds);
        ui.separator();
        ui.heading("Right controller");
        self.right.show(ui, &mut cmds);
        ui.separator();
        ui.heading("Trackers");
        for (name, w) in self.tracker_names.iter().zip(self.trackers.iter_mut()) {
            ui.horizontal(|ui| {
                ui.label(name);
                cmds.extend(w.show(ui));
            });
        }
        ui.separator();
        ui.heading("Presets");
        ui.horizontal(|ui| {
            ui.label("height");
            ui.add(egui::DragValue::new(&mut self.preset_height).speed(0.01).suffix(" m").range(0.5..=2.5));
            for name in presets::NAMES {
                if ui.button(*name).clicked() {
                    cmds.push(format!("preset {name} {}", self.preset_height));
                }
            }
        });
        self.execute_all(cmds);
        if self.output.last().is_some_and(|s| s.starts_with("> preset")) {
            self.refresh_from_world();
        }
    }

    fn refresh_from_world(&mut self) {
        let e = self.engine.lock().expect("engine lock");
        let w = e.world();
        let pos = |id: u16| w.pose(id).map_or([0.0; 3], |p| p.position);
        self.hmd_position.value = pos(device_ids::HMD);
        self.left.position.value = pos(device_ids::CONTROLLER_LEFT);
        self.right.position.value = pos(device_ids::CONTROLLER_RIGHT);
        for (i, t) in self.trackers.iter_mut().enumerate() {
            t.value = pos(device_ids::TRACKER_BASE + i as u16);
        }
    }

    fn show_display(&mut self, ui: &mut egui::Ui) {
        let mut cmds = Vec::new();
        ui.heading("Display");
        if ui.add(egui::Slider::new(&mut self.ipd_mm, 50.0..=80.0).text("IPD mm")).changed() {
            cmds.push(format!("ipd {}", self.ipd_mm / 1000.0));
        }
        let mut proj_changed = false;
        for (eye, p) in ["left", "right"].iter().zip(self.projection.iter_mut()) {
            ui.horizontal(|ui| {
                ui.label(*eye);
                for (name, v) in ["L", "R", "T", "B"].iter().zip(p.iter_mut()) {
                    ui.label(*name);
                    proj_changed |= ui.add(egui::DragValue::new(v).speed(0.01).fixed_decimals(2)).changed();
                }
            });
        }
        ui.horizontal(|ui| {
            if ui.button("symmetric").clicked() {
                self.projection = [[-1.0, 1.0, -1.0, 1.0]; 2];
                proj_changed = true;
            }
            if ui.button("asymmetric test").clicked() {
                self.projection = [[-1.2, 0.8, -1.0, 1.0], [-0.8, 1.2, -1.0, 1.0]];
                proj_changed = true;
            }
        });
        if proj_changed {
            cmds.push(self.display_commands());
        }
        if ui.checkbox(&mut self.swap_eyes, "swap eyes").changed() {
            cmds.push(format!("swap {}", u8::from(self.swap_eyes)));
        }
        self.execute_all(cmds);
    }

    fn show_console(&mut self, ui: &mut egui::Ui) {
        ui.heading("Console");
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.command_line)
                    .hint_text("pos hmd 0 1.7 0")
                    .desired_width(320.0),
            );
            let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (ui.button("run").clicked() || submitted) && !self.command_line.trim().is_empty() {
                let line = std::mem::take(&mut self.command_line);
                self.execute(&line);
                response.request_focus();
            }
        });
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.record_path).desired_width(240.0));
            let label = if self.recording { "stop recording" } else { "record" };
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
            if ui.button("clear").clicked() {
                self.output.clear();
            }
        });
        egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
            for line in &self.output {
                ui.monospace(line);
            }
        });
    }
}

impl eframe::App for App {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(mut e) = self.engine.lock() {
            e.poll();
        }
        if self.quit.load(Ordering::SeqCst) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        ctx.request_repaint_after(Duration::from_millis(100));
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::top("status").show(ui, |ui| self.show_connection(ui));
        egui::Panel::right("display").default_size(360.0).show(ui, |ui| {
            self.show_display(ui);
            ui.separator();
            self.show_console(ui);
        });
        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.show_devices(ui));
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
    let pipe_name = engine.lock().expect("engine lock").config().control.pipe_name.clone();
    let control = match ControlServer::start(&pipe_name, Arc::clone(&engine), Arc::clone(&quit)) {
        Ok(s) => Some(s),
        Err(e) => {
            log::error!("control pipe: {e}");
            None
        }
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1100.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "vrdummy feeder",
        options,
        Box::new(move |_cc| Ok(Box::new(App::new(engine, control, quit)))),
    )
}
