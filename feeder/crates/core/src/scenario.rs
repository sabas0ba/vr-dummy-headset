//! Scenario files: one command per line with a leading time in seconds.
//!
//! ```text
//! # time  command
//! 0.0     preset stand
//! 1.0     pos hmd 0 1.7 0
//! 1.5     input ctrl-r a/click 1
//! 1.6     input ctrl-r a/click 0
//! ```
//!
//! Times are absolute from the start of the run and must not decrease. The
//! same format is produced by recording (`Engine::start_recording`), so a
//! recorded session replays through the same code.

use std::path::Path;

use crate::command::{Command, CommandError};

/// One timed step.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// Seconds from the start.
    pub at_s: f64,
    /// Command to execute.
    pub command: Command,
    /// Original text, kept for logging.
    pub line: String,
}

/// Parse error with its line number (1-based).
#[derive(Debug, PartialEq)]
pub struct ScenarioError {
    /// Line number.
    pub line: usize,
    /// What went wrong.
    pub message: String,
}

impl std::fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for ScenarioError {}

/// Parses scenario text.
pub fn parse(text: &str) -> Result<Vec<Step>, ScenarioError> {
    let mut steps = Vec::new();
    let mut last = 0.0f64;
    for (idx, raw) in text.lines().enumerate() {
        let line_no = idx + 1;
        let content = raw.split('#').next().unwrap_or("").trim();
        if content.is_empty() {
            continue;
        }
        let (time, rest) = content
            .split_once(char::is_whitespace)
            .ok_or_else(|| ScenarioError {
                line: line_no,
                message: "expected '<time> <command>'".to_owned(),
            })?;
        let at_s: f64 = time.parse().map_err(|_| ScenarioError {
            line: line_no,
            message: format!("bad time: {time}"),
        })?;
        if at_s < last {
            return Err(ScenarioError {
                line: line_no,
                message: format!("time {at_s} is earlier than previous {last}"),
            });
        }
        last = at_s;
        let command = Command::parse(rest).map_err(|e: CommandError| ScenarioError {
            line: line_no,
            message: e.to_string(),
        })?;
        steps.push(Step {
            at_s,
            command,
            line: rest.trim().to_owned(),
        });
    }
    Ok(steps)
}

/// Reads and parses a scenario file.
pub fn load(path: &Path) -> Result<Vec<Step>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    Ok(parse(&text)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::DeviceRef;

    #[test]
    fn parses_timed_lines_and_skips_comments() {
        let steps = parse("# header\n0 preset stand\n\n1.5 pos hmd 0 1.7 0 # note\n").expect("parse");
        assert_eq!(steps.len(), 2);
        assert!((steps[1].at_s - 1.5).abs() < 1e-12);
        assert_eq!(steps[1].command, Command::Position(DeviceRef::Hmd, [0.0, 1.7, 0.0]));
        assert_eq!(steps[1].line, "pos hmd 0 1.7 0");
    }

    #[test]
    fn rejects_decreasing_time() {
        let err = parse("1 status\n0.5 status\n").expect_err("must fail");
        assert_eq!(err.line, 2);
    }

    #[test]
    fn reports_command_errors_with_line() {
        let err = parse("0 status\n1 fly\n").expect_err("must fail");
        assert_eq!(err.line, 2);
        assert!(err.message.contains("unknown command"));
    }
}
