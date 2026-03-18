use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ServerError;

/// Top-level server configuration, typically loaded from a TOML file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Directory where the server stores its data (objects, indexes, etc.).
    pub data_dir: PathBuf,

    /// Address the server listens on, e.g. `"127.0.0.1:9876"`.
    pub listen_addr: String,

    /// Configuration for automatic snapshot creation.
    #[serde(default)]
    pub auto_snapshot: AutoSnapshotConfig,

    /// Configuration for the file-system watcher.
    #[serde(default)]
    pub watcher: WatcherConfig,
}

/// Controls automatic snapshot (commit) creation based on file-change heuristics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSnapshotConfig {
    /// Whether auto-snapshot is enabled.
    pub enabled: bool,

    /// Number of seconds of inactivity after changes before a snapshot is created.
    pub inactivity_timeout_secs: u64,

    /// If the number of changed files exceeds this threshold, create a snapshot immediately.
    pub max_changed_files: usize,
}

impl Default for AutoSnapshotConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            inactivity_timeout_secs: 30,
            max_changed_files: 50,
        }
    }
}

/// Configuration for the file-system watcher subsystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WatcherConfig {
    /// Debounce delay in milliseconds — rapid successive events within this
    /// window are collapsed into a single logical event.
    pub debounce_ms: u64,

    /// Glob patterns for paths the watcher should ignore (e.g. `"target/**"`).
    pub ignore_patterns: Vec<String>,
}

impl Default for WatcherConfig {
    fn default() -> Self {
        Self {
            debounce_ms: 200,
            ignore_patterns: vec![
                ".git/**".to_string(),
                ".worktree/**".to_string(),
                "target/**".to_string(),
                "node_modules/**".to_string(),
            ],
        }
    }
}

impl ServerConfig {
    /// Load a `ServerConfig` from a TOML file at the given path.
    ///
    /// Returns a `ServerError::Config` if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, ServerError> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            ServerError::Config(format!("failed to read config file {}: {}", path.display(), e))
        })?;

        let config: ServerConfig = toml::from_str(&contents).map_err(|e| {
            ServerError::Config(format!(
                "failed to parse config file {}: {}",
                path.display(),
                e
            ))
        })?;

        Ok(config)
    }
}
