//! Lifecycle hooks (`.wt/hooks/`, DotWt.md).
//!
//! A hook is a script in `.wt/hooks/` named for its hook point. On Windows
//! the engine looks for `<name>.bat`, `<name>.cmd`, or `<name>.ps1`; on
//! Unix it looks for the bare `<name>` and runs it via `sh`. Context is
//! passed through `WT_*` environment variables.
//!
//! `pre-*` hooks are gating: a non-zero exit aborts the operation.
//! `post-*` hooks are informational: failures are logged and ignored.

use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use std::path::PathBuf;
use std::process::Command;

/// Hook points recognized by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    PreSnapshot,
    PostSnapshot,
    PrePush,
}

impl Hook {
    /// File name of the hook script inside `.wt/hooks/`.
    pub fn file_name(&self) -> &'static str {
        match self {
            Hook::PreSnapshot => "pre-snapshot",
            Hook::PostSnapshot => "post-snapshot",
            Hook::PrePush => "pre-push",
        }
    }

    /// True if a non-zero exit must abort the surrounding operation.
    pub fn is_gating(&self) -> bool {
        matches!(self, Hook::PreSnapshot | Hook::PrePush)
    }
}

/// What happened when a hook point was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookOutcome {
    /// No hook script is installed.
    NotInstalled,
    /// The hook ran and exited zero.
    Ran,
}

fn find_script(engine: &WorktreeEngine, hook: Hook) -> Option<PathBuf> {
    let dir = engine.wt_dir().join("hooks");
    if cfg!(windows) {
        for ext in ["bat", "cmd", "ps1"] {
            let candidate = dir.join(format!("{}.{ext}", hook.file_name()));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    } else {
        let candidate = dir.join(hook.file_name());
        candidate.is_file().then_some(candidate)
    }
}

fn command_for(script: &PathBuf) -> Command {
    if cfg!(windows) {
        if script.extension().is_some_and(|e| e == "ps1") {
            let mut cmd = Command::new("powershell");
            cmd.args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(script);
            cmd
        } else {
            let mut cmd = Command::new("cmd");
            cmd.arg("/C").arg(script);
            cmd
        }
    } else {
        let mut cmd = Command::new("sh");
        cmd.arg(script);
        cmd
    }
}

/// Run a hook if installed. `env` carries the `WT_*` context variables.
///
/// Gating hooks return [`EngineError::PermissionDenied`]-style failure on a
/// non-zero exit; non-gating hooks only log.
pub fn run(engine: &WorktreeEngine, hook: Hook, env: &[(&str, &str)]) -> Result<HookOutcome> {
    let Some(script) = find_script(engine, hook) else {
        return Ok(HookOutcome::NotInstalled);
    };

    let mut command = command_for(&script);
    command.current_dir(engine.root());
    command.env("WT_ROOT", engine.root());
    for (key, value) in env {
        command.env(key, value);
    }

    let output = command
        .output()
        .map_err(|e| EngineError::InvalidConfig(format!("hook {}: {e}", hook.file_name())))?;

    if output.status.success() {
        Ok(HookOutcome::Ran)
    } else if hook.is_gating() {
        Err(EngineError::PermissionDenied(format!(
            "hook {} rejected the operation: {}",
            hook.file_name(),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    } else {
        tracing::warn!(
            "hook {} failed (ignored): {}",
            hook.file_name(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        Ok(HookOutcome::Ran)
    }
}
