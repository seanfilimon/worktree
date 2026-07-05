//! IPC message types.
//!
//! The [`Command`] enum mirrors the dispatch table in BgProcess.md §14.3.
//! Commands serialize to the spec's dotted names (`snapshot.create`,
//! `sync.push`, …) via the `command` field of [`Request`]; arguments travel
//! in the untyped `args` object so the wire format stays exactly what the
//! spec shows.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A request from the CLI to the daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    /// Correlation id, echoed back in the response.
    pub id: String,
    /// Dotted command name, e.g. `"snapshot.create"`.
    pub command: String,
    /// Command arguments (shape depends on the command).
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub args: Value,
}

impl Request {
    /// Build a request with a fresh correlation id.
    pub fn new(command: Command, args: Value) -> Self {
        Self {
            id: format!("req-{}", uuid::Uuid::new_v4().simple()),
            command: command.name().to_string(),
            args,
        }
    }
}

/// A response from the daemon to the CLI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    /// Correlation id from the originating request.
    pub id: String,
    /// `ok` or `error`.
    pub status: ResponseStatus,
    /// Payload on success, error details on failure.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

impl Response {
    pub fn ok(id: impl Into<String>, data: Value) -> Self {
        Self {
            id: id.into(),
            status: ResponseStatus::Ok,
            data,
        }
    }

    pub fn error(id: impl Into<String>, message: &str) -> Self {
        Self {
            id: id.into(),
            status: ResponseStatus::Error,
            data: serde_json::json!({ "message": message }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    Ok,
    Error,
}

/// The daemon's command surface (BgProcess.md §14.3).
///
/// Mutating commands are serialized through the daemon's single-writer
/// lock; read-only commands may run concurrently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// Query watcher + engine state (`wt status`).
    Status,
    /// Trigger a manual snapshot (`wt snapshot`).
    SnapshotCreate,
    /// Query the snapshot store (`wt snapshot list`).
    SnapshotList,
    /// Create a branch (`wt branch create`).
    BranchCreate,
    /// Switch branches, snapshotting first if dirty (`wt branch switch`).
    BranchSwitch,
    /// List branches (`wt branch list`).
    BranchList,
    /// Delete a branch (`wt branch delete`).
    BranchDelete,
    /// Finalize staged snapshots into history (`wt push`).
    SyncPush,
    /// Trigger an immediate sync cycle (`wt sync`).
    SyncTrigger,
    /// Compute a diff (`wt diff`).
    DiffCompute,
    /// Query the snapshot DAG (`wt log`).
    LogQuery,
    /// Start a merge (`wt merge`).
    MergeStart,
    /// Query reflog entries (`wt reflog`).
    ReflogQuery,
    /// Daemon self-inspection (`wt server status`).
    DaemonInfo,
    /// Graceful daemon shutdown (`wt server stop`).
    DaemonShutdown,
}

impl Command {
    /// The dotted wire name from the spec's dispatch table.
    pub fn name(&self) -> &'static str {
        match self {
            Command::Status => "status",
            Command::SnapshotCreate => "snapshot.create",
            Command::SnapshotList => "snapshot.list",
            Command::BranchCreate => "branch.create",
            Command::BranchSwitch => "branch.switch",
            Command::BranchList => "branch.list",
            Command::BranchDelete => "branch.delete",
            Command::SyncPush => "sync.push",
            Command::SyncTrigger => "sync.trigger",
            Command::DiffCompute => "diff.compute",
            Command::LogQuery => "log.query",
            Command::MergeStart => "merge.start",
            Command::ReflogQuery => "reflog.query",
            Command::DaemonInfo => "daemon.info",
            Command::DaemonShutdown => "daemon.shutdown",
        }
    }

    /// Parse a dotted wire name back into a command.
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "status" => Command::Status,
            "snapshot.create" => Command::SnapshotCreate,
            "snapshot.list" => Command::SnapshotList,
            "branch.create" => Command::BranchCreate,
            "branch.switch" => Command::BranchSwitch,
            "branch.list" => Command::BranchList,
            "branch.delete" => Command::BranchDelete,
            "sync.push" => Command::SyncPush,
            "sync.trigger" => Command::SyncTrigger,
            "diff.compute" => Command::DiffCompute,
            "log.query" => Command::LogQuery,
            "merge.start" => Command::MergeStart,
            "reflog.query" => Command::ReflogQuery,
            "daemon.info" => Command::DaemonInfo,
            "daemon.shutdown" => Command::DaemonShutdown,
            _ => return None,
        })
    }

    /// True if the command mutates worktree state and must be serialized
    /// through the daemon's single-writer lock (BgProcess.md §14.4).
    pub fn is_mutating(&self) -> bool {
        matches!(
            self,
            Command::SnapshotCreate
                | Command::BranchCreate
                | Command::BranchSwitch
                | Command::BranchDelete
                | Command::SyncPush
                | Command::SyncTrigger
                | Command::MergeStart
                | Command::DaemonShutdown
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[Command] = &[
        Command::Status,
        Command::SnapshotCreate,
        Command::SnapshotList,
        Command::BranchCreate,
        Command::BranchSwitch,
        Command::BranchList,
        Command::BranchDelete,
        Command::SyncPush,
        Command::SyncTrigger,
        Command::DiffCompute,
        Command::LogQuery,
        Command::MergeStart,
        Command::ReflogQuery,
        Command::DaemonInfo,
        Command::DaemonShutdown,
    ];

    #[test]
    fn names_roundtrip() {
        for cmd in ALL {
            assert_eq!(Command::from_name(cmd.name()), Some(*cmd), "{:?}", cmd);
        }
        assert_eq!(Command::from_name("no.such.command"), None);
    }

    #[test]
    fn request_wire_shape_matches_spec() {
        let req = Request {
            id: "req-001".into(),
            command: Command::Status.name().into(),
            args: serde_json::json!({ "tree_id": "backend" }),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["id"], "req-001");
        assert_eq!(json["command"], "status");
        assert_eq!(json["args"]["tree_id"], "backend");
    }

    #[test]
    fn response_wire_shape_matches_spec() {
        let resp = Response::ok("req-001", serde_json::json!({ "watcher_active": true }));
        let json = serde_json::to_value(&resp).unwrap();
        assert_eq!(json["id"], "req-001");
        assert_eq!(json["status"], "ok");
        assert_eq!(json["data"]["watcher_active"], true);
    }
}
