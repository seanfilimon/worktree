//! Snapshot hooks: executables under `.wt/hooks/<phase>` (worktree-wide)
//! and `<tree>/.wt-tree/hooks/<phase>` (per tree), run around snapshots
//! (docs/ENGINEERING_PLAN.md §13.9).
//!
//! * Order: `.wt/hooks` first, then the hooks of every tree enclosing the
//!   snapshotted tree (outermost first), then the tree's own hooks.
//! * Discovery: on Unix the file `<phase>` must be executable. On Windows
//!   `<phase>.exe`, `<phase>.cmd`, `<phase>.bat`, `<phase>.ps1` (run with
//!   PowerShell) and an extensionless `<phase>` script (run with `sh`, as
//!   Git for Windows does) are recognized; the first one found per
//!   directory runs.
//! * Environment: `WT_HOOK` (= `WT_HOOK_PHASE`, the phase name), `WT_TREE`
//!   (= `WT_TREE_NAME`), `WT_TREE_PATH` (absolute tree directory),
//!   `WT_BRANCH`, `WT_ROOT` (worktree root), `WT_USER` (snapshot author) and
//!   for post hooks `WT_SNAPSHOT` (= `WT_SNAPSHOT_ID`). The working
//!   directory is the tree root and stdin is empty.
//! * `[hooks] enabled` turns hooks off, `[hooks] timeout_secs` (default 60,
//!   0 = no limit) kills a hook (and its child processes) that runs longer.
//! * A failing or timed-out pre hook aborts the snapshot with
//!   `SdkError::Other("pre-snapshot hook failed: …")`, including the tail of
//!   its output. Post hook failures are reported on stderr only.
//! * Hook output is forwarded to stderr (never stdout, which may carry
//!   `--json` output).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{Result, SdkError};
use crate::model::Snapshot;
use crate::repo::{Repository, TreeRecord, TREE_MARKER_DIR};

/// Directory holding hooks inside `.wt/` and `.wt-tree/`.
pub const HOOKS_DIR: &str = "hooks";

/// Characters of hook output quoted in an abort message.
const MAX_QUOTED_OUTPUT: usize = 2000;
/// How long to wait for a hook's output pipes after it exited (a background
/// grandchild may keep them open).
const PIPE_GRACE: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// When a hook runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before the working state is read; a non-zero exit aborts the snapshot.
    PreSnapshot,
    /// After the snapshot is written; failures are reported, not fatal.
    PostSnapshot,
}

impl Phase {
    /// File name of the hook (`pre-snapshot`, `post-snapshot`).
    pub fn file_name(self) -> &'static str {
        match self {
            Phase::PreSnapshot => "pre-snapshot",
            Phase::PostSnapshot => "post-snapshot",
        }
    }
}

/// Hook file extensions recognized on this platform, in priority order
/// ("" = the bare `<phase>` file).
#[cfg(windows)]
const EXTENSIONS: &[&str] = &["exe", "cmd", "bat", "ps1", ""];
#[cfg(not(windows))]
const EXTENSIONS: &[&str] = &[""];

/// The hook executable for `phase` in `dir`, if any.
fn hook_in(dir: &Path, phase: Phase) -> Option<PathBuf> {
    for ext in EXTENSIONS {
        let name = if ext.is_empty() {
            phase.file_name().to_string()
        } else {
            format!("{}.{ext}", phase.file_name())
        };
        let path = dir.join(name);
        let Ok(meta) = std::fs::metadata(&path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            if !crate::repo::fsutil::is_executable(&meta) {
                tracing::warn!(path = %path.display(), "hook is not executable; skipped (chmod +x to enable)");
                continue;
            }
        }
        return Some(path);
    }
    None
}

/// The hooks that run for `tree` in `phase`, in execution order.
pub fn find_hooks(repo: &Repository, tree: &str, phase: Phase) -> Result<Vec<PathBuf>> {
    let mut dirs = vec![repo.wt_dir().join(HOOKS_DIR)];
    for (_, dir) in repo.tree_ancestry(tree)? {
        dirs.push(dir.join(TREE_MARKER_DIR).join(HOOKS_DIR));
    }
    Ok(dirs.iter().filter_map(|d| hook_in(d, phase)).collect())
}

