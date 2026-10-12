//! Command line front end of the vrdummy feeder.
//!
//! ```text
//! vrdummy-cli [--config <file>] [--log <file>] [--level <level>] <subcommand>
//!   run      [--record <file>] [--no-control] [--no-stdin]
//!   scenario <file> [--record <file>]
//!   set      <command...>
//!   status
//! ```

use std::io::BufRead;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use vrdummy_core::control::{self, ControlServer, SharedEngine};
use vrdummy_core::engine::Reply;
use vrdummy_core::installation::{self, Action, Options};
use vrdummy_core::{Config, Engine, logging, scenario};

const USAGE: &str = "\
usage: vrdummy-cli [--config <file>] [--log <file>] [--level <level>] <subcommand>

subcommands:
  run      [--record <file>] [--no-control] [--no-stdin]
           connect to the driver, serve the control pipe and read commands from stdin
  scenario <file> [--record <file>]
           execute a timed scenario file, then exit
  set      [--json] [--timeout-ms <ms>] <command...>
           send one command to a running instance and print the reply
  status   [--json] [--timeout-ms <ms>]
           query the running GUI or CLI; --json returns every device
  driver   <status|register|unregister> [--path <driver-dir>]
           [--steamvr-dir <dir>] [--json] [--timeout-ms <ms>]
           manage SteamVR registration without requiring a running feeder

control options may also precede the subcommand; timeout defaults to 2000 ms
JSON output: one object on stdout, diagnostics on stderr
exit codes: 0 success, 1 command error, 2 arguments/configuration,
            3 transport error, 4 deadline exceeded, 5 incompatible reply

commands are documented in docs/control.md";

struct GlobalOptions {
    config: Option<PathBuf>,
    log_file: Option<PathBuf>,
    level: Option<String>,
    json: bool,
    timeout: Duration,
}

enum Subcommand {
    Driver {
        action: Action,
        options: Options,
    },
    Run {
        record: Option<PathBuf>,
        control: bool,
        stdin: bool,
    },
    Scenario {
        file: PathBuf,
        record: Option<PathBuf>,
    },
    Set {
        line: String,
    },
}

fn take_value(
    args: &mut std::iter::Peekable<std::vec::IntoIter<String>>,
    flag: &str,
) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_args(argv: Vec<String>) -> Result<(GlobalOptions, Subcommand), String> {
    let mut args = argv.into_iter().peekable();
    let mut global = GlobalOptions {
        config: None,
        log_file: None,
        level: None,
        json: false,
        timeout: Duration::from_secs(2),
    };
    let sub = loop {
        let Some(arg) = args.next() else {
            return Err("missing subcommand".to_owned());
        };
        match arg.as_str() {
            "--config" => global.config = Some(PathBuf::from(take_value(&mut args, "--config")?)),
            "--log" => global.log_file = Some(PathBuf::from(take_value(&mut args, "--log")?)),
            "--level" => global.level = Some(take_value(&mut args, "--level")?),
            "--json" => global.json = true,
            "--timeout-ms" => {
                global.timeout = parse_timeout(&take_value(&mut args, "--timeout-ms")?)?;
            }
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            _ => break arg,
        }
    };
    let sub = match sub.as_str() {
        "driver" => {
            let action = match args.next().as_deref() {
                Some("status") => Action::Status,
                Some("register") => Action::Register,
                Some("unregister") => Action::Unregister,
                _ => return Err("driver: expected status, register or unregister".to_owned()),
            };
            let mut options = Options::default();
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--path" => {
                        options.driver_dir = Some(PathBuf::from(take_value(&mut args, "--path")?));
                    }
                    "--steamvr-dir" => {
                        options.steamvr_dir =
                            Some(PathBuf::from(take_value(&mut args, "--steamvr-dir")?));
                    }
                    "--json" => global.json = true,
                    "--timeout-ms" => {
                        global.timeout = parse_timeout(&take_value(&mut args, "--timeout-ms")?)?;
                    }
                    _ => return Err(format!("driver: unknown option {arg}")),
                }
            }
            Subcommand::Driver { action, options }
        }
        "run" => {
            let mut record = None;
            let mut control = true;
            let mut stdin = true;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--record" => record = Some(PathBuf::from(take_value(&mut args, "--record")?)),
                    "--no-control" => control = false,
                    "--no-stdin" => stdin = false,
                    other => return Err(format!("run: unknown option {other}")),
                }
            }
            Subcommand::Run {
                record,
                control,
                stdin,
            }
        }
        "scenario" | "replay" => {
            let file = PathBuf::from(args.next().ok_or("scenario: missing file")?);
            let mut record = None;
            while let Some(a) = args.next() {
                match a.as_str() {
                    "--record" => record = Some(PathBuf::from(take_value(&mut args, "--record")?)),
                    other => return Err(format!("scenario: unknown option {other}")),
                }
            }
            Subcommand::Scenario { file, record }
        }
        "set" => {
            parse_control_options(&mut args, &mut global)?;
            let rest: Vec<String> = args.collect();
            if rest.is_empty() {
                return Err("set: missing command".to_owned());
            }
            Subcommand::Set {
                line: rest.join(" "),
            }
        }
        "status" => {
            parse_control_options(&mut args, &mut global)?;
            if let Some(arg) = args.next() {
                return Err(format!("status: unknown option {arg}"));
            }
            Subcommand::Set {
                line: "status".to_owned(),
            }
        }
        other => return Err(format!("unknown subcommand: {other}")),
    };
    if global.json && !matches!(sub, Subcommand::Set { .. } | Subcommand::Driver { .. }) {
        return Err("--json is supported by status, set and driver".to_owned());
    }
    Ok((global, sub))
}

