//! `wt daemon`: start and control the `worktree-bg` background daemon.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::config::wt_home;
use worktree_sdk::ipc::{self, DaemonStatus, IpcRequest, IpcResponse};
use worktree_sdk::{Repository, SdkError};

use crate::context::{block_on, Context};
use crate::error::{CliError, Result};
use crate::output::format::{ago, plural, short};
use crate::output::Text;
use crate::paths;

/// How long `wt daemon start` waits for the daemon to answer.
const START_TIMEOUT: Duration = Duration::from_secs(15);
const START_POLL: Duration = Duration::from_millis(100);

#[derive(Debug, Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub command: DaemonCommand,
}

#[derive(Debug, Subcommand)]
pub enum DaemonCommand {
    /// Start the daemon in the background
    Start {
        /// Run attached to this terminal instead
        #[arg(long)]
        foreground: bool,
    },
    /// Stop the daemon
    Stop,
    /// Show the daemon's state and watched worktrees
    Status,
    /// Auto-snapshot a worktree (default: the current one)
    Watch { path: Option<PathBuf> },
    /// Stop watching a worktree (default: the current one)
    Unwatch { path: Option<PathBuf> },
    /// Pause auto-snapshots
    Pause,
    /// Resume auto-snapshots
    Resume,
}

#[derive(Serialize)]
struct Message<'a> {
    message: &'a str,
}

pub fn run(ctx: &Context, args: DaemonArgs) -> Result<()> {
    let request = match args.command {
        DaemonCommand::Start { foreground: true } => return start_foreground(),
        DaemonCommand::Start { foreground: false } => return start(ctx),
        DaemonCommand::Status => return status(ctx),
        DaemonCommand::Stop => IpcRequest::Shutdown,
        DaemonCommand::Watch { path } => IpcRequest::Watch {
            path: worktree_root(ctx, path, false)?,
        },
        DaemonCommand::Unwatch { path } => IpcRequest::Unwatch {
            path: worktree_root(ctx, path, true)?,
        },
        DaemonCommand::Pause => IpcRequest::Pause { path: None },
        DaemonCommand::Resume => IpcRequest::Resume { path: None },
    };
    let message = expect_ok(block_on(ipc::send(&request))??)?;
    ctx.out
        .emit(&Message { message: &message }, |t| t.line(&message))
}

fn status(ctx: &Context) -> Result<()> {
    let status = match block_on(ipc::send(&IpcRequest::Status))?? {
        IpcResponse::Status(status) => status,
        other => return Err(unexpected(&other)),
    };
    ctx.out
        .emit(&status, |t| render_status(t, &status, "running"))
}

fn start(ctx: &Context) -> Result<()> {
    if let Some(status) = block_on(running_status())?? {
        return ctx
            .out
            .emit(&status, |t| render_status(t, &status, "already running"));
    }
    let binary = daemon_binary()?;
    let mut child = spawn_detached(&binary)
        .map_err(|e| CliError::failure(format!("cannot start '{}': {e}", binary.display())))?;
    let status = block_on(wait_until_ready(&mut child))??;
    ctx.out
        .emit(&status, |t| render_status(t, &status, "started"))
}

/// Run the daemon attached to the terminal until it exits.
fn start_foreground() -> Result<()> {
    let binary = daemon_binary()?;
    let status = Command::new(&binary)
        .args(["run", "--foreground"])
        .status()
        .map_err(|e| CliError::failure(format!("cannot start '{}': {e}", binary.display())))?;
    if status.success() {
        Ok(())
    } else {
        Err(CliError::failure(format!(
            "worktree-bg exited with {status}"
        )))
    }
}

/// The daemon's status if one is running and answering.
async fn running_status() -> Result<Option<DaemonStatus>> {
    if ipc::read_daemon_info()?.is_none() {
        return Ok(None);
    }
    match ipc::send(&IpcRequest::Status).await {
        Ok(IpcResponse::Status(status)) => Ok(Some(status)),
        Ok(other) => Err(unexpected(&other)),
        Err(SdkError::DaemonNotRunning) => Ok(None),
        Err(e) => Err(e.into()),
    }
}