/// Run the hooks of `phase` for a tree. `snapshot` is set for post hooks.
pub(crate) fn run(
    repo: &Repository,
    tree: &TreeRecord,
    phase: Phase,
    branch: &str,
    snapshot: Option<&Snapshot>,
) -> Result<()> {
    let hooks = find_hooks(repo, &tree.name, phase)?;
    if hooks.is_empty() {
        return Ok(());
    }
    let config = repo.effective_config(Some(&tree.name))?.hooks;
    if !config.enabled {
        return Ok(());
    }
    let timeout = (config.timeout_secs > 0).then(|| Duration::from_secs(config.timeout_secs));
    let tree_dir = repo.tree_dir_of(tree);
    let mut env: Vec<(&str, String)> = vec![
        ("WT_HOOK", phase.file_name().to_string()),
        ("WT_HOOK_PHASE", phase.file_name().to_string()),
        ("WT_TREE", tree.name.clone()),
        ("WT_TREE_NAME", tree.name.clone()),
        ("WT_TREE_PATH", tree_dir.display().to_string()),
        ("WT_BRANCH", branch.to_string()),
        ("WT_ROOT", repo.root().display().to_string()),
        ("WT_USER", repo.author()),
    ];
    if let Some(s) = snapshot {
        env.push(("WT_SNAPSHOT", s.id.clone()));
        env.push(("WT_SNAPSHOT_ID", s.id.clone()));
    }

    for hook in hooks {
        let shown = display_path(repo.root(), &hook);
        let result = execute(&hook, &tree_dir, &env, timeout);
        let failure = match &result {
            Ok(out) => {
                if out.status.is_some_and(|s| s.success()) {
                    forward(&out.output);
                    continue;
                }
                match out.status {
                    Some(status) => format!("{shown}: {}", describe_status(status)),
                    None => format!(
                        "{shown}: timed out after {}s",
                        timeout.map_or(0, |t| t.as_secs())
                    ),
                }
            }
            Err(e) => format!("{shown}: cannot run: {e}"),
        };
        let output = result.as_ref().map(|o| tail(&o.output)).unwrap_or_default();
        let detail = if output.is_empty() {
            failure
        } else {
            format!("{failure}: {output}")
        };
        match phase {
            Phase::PreSnapshot => {
                return Err(SdkError::Other(format!(
                    "{} hook failed: {detail}",
                    phase.file_name()
                )));
            }
            Phase::PostSnapshot => {
                tracing::warn!(hook = %shown, "post-snapshot hook failed");
                eprintln!("warning: {} hook failed: {detail}", phase.file_name());
            }
        }
    }
    Ok(())
}

/// Path of a hook relative to the worktree root, '/'-separated.
fn display_path(root: &Path, hook: &Path) -> String {
    hook.strip_prefix(root)
        .unwrap_or(hook)
        .to_string_lossy()
        .replace('\\', "/")
}

fn describe_status(status: ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("exit code {code}"),
        None => format!("terminated ({status})"),
    }
}

/// Last [`MAX_QUOTED_OUTPUT`] characters of hook output, trimmed.
fn tail(output: &[u8]) -> String {
    let text = String::from_utf8_lossy(output);
    let text = text.trim();
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= MAX_QUOTED_OUTPUT {
        return text.to_string();
    }
    let rest: String = chars[chars.len() - MAX_QUOTED_OUTPUT..].iter().collect();
    format!("…{rest}")
}

fn forward(output: &[u8]) {
    if !output.is_empty() {
        let mut err = std::io::stderr().lock();
        let _ = err.write_all(output);
        let _ = err.flush();
    }
}

/// What a hook run produced. `status` is `None` when it timed out.
struct Outcome {
    status: Option<ExitStatus>,
    /// stdout followed by stderr.
    output: Vec<u8>,
}

/// The command that runs `hook`.
fn command_for(hook: &Path) -> Command {
    let ext = hook
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default();
    match ext.as_str() {
        "ps1" => {
            let mut c = Command::new("powershell");
            c.args([
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy",
                "Bypass",
                "-File",
            ])
            .arg(hook);
            c
        }
        "" if cfg!(windows) => {
            let mut c = Command::new("sh");
            c.arg(hook);
            c
        }
        _ => Command::new(hook),
    }
}

