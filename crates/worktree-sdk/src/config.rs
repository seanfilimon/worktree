//! `.wt/config.toml` and the per-user `$WT_HOME` directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Per-user state directory: `$WT_HOME` if set, else `~/.wt`.
/// Holds `credentials.toml`, `bg.json`, `bg.toml`, `bg.log`.
pub fn wt_home() -> PathBuf {
    if let Some(p) = std::env::var_os("WT_HOME") {
        return PathBuf::from(p);
    }
    dirs_home().join(".wt")
}

fn dirs_home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeConfig {
    pub worktree: WorktreeSection,
    #[serde(default)]
    pub user: UserSection,
    #[serde(default)]
    pub auto_snapshot: AutoSnapshotSection,
    #[serde(default)]
    pub sync: SyncSection,
    /// Named remotes, e.g. `[remote.origin]`.
    #[serde(default)]
    pub remote: std::collections::BTreeMap<String, RemoteSection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeSection {
    pub name: String,
    pub id: String,
    pub created_at: String,
    #[serde(default = "default_branch")]
    pub default_branch: String,
}

fn default_branch() -> String {
    "main".into()
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserSection {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub email: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutoSnapshotSection {
    pub enabled: bool,
    /// Snapshot after this many seconds without file events.
    pub inactivity_secs: u64,
    /// Snapshot immediately once this many files have changed.
    pub max_changed_files: usize,
}

impl Default for AutoSnapshotSection {
    fn default() -> Self {
        Self { enabled: true, inactivity_secs: 30, max_changed_files: 50 }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncSection {
    /// Upload auto-snapshots as staged (team-visible) snapshots.
    pub auto_stage: bool,
    /// Remote used by the daemon.
    pub remote: String,
}

impl Default for SyncSection {
    fn default() -> Self {
        Self { auto_stage: true, remote: "origin".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteSection {
    /// Server base URL, e.g. `http://localhost:7420` (gRPC + REST share it).
    pub url: String,
    /// Tenant slug on the server.
    pub tenant: String,
}

impl WorktreeConfig {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&text)?)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Get a dotted key ("user.name", "remote.origin.url", ...).
    pub fn get(&self, key: &str) -> Option<String> {
        let _ = key;
        todo!("sdk-core: WorktreeConfig::get")
    }

    /// Set a dotted key; `InvalidConfig` for unknown keys or bad values.
    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        let _ = (key, value);
        todo!("sdk-core: WorktreeConfig::set")
    }

    /// All keys as (dotted key, value), sorted.
    pub fn entries(&self) -> Vec<(String, String)> {
        todo!("sdk-core: WorktreeConfig::entries")
    }
}
