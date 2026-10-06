//! JSON Lines logger.
//!
//! Each record is one line: `{"ts_us":<monotonic>,"level":"INFO","target":"<module>","msg":"..."}`.
//! Records go to stderr in a short human form and, when configured, to a file
//! in JSON Lines. Log lines forwarded by the driver are injected with the
//! target `driver`.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::Instant;

use log::{Level, LevelFilter, Log, Metadata, Record};

struct Sinks {
    file: Option<File>,
    stderr: bool,
}

struct JsonlLogger {
    start: Instant,
    sinks: Mutex<Sinks>,
}

static LOGGER: OnceLock<JsonlLogger> = OnceLock::new();

/// Microseconds since the logger was initialised.
pub fn monotonic_micros() -> u64 {
    LOGGER
        .get()
        .map_or(0, |l| l.start.elapsed().as_micros() as u64)
}

/// Escapes a string for inclusion inside a JSON string literal.
pub fn escape_json(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                use std::fmt::Write as _;
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out
}

/// Formats one JSON Lines record (without the trailing newline).
pub fn format_record(ts_us: u64, level: Level, target: &str, msg: &str) -> String {
    format!(
        "{{\"ts_us\":{ts_us},\"level\":\"{}\",\"target\":\"{}\",\"msg\":\"{}\"}}",
        level.as_str(),
        escape_json(target),
        escape_json(msg)
    )
}

impl JsonlLogger {
    fn write(&self, ts_us: u64, level: Level, target: &str, msg: &str) {
        let Ok(mut sinks) = self.sinks.lock() else {
            return;
        };
        if sinks.stderr {
            let _ = writeln!(
                std::io::stderr(),
                "{:>10.3} {:<5} {} {}",
                ts_us as f64 / 1_000_000.0,
                level.as_str(),
                target,
                msg
            );
        }
        if let Some(file) = sinks.file.as_mut() {
            let line = format_record(ts_us, level, target, msg);
            let _ = writeln!(file, "{line}");
        }
    }
}

impl Log for JsonlLogger {
    fn enabled(&self, metadata: &Metadata<'_>) -> bool {
        metadata.level() <= log::max_level()
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let ts = self.start.elapsed().as_micros() as u64;
        self.write(ts, record.level(), record.target(), &record.args().to_string());
    }

    fn flush(&self) {
        if let Ok(mut sinks) = self.sinks.lock()
            && let Some(file) = sinks.file.as_mut()
        {
            let _ = file.flush();
        }
    }
}

/// Parses a level name. Unknown names return `None`.
pub fn parse_level(name: &str) -> Option<LevelFilter> {
    match name.trim().to_ascii_lowercase().as_str() {
        "off" => Some(LevelFilter::Off),
        "error" => Some(LevelFilter::Error),
        "warn" => Some(LevelFilter::Warn),
        "info" => Some(LevelFilter::Info),
        "debug" => Some(LevelFilter::Debug),
        "trace" => Some(LevelFilter::Trace),
        _ => None,
    }
}

/// Installs the logger. `VRDUMMY_LOG` overrides `level`. Calling it twice
/// keeps the first configuration and returns `Ok(())`.
pub fn init(file: Option<&Path>, level: LevelFilter, stderr: bool) -> std::io::Result<()> {
    let file = match file {
        Some(p) if !p.as_os_str().is_empty() => Some(OpenOptions::new().create(true).append(true).open(p)?),
        _ => None,
    };
    let logger = LOGGER.get_or_init(|| JsonlLogger {
        start: Instant::now(),
        sinks: Mutex::new(Sinks { file, stderr }),
    });
    let level = std::env::var("VRDUMMY_LOG")
        .ok()
        .and_then(|v| parse_level(&v))
        .unwrap_or(level);
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level);
    }
    Ok(())
}

/// Writes a record that originated in the driver, keeping its own timestamp.
pub fn log_driver_record(level: Level, driver_ts_us: u64, text: &str) {
    if let Some(logger) = LOGGER.get()
        && level <= log::max_level()
    {
        logger.write(monotonic_micros(), level, "driver", &format!("[{driver_ts_us}] {text}"));
    }
}

/// Maps a protocol log level onto the `log` crate level.
pub fn level_from_protocol(level: vrdummy_protocol::LogLevel) -> Level {
    use vrdummy_protocol::LogLevel as P;
    match level {
        P::Error => Level::Error,
        P::Warn => Level::Warn,
        P::Info => Level::Info,
        P::Debug => Level::Debug,
        P::Trace => Level::Trace,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escaping() {
        assert_eq!(escape_json("a\"b\\c\nd"), "a\\\"b\\\\c\\nd");
        assert_eq!(escape_json("\u{1}"), "\\u0001");
        assert_eq!(escape_json("plain"), "plain");
    }

    #[test]
    fn record_format() {
        let line = format_record(12, Level::Info, "core::engine", "hello \"x\"");
        assert_eq!(
            line,
            "{\"ts_us\":12,\"level\":\"INFO\",\"target\":\"core::engine\",\"msg\":\"hello \\\"x\\\"\"}"
        );
    }

    #[test]
    fn level_names() {
        assert_eq!(parse_level("DEBUG"), Some(LevelFilter::Debug));
        assert_eq!(parse_level(" warn "), Some(LevelFilter::Warn));
        assert_eq!(parse_level("loud"), None);
    }
}