fn execute(
    hook: &Path,
    cwd: &Path,
    env: &[(&str, String)],
    timeout: Option<Duration>,
) -> std::io::Result<Outcome> {
    let mut cmd = command_for(hook);
    cmd.current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Own process group so a timeout can kill the hook's children too.
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    let stdout = pipe_reader(child.stdout.take());
    let stderr = pipe_reader(child.stderr.take());
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        if timeout.is_some_and(|t| started.elapsed() >= t) {
            kill_tree(&mut child);
            break None;
        }
        std::thread::sleep(POLL_INTERVAL);
    };
    let deadline = Instant::now() + PIPE_GRACE;
    let left = || deadline.saturating_duration_since(Instant::now());
    let mut output = stdout.recv_timeout(left()).unwrap_or_default();
    output.extend(stderr.recv_timeout(left()).unwrap_or_default());
    Ok(Outcome { status, output })
}

/// Read a pipe to the end on a helper thread.
fn pipe_reader<R: Read + Send + 'static>(pipe: Option<R>) -> mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    match pipe {
        Some(mut pipe) => {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                let _ = pipe.read_to_end(&mut buf);
                let _ = tx.send(buf);
            });
        }
        None => {
            let _ = tx.send(Vec::new());
        }
    }
    rx
}

/// How long the process-tree kill may take before the hook process itself
/// is terminated directly.
const KILL_GRACE: Duration = Duration::from_secs(3);

/// Kill a timed-out hook and every process it started (by PID, never by
/// image name), then reap it.
fn kill_tree(child: &mut Child) {
    let pid = child.id().to_string();
    #[cfg(windows)]
    let killer = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(unix)]
    let killer = Command::new("kill")
        .args(["-KILL", "--", &format!("-{pid}")])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(not(any(windows, unix)))]
    let killer: std::io::Result<Child> = Err(std::io::ErrorKind::Unsupported.into());
    let started = Instant::now();
    while started.elapsed() < KILL_GRACE {
        if matches!(child.try_wait(), Ok(Some(_)) | Err(_)) {
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let _ = child.kill();
    let _ = child.wait();
    if let Ok(mut killer) = killer {
        // Reap the helper in the background; it finishes on its own.
        std::thread::spawn(move || {
            let _ = killer.wait();
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_names() {
        assert_eq!(Phase::PreSnapshot.file_name(), "pre-snapshot");
        assert_eq!(Phase::PostSnapshot.file_name(), "post-snapshot");
    }

    #[test]
    fn output_tail_is_bounded() {
        assert_eq!(tail(b"  short\n"), "short");
        let long = "x".repeat(MAX_QUOTED_OUTPUT + 10);
        let t = tail(long.as_bytes());
        assert!(t.starts_with('…'));
        assert_eq!(t.chars().count(), MAX_QUOTED_OUTPUT + 1);
    }

    #[test]
    fn hook_discovery_prefers_platform_extensions() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(hook_in(dir.path(), Phase::PreSnapshot), None);
        std::fs::create_dir(dir.path().join("pre-snapshot.cmd")).unwrap();
        assert_eq!(
            hook_in(dir.path(), Phase::PreSnapshot),
            None,
            "directories are not hooks"
        );
        #[cfg(windows)]
        {
            std::fs::write(dir.path().join("pre-snapshot"), "#!/bin/sh\n").unwrap();
            std::fs::write(dir.path().join("pre-snapshot.bat"), "@echo off\n").unwrap();
            assert_eq!(
                hook_in(dir.path(), Phase::PreSnapshot),
                Some(dir.path().join("pre-snapshot.bat"))
            );
            assert_eq!(hook_in(dir.path(), Phase::PostSnapshot), None);
        }
        #[cfg(unix)]
        {
            let path = dir.path().join("post-snapshot");
            std::fs::write(&path, "#!/bin/sh\n").unwrap();
            assert_eq!(
                hook_in(dir.path(), Phase::PostSnapshot),
                None,
                "not executable"
            );
            crate::repo::fsutil::set_executable(&path, true).unwrap();
            assert_eq!(hook_in(dir.path(), Phase::PostSnapshot), Some(path));
        }
    }

    #[test]
    fn display_paths_are_root_relative() {
        let root = Path::new("/w");
        assert_eq!(
            display_path(root, &root.join(".wt").join("hooks").join("pre-snapshot")),
            ".wt/hooks/pre-snapshot"
        );
    }
}