async fn wait_until_ready(child: &mut Child) -> Result<DaemonStatus> {
    let deadline = Instant::now() + START_TIMEOUT;
    let log = wt_home().join("bg.log");
    loop {
        if let Some(exit) = child.try_wait()? {
            return Err(
                CliError::failure(format!("worktree-bg exited during startup ({exit})"))
                    .with_hint(format!("see {}", log.display())),
            );
        }
        if let Some(status) = running_status().await? {
            return Ok(status);
        }
        if Instant::now() >= deadline {
            return Err(CliError::failure(format!(
                "worktree-bg did not respond within {} seconds",
                START_TIMEOUT.as_secs()
            ))
            .with_hint(format!("see {}", log.display())));
        }
        tokio::time::sleep(START_POLL).await;
    }
}

fn spawn_detached(binary: &Path) -> std::io::Result<Child> {
    // Run from $WT_HOME so the daemon never pins the caller's directory.
    let home = wt_home();
    std::fs::create_dir_all(&home)?;
    let mut command = Command::new(binary);
    command
        .arg("run")
        .current_dir(&home)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn()
}

fn daemon_binary() -> Result<PathBuf> {
    let exe = std::env::current_exe().ok();
    let exe_dir = exe.as_deref().and_then(Path::parent);
    find_daemon_binary(exe_dir, std::env::var_os("PATH").as_deref()).ok_or_else(|| {
        CliError::failure("cannot find the worktree-bg executable")
            .with_hint("install worktree-bg next to wt or on your PATH")
    })
}

/// `worktree-bg` next to the current executable, else the first on `PATH`.
fn find_daemon_binary(exe_dir: Option<&Path>, path_var: Option<&OsStr>) -> Option<PathBuf> {
    let name = format!("worktree-bg{}", std::env::consts::EXE_SUFFIX);
    exe_dir
        .map(|dir| dir.join(&name))
        .into_iter()
        .chain(
            path_var
                .into_iter()
                .flat_map(std::env::split_paths)
                .map(|dir| dir.join(&name)),
        )
        .find(|candidate| candidate.is_file())
}

/// The worktree root for `path` (default: cwd). When `allow_missing`, a
/// path that is no longer a worktree is passed through as-is so stale
/// watches can still be removed.
fn worktree_root(ctx: &Context, path: Option<PathBuf>, allow_missing: bool) -> Result<PathBuf> {
    let base = path.map_or_else(|| ctx.cwd().to_path_buf(), |p| ctx.absolute(&p));
    match Repository::open(&base) {
        Ok(repo) => Ok(repo.root().to_path_buf()),
        Err(SdkError::NotAWorktree) if allow_missing => Ok(paths::normalize(&base)),
        Err(e) => Err(e.into()),
    }
}

fn expect_ok(response: IpcResponse) -> Result<String> {
    match response {
        IpcResponse::Ok { message } => Ok(message),
        IpcResponse::Error { message } => Err(CliError::failure(message)),
        other => Err(unexpected(&other)),
    }
}

fn unexpected(response: &IpcResponse) -> CliError {
    CliError::failure(format!("unexpected response from the daemon: {response:?}"))
}

