//! Application configuration — TOML-based, loaded from disk.
//!
//! The config file lives at `~/.config/dos/config.toml` on Linux/macOS and
//! `%APPDATA%\dos\config.toml` on Windows.
//!
//! Every field has a sensible default so the agent can start with zero config.

use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::{constants::*, CommonError};

/// Top-level application configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Human-readable name for this node (defaults to the OS hostname).
    pub node_name: String,
    /// Relay server URL (e.g. `ws://relay.example.com:7890`).
    pub relay_url: String,
    /// Local WebSocket port this node listens on.
    pub node_port: u16,
    /// Path to the SQLite database file.
    pub db_path: String,
    /// Logging level filter string (e.g. `"info"`, `"debug"`).
    pub log_level: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            node_name: hostname(),
            relay_url: format!("ws://127.0.0.1:{DEFAULT_RELAY_PORT}"),
            node_port: DEFAULT_NODE_PORT,
            db_path: DEFAULT_DB_FILENAME.to_string(),
            log_level: "info".to_string(),
        }
    }
}

impl Config {
    /// Load configuration from `path`.
    ///
    /// If the file does not exist, a default config is returned and a warning
    /// is logged. This allows the agent to start without any config file.
    ///
    /// # Errors
    /// Returns [`CommonError::ConfigRead`] if the file exists but cannot be
    /// read, or [`CommonError::ConfigParse`] if it is not valid TOML.
    pub fn load(path: &str) -> Result<Self, CommonError> {
        match std::fs::read_to_string(path) {
            Ok(contents) => {
                toml::from_str(&contents).map_err(|e| CommonError::ConfigParse(e.to_string()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                warn!(path, "config file not found — using defaults");
                Ok(Self::default())
            }
            Err(e) => Err(CommonError::ConfigRead(e.to_string())),
        }
    }
}

/// Best-effort hostname retrieval. Falls back to `"unknown-node"`.
fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| {
            // Try reading /etc/hostname on Linux
            std::fs::read_to_string("/etc/hostname").map(|s| s.trim().to_string())
        })
        .unwrap_or_else(|_| "unknown-node".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_sensible_relay_url() {
        let cfg = Config::default();
        assert!(cfg.relay_url.starts_with("ws://"));
        assert_eq!(cfg.node_port, DEFAULT_NODE_PORT);
    }

    #[test]
    fn load_nonexistent_returns_default() {
        let cfg = Config::load("/tmp/dos_test_nonexistent_config.toml")
            .expect("should return default");
        assert_eq!(cfg.node_port, DEFAULT_NODE_PORT);
    }
}
