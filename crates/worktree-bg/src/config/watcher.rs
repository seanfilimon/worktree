use serde::{Deserialize, Serialize};

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