fn render_status(t: &mut Text, status: &DaemonStatus, state: &str) {
    t.line(format_args!(
        "worktree-bg {} (pid {}, version {}, started {})",
        state.green().bold(),
        status.pid,
        status.version,
        ago(status.started_at)
    ));
    if status.watched.is_empty() {
        t.line("Not watching any worktrees (add one with `wt daemon watch`).");
        return;
    }
    t.line(format_args!(
        "Watching {}:",
        plural(status.watched.len(), "worktree", "worktrees")
    ));
    for w in &status.watched {
        let state = if w.paused {
            "paused".yellow()
        } else {
            "active".green()
        };
        let last = match (&w.last_snapshot, w.last_snapshot_at) {
            (Some(id), Some(at)) => format!("last {} {}", short(id), ago(at)),
            _ => "no snapshots yet".to_string(),
        };
        t.line(format_args!(
            "  {}  {state}  {} pending, {}, {last}",
            w.root.display().to_string().bold(),
            w.pending_changes,
            plural(
                usize::try_from(w.snapshots_created).unwrap_or(usize::MAX),
                "snapshot",
                "snapshots"
            )
        ));
        if let Some(at) = w.last_stage_at {
            t.line(format_args!("      last staged {}", ago(at)));
        }
        if let Some(err) = &w.last_error {
            t.line(format_args!("      {} {err}", "error:".red()));
        }
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;
    use worktree_sdk::ipc::WatchedWorktree;

    use super::*;

    fn touch(path: &Path) {
        std::fs::write(path, b"").expect("write");
    }

    #[test]
    fn finds_binary_next_to_exe_first() {
        let exe_dir = tempfile::tempdir().expect("tempdir");
        let path_dir = tempfile::tempdir().expect("tempdir");
        let name = format!("worktree-bg{}", std::env::consts::EXE_SUFFIX);
        touch(&exe_dir.path().join(&name));
        touch(&path_dir.path().join(&name));
        let path_var = std::env::join_paths([path_dir.path()]).expect("join");
        assert_eq!(
            find_daemon_binary(Some(exe_dir.path()), Some(&path_var)),
            Some(exe_dir.path().join(&name))
        );
    }

    #[test]
    fn falls_back_to_path() {
        let exe_dir = tempfile::tempdir().expect("tempdir");
        let empty = tempfile::tempdir().expect("tempdir");
        let path_dir = tempfile::tempdir().expect("tempdir");
        let name = format!("worktree-bg{}", std::env::consts::EXE_SUFFIX);
        touch(&path_dir.path().join(&name));
        let path_var = std::env::join_paths([empty.path(), path_dir.path()]).expect("join");
        assert_eq!(
            find_daemon_binary(Some(exe_dir.path()), Some(&path_var)),
            Some(path_dir.path().join(&name))
        );
        assert_eq!(find_daemon_binary(Some(exe_dir.path()), None), None);
        assert_eq!(find_daemon_binary(None, None), None);
    }

    #[test]
    fn ok_responses() {
        assert_eq!(
            expect_ok(IpcResponse::Ok {
                message: "done".into()
            })
            .expect("ok"),
            "done"
        );
        let err = expect_ok(IpcResponse::Error {
            message: "nope".into(),
        })
        .expect_err("error");
        assert_eq!(err.to_string(), "nope");
    }

    #[test]
    fn status_rendering() {
        colored::control::set_override(false);
        let mut status = DaemonStatus {
            pid: 42,
            version: "0.1.0".into(),
            started_at: Utc::now(),
            watched: vec![],
        };
        let mut t = Text::default();
        render_status(&mut t, &status, "running");
        assert!(t
            .as_str()
            .starts_with("worktree-bg running (pid 42, version 0.1.0, started just now)\n"));
        assert!(t.as_str().contains("Not watching any worktrees"));

        status.watched.push(WatchedWorktree {
            root: PathBuf::from("proj"),
            paused: true,
            pending_changes: 3,
            snapshots_created: 1,
            last_snapshot: Some("0123456789abcdef".into()),
            last_snapshot_at: Some(Utc::now()),
            last_stage_at: None,
            last_error: Some("disk full".into()),
        });
        let mut t = Text::default();
        render_status(&mut t, &status, "running");
        let out = t.as_str();
        assert!(out.contains("Watching 1 worktree:"), "{out}");
        assert!(
            out.contains("proj  paused  3 pending, 1 snapshot, last 01234567 just now"),
            "{out}"
        );
        assert!(out.contains("error: disk full"), "{out}");
    }
}
