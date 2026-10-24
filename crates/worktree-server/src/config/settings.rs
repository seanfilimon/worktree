use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ServerError;

/// Top-level server configuration, typically loaded from a TOML file.
///
/// The remote server owns only its own concerns (data directory + bind
/// address). Bg-side concerns (watcher debounce, auto-snapshot
/// thresholds) live in `worktree-bg/src/config/` and are not part of
/// this struct — the two runtimes load and own their config
/// independently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Directory where the server stores its data (objects, indexes, etc.).
    pub data_dir: PathBuf,

    /// Address the server listens on, e.g. `"127.0.0.1:9876"`.
    pub listen_addr: String,
}

impl ServerConfig {
    /// Load a `ServerConfig` from a TOML file at the given path.
    ///
    /// Returns a `ServerError::Config` if the file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, ServerError> {
        let contents = std::fs::read_to_string(path).map_err(|e| {
            ServerError::Config(format!(
                "failed to read config file {}: {}",
                path.display(),
                e
            ))
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
