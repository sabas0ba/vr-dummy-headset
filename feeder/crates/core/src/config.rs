//! Feeder configuration file (TOML).

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Driver connection settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct DriverConfig {
    /// Named pipe without the `\\.\pipe\` prefix. Must match `driver_vrdummy.pipe_name`.
    pub pipe_name: String,
    /// Delay between reconnection attempts in milliseconds.
    pub reconnect_interval_ms: u64,
    /// How long to wait for the driver handshake in milliseconds.
    pub handshake_timeout_ms: u64,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            pipe_name: vrdummy_protocol::DEFAULT_PIPE_NAME.to_owned(),
            reconnect_interval_ms: 1000,
            handshake_timeout_ms: 2000,
        }
    }
}

/// Control API settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct ControlConfig {
    /// Named pipe without the `\\.\pipe\` prefix.
    pub pipe_name: String,
}

impl Default for ControlConfig {
    fn default() -> Self {
        Self {
            pipe_name: "vrdummy-control".to_owned(),
        }
    }
}

/// OSC target (VRChat listens on 9000 by default).
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct OscConfig {
    /// Destination host.
    pub host: String,
    /// Destination port.
    pub port: u16,
}

impl Default for OscConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_owned(),
            port: 9000,
        }
    }
}

/// Logging settings.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct LogConfig {
    /// JSON Lines file. Empty disables the file sink.
    pub file: String,
    /// Level name: error, warn, info, debug, trace. `VRDUMMY_LOG` overrides it.
    pub level: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            file: String::new(),
            level: "info".to_owned(),
        }
    }
}

/// Top-level configuration.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Driver connection.
    pub driver: DriverConfig,
    /// Control API.
    pub control: ControlConfig,
    /// OSC target.
    pub osc: OscConfig,
    /// Logging.
    pub log: LogConfig,
}

/// Error loading a configuration file.
#[derive(Debug)]
pub enum ConfigError {
    /// The file could not be read.
    Io(PathBuf, std::io::Error),
    /// The file is not valid TOML for [`Config`].
    Parse(PathBuf, toml::de::Error),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(p, e) => write!(f, "{}: {e}", p.display()),
            Self::Parse(p, e) => write!(f, "{}: {e}", p.display()),
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Loads a TOML file. Missing keys take their defaults.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(path.to_owned(), e))?;
        Self::parse(&text).map_err(|e| ConfigError::Parse(path.to_owned(), e))
    }

    /// Parses TOML text.
    pub fn parse(text: &str) -> Result<Self, toml::de::Error> {
        toml::from_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_text_gives_defaults() {
        let c = Config::parse("").expect("parse");
        assert_eq!(c.driver.pipe_name, "vrdummy-driver");
        assert_eq!(c.osc.port, 9000);
        assert_eq!(c.log.level, "info");
    }

    #[test]
    fn partial_sections_keep_other_defaults() {
        let c = Config::parse("[osc]\nport = 9100\n[log]\nfile = \"a.jsonl\"\n").expect("parse");
        assert_eq!(c.osc.port, 9100);
        assert_eq!(c.osc.host, "127.0.0.1");
        assert_eq!(c.log.file, "a.jsonl");
        assert_eq!(c.control.pipe_name, "vrdummy-control");
    }

    #[test]
    fn unknown_type_is_an_error() {
        assert!(Config::parse("[osc]\nport = \"x\"\n").is_err());
    }
}