fn parse_timeout(value: &str) -> Result<Duration, String> {
    let milliseconds: u64 = value
        .parse()
        .map_err(|_| "--timeout-ms requires an integer from 1 to 3600000".to_owned())?;
    if !(1..=3_600_000).contains(&milliseconds) {
        return Err("--timeout-ms must be from 1 to 3600000".to_owned());
    }
    Ok(Duration::from_millis(milliseconds))
}

fn parse_control_options(
    args: &mut std::iter::Peekable<std::vec::IntoIter<String>>,
    global: &mut GlobalOptions,
) -> Result<(), String> {
    while let Some(option) = args.peek().map(String::as_str) {
        match option {
            "--json" => {
                args.next();
                global.json = true;
            }
            "--timeout-ms" => {
                args.next();
                global.timeout = parse_timeout(&take_value(args, "--timeout-ms")?)?;
            }
            _ => break,
        }
    }
    Ok(())
}

fn report_error(json: bool, kind: &str, message: &str, exit: u8) -> ExitCode {
    if json {
        println!(
            "{{\"schema_version\":1,\"ok\":false,\"error\":{{\"kind\":\"{kind}\",\"message\":\"{}\"}}}}",
            logging::escape_json(message)
        );
    } else {
        eprintln!("error: {message}");
    }
    ExitCode::from(exit)
}

fn run_control(config: &Config, line: &str, global: &GlobalOptions) -> ExitCode {
    let snapshot = global.json
        && matches!(
            vrdummy_core::Command::parse(line),
            Ok(vrdummy_core::Command::Status | vrdummy_core::Command::StatusJson)
        );
    let command = if snapshot { "status --json" } else { line };
    let reply =
        match control::send_line_with_timeout(&config.control.pipe_name, command, global.timeout) {
            Ok(reply) => reply,
            Err(error) => {
                let (kind, exit) = if error.kind() == std::io::ErrorKind::TimedOut {
                    ("timeout", 4)
                } else {
                    ("transport_error", 3)
                };
                return report_error(global.json, kind, &error.to_string(), exit);
            }
        };
    if let Some(error) = reply.strip_prefix("err ") {
        if global.json {
            return report_error(true, "command_error", error, 1);
        }
        println!("{reply}");
        return ExitCode::FAILURE;
    }
    let value = if reply == "ok" {
        "null".to_owned()
    } else if reply == "quit" {
        "{\"quit\":true}".to_owned()
    } else if let Some(text) = reply.strip_prefix("ok ") {
        if snapshot {
            // A legacy server ignores the status option and returns text.
            // Refuse that response instead of emitting malformed JSON.
            if !text.starts_with("{\"schema_version\":1,") || !text.ends_with('}') {
                return report_error(
                    global.json,
                    "protocol_error",
                    "server does not support JSON status; restart the GUI or feeder with the matching version",
                    5,
                );
            }
            text.to_owned()
        } else {
            format!("\"{}\"", logging::escape_json(text))
        }
    } else {
        return report_error(
            global.json,
            "protocol_error",
            "unrecognised control reply",
            5,
        );
    };
    if global.json {
        println!("{{\"schema_version\":1,\"ok\":true,\"result\":{value}}}");
    } else {
        println!("{reply}");
    }
    ExitCode::SUCCESS
}

fn load_config(global: &GlobalOptions) -> Result<Config, String> {
    match &global.config {
        Some(path) => Config::load(path).map_err(|e| e.to_string()),
        None => Ok(Config::default()),
    }
}

fn init_logging(global: &GlobalOptions, config: &Config) -> Result<(), String> {
    let level_name = global.level.as_deref().unwrap_or(&config.log.level);
    let level = logging::parse_level(level_name)
        .ok_or_else(|| format!("unknown log level: {level_name}"))?;
    let file = global
        .log_file
        .clone()
        .or_else(|| (!config.log.file.is_empty()).then(|| PathBuf::from(&config.log.file)));
    logging::init(file.as_deref(), level, true).map_err(|e| format!("log file: {e}"))
}

