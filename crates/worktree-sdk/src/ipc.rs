//! CLI <-> background daemon (`worktree-bg`) protocol.
//!
//! Transport: newline-delimited JSON over TCP on 127.0.0.1. One request line,
//! one response line per connection. The daemon writes
//! `$WT_HOME/bg.json` = `{"pid": u32, "port": u16, "version": "..."}` on
//! startup and removes it on clean shutdown.

use std::path::PathBuf;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum IpcRequest {
    Status,
    /// Start watching a worktree root.
    Watch { path: PathBuf },
    Unwatch { path: PathBuf },
    /// Pause/resume auto-snapshots (all worktrees if path is None).
    Pause { path: Option<PathBuf> },
    Resume { path: Option<PathBuf> },
    /// Take an auto-snapshot now (all trees of the worktree).
    SnapshotNow { path: PathBuf },
    Shutdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum IpcResponse {
    Status(DaemonStatus),
    Ok { message: String },
    Error { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub pid: u32,
    pub version: String,
    pub started_at: DateTime<Utc>,
    pub watched: Vec<WatchedWorktree>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedWorktree {
    pub root: PathBuf,
    pub paused: bool,
    pub pending_changes: usize,
    pub snapshots_created: u64,
    pub last_snapshot: Option<String>,
    pub last_snapshot_at: Option<DateTime<Utc>>,
    pub last_stage_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonInfo {
    pub pid: u32,
    pub port: u16,
    pub version: String,
}

/// `$WT_HOME/bg.json`.
pub fn daemon_info_path() -> PathBuf {
    crate::config::wt_home().join("bg.json")
}

/// Read `bg.json` if present (does not check liveness).
pub fn read_daemon_info() -> Result<Option<DaemonInfo>> {
    todo!("bg-daemon: read_daemon_info")
}

/// Send one request to the running daemon. `DaemonNotRunning` if bg.json is
/// missing or the connection is refused (stale file).
pub async fn send(req: &IpcRequest) -> Result<IpcResponse> {
    let _ = req;
    todo!("bg-daemon: send")
}
