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
use vrdummy_core::{Config, Engine, logging, scenario};

const USAGE: &str = "\
usage: vrdummy-cli [--config <file>] [--log <file>] [--level <level>] <subcommand>

subcommands:
  run      [--record <file>] [--no-control] [--no-stdin]
           connect to the driver, serve the control pipe and read commands from stdin
  scenario <file> [--record <file>]
           execute a timed scenario file, then exit
  set      <command...>
           send one command to a running instance and print the reply
  status   same as `set status`

commands are documented in docs/control.md";

struct GlobalOptions {
    config: Option<PathBuf>,
    log_file: Option<PathBuf>,
    level: Option<String>,
}

enum Subcommand {
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
    };
    let sub = loop {
        let Some(arg) = args.next() else {
            return Err("missing subcommand".to_owned());
        };
        match arg.as_str() {
            "--config" => global.config = Some(PathBuf::from(take_value(&mut args, "--config")?)),
            "--log" => global.log_file = Some(PathBuf::from(take_value(&mut args, "--log")?)),
            "--level" => global.level = Some(take_value(&mut args, "--level")?),
            "-h" | "--help" => return Err(String::new()),
            s if s.starts_with('-') => return Err(format!("unknown option: {s}")),
            _ => break arg,
        }
    };
    let sub = match sub.as_str() {
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
            let rest: Vec<String> = args.collect();
            if rest.is_empty() {
                return Err("set: missing command".to_owned());
            }
            Subcommand::Set {
                line: rest.join(" "),
            }
        }
        "status" => Subcommand::Set {
            line: "status".to_owned(),
        },
        other => return Err(format!("unknown subcommand: {other}")),
    };
    Ok((global, sub))
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
        let due = start + Duration::from_secs_f64(step.at_s);
        while Instant::now() < due {
            engine.poll();
            std::thread::sleep(
                due.saturating_duration_since(Instant::now())
                    .min(Duration::from_millis(50)),
            );
        }
        match engine.execute(&step.command) {
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
    let (global, sub) = match parse_args(argv) {
        Ok(v) => v,
        Err(msg) => {
            if !msg.is_empty() {
                eprintln!("error: {msg}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let config = match load_config(&global) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = init_logging(&global, &config) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    let result = match sub {
        Subcommand::Run {
            record,
            control,
            stdin,
        } => run(config, record.as_deref(), control, stdin),
        Subcommand::Scenario { file, record } => run_scenario(config, &file, record.as_deref()),
        Subcommand::Set { line } => match control::send_line(&config.control.pipe_name, &line) {
            Ok(reply) => {
                println!("{reply}");
                if reply.starts_with("err") {
                    Err(String::new())
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(format!(
                "control pipe: {e} (is `vrdummy-cli run` or the GUI running?)"
            )),
        },
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
    fn rejects_unknown() {
        assert!(parse_args(argv("fly")).is_err());
        assert!(parse_args(argv("--config")).is_err());
        assert!(parse_args(argv("set")).is_err());
    }
}