fn print_reply(reply: &Reply) {
    if let Reply::Ok(text) = reply
        && !text.is_empty()
    {
        println!("{text}");
    }
}

fn run(
    config: Config,
    record: Option<&Path>,
    serve_control: bool,
    read_stdin: bool,
) -> Result<(), String> {
    let mut engine = Engine::new(config);
    if let Some(path) = record {
        engine
            .start_recording(path)
            .map_err(|e| format!("record: {e}"))?;
        log::info!("recording to {}", path.display());
    }
    if !engine.try_connect() {
        log::warn!("driver not reachable; will keep retrying in the background");
    }
    log::debug!("engine ready; starting control server");
    let engine: SharedEngine = Arc::new(Mutex::new(engine));
    let quit = Arc::new(AtomicBool::new(false));

    let mut control_server = None;
    if serve_control {
        let name = engine
            .lock()
            .map_err(|_| "engine lock")?
            .config()
            .control
            .pipe_name
            .clone();
        control_server = Some(
            ControlServer::start(&name, Arc::clone(&engine), Arc::clone(&quit))
                .map_err(|e| e.to_string())?,
        );
    }

    let (tx, rx) = mpsc::channel::<String>();
    if read_stdin {
        std::thread::Builder::new()
            .name("vrdummy-stdin".to_owned())
            .spawn(move || {
                for line in std::io::stdin().lock().lines() {
                    let Ok(line) = line else { break };
                    if tx.send(line).is_err() {
                        break;
                    }
                }
            })
            .map_err(|e| e.to_string())?;
    }

    log::info!("running; type commands, `quit` to exit");
    while !quit.load(Ordering::SeqCst) {
        {
            let mut e = engine.lock().map_err(|_| "engine lock")?;
            e.poll();
        }
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(line) => {
                if line.trim().is_empty() {
                    continue;
                }
                let mut e = engine.lock().map_err(|_| "engine lock")?;
                match e.execute_line(&line) {
                    Ok(Reply::Quit) => quit.store(true, Ordering::SeqCst),
                    Ok(reply) => print_reply(&reply),
                    Err(err) => eprintln!("error: {err}"),
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if read_stdin {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
    if let Some(mut s) = control_server {
        s.stop();
    }
    engine.lock().map_err(|_| "engine lock")?.stop_recording();
    Ok(())
}

fn run_scenario(config: Config, file: &Path, record: Option<&Path>) -> Result<(), String> {
    let steps = scenario::load(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let mut engine = Engine::new(config);
    if let Some(path) = record {
        engine
            .start_recording(path)
            .map_err(|e| format!("record: {e}"))?;
    }
    if !engine.try_connect() {
        log::warn!("driver not reachable; commands are applied locally only");
    }
    let start = Instant::now();
    let mut failures = 0usize;
    for step in &steps {
        let at = Duration::try_from_secs_f64(step.at_s)
            .map_err(|e| format!("invalid scenario time {}: {e}", step.at_s))?;
        while start.elapsed() < at {
            engine.poll();
            std::thread::sleep(
                at.saturating_sub(start.elapsed())
                    .min(Duration::from_millis(50)),
            );
        }
        match engine.execute_line(&step.line) {
            Ok(Reply::Quit) => break,
            Ok(reply) => {
                log::info!("{:.3} {}", step.at_s, step.line);
                print_reply(&reply);
            }
            Err(e) => {
                failures += 1;
                log::error!("{:.3} {}: {e}", step.at_s, step.line);
            }
        }
    }
    engine.stop_recording();
    if failures > 0 {
        return Err(format!("{failures} step(s) failed"));
    }
    Ok(())
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv == ["--help"] || argv == ["-h"] {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let json_requested = argv.iter().any(|arg| arg == "--json");
    let (global, sub) = match parse_args(argv) {
        Ok(v) => v,
        Err(msg) => {
            if json_requested {
                return report_error(true, "argument_error", &msg, 2);
            }
            if !msg.is_empty() {
                eprintln!("error: {msg}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    if let Subcommand::Driver { action, options } = &sub {
        return match installation::execute(*action, options, global.timeout) {
            Ok(status) => {
                if global.json {
                    let json = match status.to_json() {
                        Ok(json) => json,
                        Err(error) => {
                            return report_error(true, "driver_management_error", &error, 1);
                        }
                    };
                    println!("{{\"schema_version\":1,\"ok\":true,\"result\":{json}}}");
                } else {
                    println!(
                        "registered={} path={} package_valid={} tool_available={} changed={}",
                        status.registered,
                        status.driver_dir.display(),
                        status.package_valid,
                        status.tool_available,
                        status.changed
                    );
                    if status.restart_required {
                        println!("Start or restart SteamVR to apply the registration change.");
                    }
                }
                ExitCode::SUCCESS
            }
            Err(error) => report_error(global.json, "driver_management_error", &error, 1),
        };
    }
    let config = match load_config(&global) {
        Ok(c) => c,
        Err(e) => {
            return report_error(global.json, "argument_error", &e, 2);
        }
    };
    if let Err(e) = init_logging(&global, &config) {
        return report_error(global.json, "argument_error", &e, 2);
    }

    if let Subcommand::Set { line } = &sub {
        return run_control(&config, line, &global);
    }

    let result = match sub {
        Subcommand::Run {
            record,
            control,
            stdin,
        } => run(config, record.as_deref(), control, stdin),
        Subcommand::Scenario { file, record } => run_scenario(config, &file, record.as_deref()),
        Subcommand::Set { .. } | Subcommand::Driver { .. } => unreachable!("commands return above"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("error: {msg}");
            }
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn parses_run_with_options() {
        let (g, sub) =
            parse_args(argv("--level debug run --record r.txt --no-stdin")).expect("parse");
        assert_eq!(g.level.as_deref(), Some("debug"));
        match sub {
            Subcommand::Run {
                record,
                control,
                stdin,
            } => {
                assert_eq!(record, Some(PathBuf::from("r.txt")));
                assert!(control);
                assert!(!stdin);
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn parses_set_joining_words() {
        let (_, sub) = parse_args(argv("set pos hmd 0 1 0")).expect("parse");
        match sub {
            Subcommand::Set { line } => assert_eq!(line, "pos hmd 0 1 0"),
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn scenario_playback_records_replayable_commands() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../.tmp")
            .join(format!("scenario-record-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("test directory");
        let input = dir.join("input.txt");
        let output = dir.join("output.txt");
        std::fs::write(
            &input,
            "0 pos hmd 0 1.8 0\n0 move hmd 1 0 0\n0 status\n0 quit\n0 move hmd 5 0 0\n",
        )
        .expect("scenario");
        let mut config = Config::default();
        config.driver.pipe_name = format!("vrdummy-no-driver-{}", std::process::id());
        run_scenario(config, &input, Some(&output)).expect("playback");
        let recorded = scenario::load(&output).expect("replayable recording");
        assert_eq!(recorded.len(), 2);
        assert_eq!(recorded[0].line, "pos hmd 0 1.8 0");
        assert_eq!(recorded[1].line, "move hmd 1 0 0");
        let mut replay = Engine::new(Config::default());
        for step in recorded {
            replay.execute(&step.command).expect("replay");
        }
        let position = replay.world().pose(0).expect("hmd").position;
        for (actual, expected) in position.into_iter().zip([1.0, 1.8, 0.0]) {
            assert!((actual - expected).abs() < 1e-9);
        }
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn rejects_unknown() {
        assert!(parse_args(argv("fly")).is_err());
        assert!(parse_args(argv("--config")).is_err());
        assert!(parse_args(argv("set")).is_err());
    }

    #[test]
    fn parses_driver_management_without_a_running_feeder() {
        let (global, sub) = parse_args(
            vec![
                "driver",
                "register",
                "--path",
                "C:/driver with spaces",
                "--json",
                "--timeout-ms",
                "5000",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        )
        .unwrap();
        assert!(global.json);
        assert_eq!(global.timeout, Duration::from_secs(5));
        let Subcommand::Driver { action, options } = sub else {
            panic!("driver command")
        };
        assert_eq!(action, Action::Register);
        assert_eq!(
            options.driver_dir,
            Some(PathBuf::from("C:/driver with spaces"))
        );
        for invalid in [
            "driver",
            "driver delete",
            "driver status --unknown",
            "driver register --path",
        ] {
            assert!(parse_args(argv(invalid)).is_err());
        }
    }

    #[test]
    fn parses_agent_control_options_and_rejects_ambiguous_input() {
        for line in [
            "--json --timeout-ms 150 status",
            "status --json --timeout-ms 150",
            "set --json --timeout-ms 150 status",
        ] {
            let (global, command) = parse_args(argv(line)).expect("agent options");
            assert!(global.json);
            assert_eq!(global.timeout, Duration::from_millis(150));
            assert!(matches!(command, Subcommand::Set { line } if line == "status"));
        }
        for line in [
            "status surprise",
            "status --timeout-ms 0",
            "status --timeout-ms NaN",
            "status --timeout-ms 3600001",
            "--json run",
        ] {
            assert!(parse_args(argv(line)).is_err(), "{line}");
        }
    }
}
