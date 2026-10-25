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
    /// Stop the daemon (if running) and start it again
    Restart,
    /// Show the daemon's log ($WT_HOME/bg.log)
    Logs {
        /// Number of trailing lines to show
        #[arg(short = 'n', long = "lines", value_name = "N", default_value_t = 50)]
        lines: usize,
        /// Keep printing new lines as they are written (Ctrl-C to stop)
        #[arg(short, long)]
        follow: bool,
    },
    /// Snapshot every changed tree of a worktree now (default: the current one)
    Snapshot {
        path: Option<PathBuf>,
        /// Snapshot every watched worktree
        #[arg(long, conflicts_with = "path")]
        all: bool,
    },
}

/// How long `wt daemon restart` waits for the old daemon to exit (its
/// shutdown snapshots every pending change first).
const STOP_TIMEOUT: Duration = Duration::from_secs(90);

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
        DaemonCommand::Restart => return restart(ctx),
        DaemonCommand::Logs { lines, follow } => return logs(ctx, lines, follow),
        DaemonCommand::Snapshot { all: true, .. } => return snapshot_all(ctx),
        DaemonCommand::Snapshot { path, all: false } => IpcRequest::SnapshotNow {
            path: worktree_root(ctx, path, false)?,
        },
    };
    let message = expect_ok(block_on(ipc::send(&request))??)?;
    ctx.out
        .emit(&Message { message: &message }, |t| t.line(&message))
}

/// The `wt status` hint: `Some` when auto-snapshot is enabled for `repo`
/// but no daemon is running, or the running daemon does not watch it.
pub fn status_hint(repo: &Repository) -> Option<String> {
    if !repo.config().auto_snapshot.enabled {
        return None;
    }
    let status = match ipc::read_daemon_info() {
        // No bg.json: not running (no connection attempt needed).
        Ok(None) => None,
        Ok(Some(_)) => match block_on(ipc::send(&IpcRequest::Status)) {
            Ok(Ok(IpcResponse::Status(status))) => Some(status),
            Ok(Err(SdkError::DaemonNotRunning)) => None,
            // Unreachable for other reasons (bad token, timeout): stay quiet.
            _ => return None,
        },
        Err(_) => return None,
    };
    hint_for(repo.root(), status.as_ref())
}

fn hint_for(root: &Path, status: Option<&DaemonStatus>) -> Option<String> {
    let Some(status) = status else {
        return Some(
            "hint: auto-snapshot is enabled but the background daemon is not running \
             (start it with `wt daemon start`)"
                .into(),
        );
    };
    // The daemon reports canonical roots; compare canonical forms.
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| paths::normalize(p));
    let root = canon(root);
    let watched = status
        .watched
        .iter()
        .any(|w| root.starts_with(canon(&w.root)));
    (!watched).then(|| {
        "hint: the background daemon is not watching this worktree \
         (enable auto-snapshots with `wt daemon watch`)"
            .into()
    })
}

/// Start the daemon unless one is running, then watch `root`. Used by
/// `wt init --watch` and `wt clone --watch`; returns the daemon's reply.
pub fn start_and_watch(root: &Path) -> Result<String> {
    if block_on(running_status())??.is_none() {
        let binary = daemon_binary()?;
        let mut child = spawn_detached(&binary)
            .map_err(|e| CliError::failure(format!("cannot start '{}': {e}", binary.display())))?;
        block_on(wait_until_ready(&mut child))??;
    }
    expect_ok(block_on(ipc::send(&IpcRequest::Watch {
        path: root.to_path_buf(),
    }))??)
}

fn restart(ctx: &Context) -> Result<()> {
    if block_on(running_status())??.is_some() {
        expect_ok(block_on(ipc::send(&IpcRequest::Shutdown))??)?;
        block_on(wait_until_stopped())??;
    }
    start(ctx)
}

