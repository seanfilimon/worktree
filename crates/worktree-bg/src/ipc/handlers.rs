//! IPC command handlers.
//!
//! Each handler opens the engine, performs the operation, and returns a
//! JSON payload. All run on the blocking pool (engine ops are file I/O)
//! except the daemon-control commands handled inline by the dispatcher.

use crate::error::BgError;
use crate::service::daemon::DaemonContext;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::Ordering;
use worktree_engine::{ops, WorktreeEngine};
use worktree_ipc::Command;

/// `daemon.info` — served inline from daemon state (no engine access).
pub fn daemon_info(ctx: &DaemonContext) -> Value {
    let health = ctx
        .health
        .lock()
        .map(|h| h.status())
        .unwrap_or_else(|poisoned| poisoned.into_inner().status());
    json!({
        "pid": std::process::id(),
        "version": env!("CARGO_PKG_VERSION"),
        "root": ctx.root.display().to_string(),
        "uptime_secs": health.uptime_secs,
        "snapshots_created": health.snapshots_created,
        "watcher_active": ctx.watcher_active.load(Ordering::SeqCst),
        "auto_snapshot": {
            "enabled": ctx.auto_snapshot.enabled,
            "inactivity_timeout_secs": ctx.auto_snapshot.inactivity_timeout_secs,
            "max_changed_files": ctx.auto_snapshot.max_changed_files,
        },
    })
}

/// Dispatch table for engine-backed commands.
pub fn execute_blocking(root: &Path, command: Command, args: Value) -> Result<Value, BgError> {
    let engine = WorktreeEngine::open(root).map_err(engine_err)?;
    match command {
        Command::Status => status(&engine),
        Command::SnapshotCreate => snapshot_create(&engine, &args),
        Command::SnapshotList | Command::LogQuery => log_query(&engine, &args),
        Command::BranchCreate => branch_create(&engine, &args),
        Command::BranchSwitch => branch_switch(&engine, &args),
        Command::BranchList => branch_list(&engine),
        Command::BranchDelete => branch_delete(&engine, &args),
        Command::MergeStart => merge_start(&engine, &args),
        Command::DiffCompute => diff_compute(&engine, &args),
        Command::ReflogQuery => reflog_query(&engine, &args),
        Command::SyncPush => sync_push(&engine),
        Command::SyncTrigger => sync_trigger(&engine),
        Command::DaemonInfo | Command::DaemonShutdown => {
            unreachable!("daemon-control commands are handled inline")
        }
    }
}

fn engine_err(e: worktree_engine::EngineError) -> BgError {
    BgError::Engine(e.to_string())
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, BgError> {
    serde_json::to_value(value).map_err(|e| BgError::Engine(format!("serialize response: {e}")))
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, BgError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| BgError::Engine(format!("missing argument '{key}'")))
}

fn arg_opt_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(Value::as_str)
}

fn arg_count(args: &Value, default: usize) -> usize {
    args.get("count")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(default)
}

fn status(engine: &WorktreeEngine) -> Result<Value, BgError> {
    let status = ops::status::compute_status(engine).map_err(engine_err)?;
    to_value(&status)
}

fn snapshot_create(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    let message = arg_str(args, "message")?;
    let tree = arg_opt_str(args, "tree");
    let snapshot = ops::snapshot::create_snapshot(engine, tree, message).map_err(engine_err)?;
    to_value(&snapshot)
}

fn log_query(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    let snapshots = ops::log::show_log(engine, arg_count(args, 20)).map_err(engine_err)?;
    to_value(&snapshots)
}

fn branch_create(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    let branch =
        ops::branch::create_branch(engine, arg_str(args, "name")?, None).map_err(engine_err)?;
    to_value(&branch)
}

fn branch_switch(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    ops::branch::switch_branch(engine, arg_str(args, "name")?, None).map_err(engine_err)?;
    Ok(json!({ "switched": true }))
}

fn branch_list(engine: &WorktreeEngine) -> Result<Value, BgError> {
    let (branches, current) = ops::branch::list_branches(engine, None).map_err(engine_err)?;
    Ok(json!({ "branches": branches, "current": current }))
}

fn branch_delete(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    ops::branch::delete_branch(engine, arg_str(args, "name")?, None).map_err(engine_err)?;
    Ok(json!({ "deleted": true }))
}

fn merge_start(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    let result = ops::merge::merge_branch(engine, arg_str(args, "source")?).map_err(engine_err)?;
    to_value(&result)
}

fn diff_compute(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    let entries = match (arg_opt_str(args, "from"), arg_opt_str(args, "to")) {
        (Some(from), Some(to)) => {
            ops::diff::diff_snapshots(engine, from, to).map_err(engine_err)?
        }
        _ => ops::diff::diff_working_tree(engine).map_err(engine_err)?,
    };
    to_value(&entries)
}

fn reflog_query(engine: &WorktreeEngine, args: &Value) -> Result<Value, BgError> {
    let entries = ops::reflog::show_reflog(engine, arg_count(args, 20)).map_err(engine_err)?;
    to_value(&entries)
}

fn sync_push(engine: &WorktreeEngine) -> Result<Value, BgError> {
    let result = ops::sync::push(engine).map_err(engine_err)?;
    to_value(&result)
}

fn sync_trigger(engine: &WorktreeEngine) -> Result<Value, BgError> {
    let result = ops::sync::pull(engine).map_err(engine_err)?;
    to_value(&result)
}
