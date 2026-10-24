use serde::{Deserialize, Serialize};

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