/// Wait until the daemon no longer answers.
async fn wait_until_stopped() -> Result<()> {
    let deadline = Instant::now() + STOP_TIMEOUT;
    loop {
        if running_status().await?.is_none() {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(CliError::failure(format!(
                "worktree-bg did not stop within {} seconds",
                STOP_TIMEOUT.as_secs()
            ))
            .with_hint(format!("see {}", wt_home().join("bg.log").display())));
        }
        tokio::time::sleep(START_POLL).await;
    }
}

#[derive(Serialize)]
struct SnapshotAll {
    results: Vec<SnapshotResult>,
}

#[derive(Serialize)]
struct SnapshotResult {
    root: PathBuf,
    ok: bool,
    message: String,
}

/// `wt daemon snapshot --all`: SnapshotNow for every watched worktree.
fn snapshot_all(ctx: &Context) -> Result<()> {
    let status = match block_on(ipc::send(&IpcRequest::Status))?? {
        IpcResponse::Status(status) => status,
        other => return Err(unexpected(&other)),
    };
    let mut results = Vec::new();
    for w in status.watched {
        let resp = block_on(ipc::send(&IpcRequest::SnapshotNow {
            path: w.root.clone(),
        }))??;
        let (ok, message) = match resp {
            IpcResponse::Ok { message } => (true, message),
            IpcResponse::Error { message } => (false, message),
            other => return Err(unexpected(&other)),
        };
        results.push(SnapshotResult {
            root: w.root,
            ok,
            message,
        });
    }
    let failed = results.iter().filter(|r| !r.ok).count();
    let out = SnapshotAll { results };
    ctx.out.emit(&out, |t| {
        if out.results.is_empty() {
            t.line("Not watching any worktrees.");
        }
        for r in &out.results {
            let mark = if r.ok { "ok".green() } else { "failed".red() };
            t.line(format_args!("{}  {mark}  {}", r.root.display(), r.message));
        }
    })?;
    if failed > 0 {
        return Err(CliError::failure(format!(
            "{} worktree(s) could not be snapshotted",
            failed
        )));
    }
    Ok(())
}

#[derive(Serialize)]
struct LogTail<'a> {
    path: &'a Path,
    lines: Vec<String>,
}

/// `wt daemon logs [-n N] [--follow]`.
fn logs(ctx: &Context, lines: usize, follow: bool) -> Result<()> {
    let path = wt_home().join("bg.log");
    let text = match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && follow => String::new(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(
                CliError::failure(format!("no daemon log at {}", path.display()))
                    .with_hint("start the daemon with `wt daemon start`"),
            )
        }
        Err(e) => return Err(e.into()),
    };
    let tail = tail_lines(&text, lines);
    let out = LogTail {
        path: &path,
        lines: tail.iter().map(|l| l.to_string()).collect(),
    };
    ctx.out.emit(&out, |t| {
        for line in &out.lines {
            t.line(line);
        }
    })?;
    if follow && !ctx.out.is_json() {
        follow_log(&path, text.len() as u64)?;
    }
    Ok(())
}

/// The last `n` lines of `text`.
fn tail_lines(text: &str, n: usize) -> Vec<&str> {
    let all: Vec<&str> = text.lines().collect();
    all[all.len().saturating_sub(n)..].to_vec()
}

/// Print bytes appended to `path` after `offset` until interrupted; a log
/// rotated by a daemon restart is followed from its start.
fn follow_log(path: &Path, mut offset: u64) -> Result<()> {
    use std::io::{Read, Seek, SeekFrom, Write};
    let stdout = std::io::stdout();
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let Ok(mut file) = std::fs::File::open(path) else {
            continue;
        };
        let len = file.metadata()?.len();
        if len < offset {
            offset = 0;
        }
        if len == offset {
            continue;
        }
        file.seek(SeekFrom::Start(offset))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        offset += buf.len() as u64;
        let mut out = stdout.lock();
        out.write_all(&buf)?;
        out.flush()?;
    }
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
        Ok(IpcResponse::Error { message }) => Err(CliError::failure(message)),
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
    // CreateProcess inherits every inheritable handle, including wt's own
    // stdout/stderr: a long-lived daemon would then keep a caller's pipe
    // open (`wt init --watch | cat` would never finish). The guard clears
    // the flag until the spawn below is done.
    #[cfg(windows)]
    let _no_inherit = {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        command.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
        windows_stdio::NoInherit::new()
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command.spawn()
}

#[cfg(windows)]
mod windows_stdio {
    use windows_sys::Win32::Foundation::{
        GetHandleInformation, SetHandleInformation, HANDLE, HANDLE_FLAG_INHERIT,
        INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::System::Console::{
        GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    /// Clears HANDLE_FLAG_INHERIT on this process's std handles while alive,
    /// restoring the previous flags on drop.
    pub struct NoInherit(Vec<(HANDLE, u32)>);

    impl NoInherit {
        pub fn new() -> Self {
            let mut saved = Vec::new();
            for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
                // SAFETY: plain Win32 calls on handles owned by this process;
                // invalid or null handles are skipped.
                unsafe {
                    let h = GetStdHandle(which);
                    if h.is_null() || h == INVALID_HANDLE_VALUE {
                        continue;
                    }
                    let mut flags = 0u32;
                    if GetHandleInformation(h, &mut flags) == 0 {
                        continue;
                    }
                    if flags & HANDLE_FLAG_INHERIT != 0
                        && SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0) != 0
                    {
                        saved.push((h, flags));
                    }
                }
            }
            Self(saved)
        }
    }

    impl Drop for NoInherit {
        fn drop(&mut self) {
            for &(h, flags) in &self.0 {
                // SAFETY: restores the flag on the same handle.
                unsafe {
                    SetHandleInformation(h, HANDLE_FLAG_INHERIT, flags & HANDLE_FLAG_INHERIT);
                }
            }
        }
    }
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
        if let Some(state) = &w.sync_state {
            let fetched = match w.last_fetch_at {
                Some(at) => format!(", last fetch {}", ago(at)),
                None => String::new(),
            };
            t.line(format_args!("      sync: {state}{fetched}"));
        }
        if let Some(err) = &w.sync_error {
            t.line(format_args!("      {} {err}", "sync error:".red()));
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
            last_fetch_at: Some(Utc::now()),
            sync_state: Some("watching".into()),
            sync_error: Some("server unavailable".into()),
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
        assert!(out.contains("sync: watching, last fetch just now"), "{out}");
        assert!(out.contains("sync error: server unavailable"), "{out}");
    }

    #[test]
    fn status_hints() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("proj");
        std::fs::create_dir_all(root.join("sub")).expect("mkdir");
        let hint = hint_for(&root, None).expect("not running");
        assert!(hint.contains("wt daemon start"), "{hint}");

        let mut status = DaemonStatus {
            pid: 1,
            version: "x".into(),
            started_at: Utc::now(),
            watched: vec![],
        };
        let hint = hint_for(&root, Some(&status)).expect("not watched");
        assert!(hint.contains("wt daemon watch"), "{hint}");

        status.watched.push(WatchedWorktree {
            root: root.clone(),
            paused: false,
            pending_changes: 0,
            snapshots_created: 0,
            last_snapshot: None,
            last_snapshot_at: None,
            last_stage_at: None,
            last_error: None,
            last_fetch_at: None,
            sync_state: None,
            sync_error: None,
        });
        assert_eq!(hint_for(&root, Some(&status)), None);
        assert_eq!(hint_for(&root.join("sub"), Some(&status)), None);
    }

    #[test]
    fn log_tail() {
        assert_eq!(tail_lines("a\nb\nc\n", 2), vec!["b", "c"]);
        assert_eq!(tail_lines("a\nb", 10), vec!["a", "b"]);
        assert!(tail_lines("", 5).is_empty());
        assert!(tail_lines("a\n", 0).is_empty());
    }
}
