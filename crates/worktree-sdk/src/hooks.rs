//! Client hooks: executables under `.wt/hooks/<phase>` (worktree-wide)
//! and `<tree>/.wt-tree/hooks/<phase>` (per tree), run around snapshots
//! (docs/ENGINEERING_PLAN.md §13.9), pushes, merges and branch switches.
//!
//! * Phases: `pre-snapshot`, `post-snapshot`, `pre-push`, `post-push`,
//!   `pre-merge`, `post-merge` and `post-switch` (see [`Phase`]).
//! * Order: `.wt/hooks` first, then the hooks of every tree enclosing the
//!   tree (outermost first), then the tree's own hooks.
//! * Discovery: on Unix the file `<phase>` must be executable. On Windows
//!   `<phase>.exe`, `<phase>.cmd`, `<phase>.bat`, `<phase>.ps1` (run with
//!   PowerShell) and an extensionless `<phase>` script (run with `sh`, as
//!   Git for Windows does) are recognized; the first one found per
//!   directory runs.
//! * Environment (every phase): `WT_HOOK` (= `WT_HOOK_PHASE`, the phase
//!   name), `WT_TREE` (= `WT_TREE_NAME`), `WT_TREE_PATH` (absolute tree
//!   directory), `WT_BRANCH`, `WT_ROOT` (worktree root) and `WT_USER`
//!   (snapshot author). Snapshot post hooks also get `WT_SNAPSHOT`
//!   (= `WT_SNAPSHOT_ID`); the push, merge and switch phases get the
//!   variables listed on their [`Phase`] variant. The working directory is
//!   the tree root. stdin is empty except for the push hooks (the pushed
//!   snapshot ids) and `post-merge` (the conflicted paths), one per line.
//! * `[hooks] enabled` turns hooks off, `[hooks] timeout_secs` (default 60,
//!   0 = no limit) kills a hook (and its child processes) that runs longer.
//! * A failing or timed-out `pre-*` hook aborts the operation before it
//!   changes anything, with `SdkError::Other("<phase> hook failed: …")`
//!   including the tail of its output. `post-*` hook failures are reported
//!   on stderr only; the operation is kept.
//! * Hook output is forwarded to stderr (never stdout, which may carry
//!   `--json` output).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::error::{Result, SdkError};
use crate::model::{MergeOutcome, Snapshot};
use crate::repo::{Repository, TreeRecord, TREE_MARKER_DIR};

/// Directory holding hooks inside `.wt/` and `.wt-tree/`.
pub const HOOKS_DIR: &str = "hooks";

/// Characters of hook output quoted in an abort message.
const MAX_QUOTED_OUTPUT: usize = 2000;
/// How long to wait for a hook's output pipes after it exited (a background
/// grandchild may keep them open).
const PIPE_GRACE: Duration = Duration::from_secs(2);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// When a hook runs. `pre-*` hooks can veto the operation (non-zero exit or
/// timeout); `post-*` hook failures are reported, not fatal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Before the working state is read; a non-zero exit aborts the snapshot.
    PreSnapshot,
    /// After the snapshot is written; failures are reported, not fatal.
    PostSnapshot,
    /// Before a push uploads anything (once the server's branch tip is known
    /// and the push is found to be possible); a non-zero exit aborts the
    /// push. `WT_BRANCH` is the pushed branch; also `WT_REMOTE`,
    /// `WT_REMOTE_URL`, `WT_PUSH_OLD_TIP` (server tip, empty for a new
    /// branch), `WT_PUSH_NEW_TIP`, `WT_PUSH_FORCE` (`1`/`0`) and
    /// `WT_PUSH_SNAPSHOTS` (count); stdin lists the ids of the snapshots
    /// being pushed, oldest first. Not run when the server branch is
    /// already at the local tip.
    PrePush,
    /// After the server accepted a push (same environment and stdin as
    /// `pre-push`); failures are reported, not fatal.
    PostPush,
    /// Before a merge (`wt merge`, and the integration step of `wt pull`
    /// and `wt sync`) touches the working directory or the branch; a
    /// non-zero exit aborts it. Not run when there is nothing to merge.
    /// `WT_BRANCH` is the branch merged into; also `WT_MERGE_SOURCE` (as
    /// given; `<remote>/<branch>` for a pull), `WT_MERGE_SOURCE_TIP`,
    /// `WT_MERGE_OLD_TIP` (empty for an empty branch), `WT_MERGE_KIND`
    /// (`fast-forward` or `merge`) and, for a pull or sync, `WT_REMOTE`.
    PreMerge,
    /// After a merge, also when it stopped with conflicts: the `pre-merge`
    /// environment plus `WT_MERGE_RESULT` (`fast-forward`, `merged` or
    /// `conflicts`), `WT_MERGE_CONFLICTS` (number of conflicted paths) and
    /// `WT_MERGE_NEW_TIP` (the branch tip afterwards); stdin lists the
    /// conflicted paths. Failures are reported, not fatal.
    PostMerge,
    /// After a branch switch changed the current branch (or rewrote the
    /// working directory with `--force`): `WT_BRANCH` is the new branch;
    /// also `WT_SWITCH_FROM`, `WT_SWITCH_TO`, `WT_SWITCH_OLD_TIP`,
    /// `WT_SWITCH_NEW_TIP` (empty for an empty branch) and
    /// `WT_SWITCH_CREATED` (`1` with `-c`, else `0`). Failures are
    /// reported, not fatal.
    PostSwitch,
}

impl Phase {
    /// Every phase, in documentation order.
    pub const ALL: [Phase; 7] = [
        Phase::PreSnapshot,
        Phase::PostSnapshot,
        Phase::PrePush,
        Phase::PostPush,
        Phase::PreMerge,
        Phase::PostMerge,
        Phase::PostSwitch,
    ];

    /// File name of the hook (`pre-snapshot`, `post-push`, ...).
    pub fn file_name(self) -> &'static str {
        match self {
            Phase::PreSnapshot => "pre-snapshot",
            Phase::PostSnapshot => "post-snapshot",
            Phase::PrePush => "pre-push",
            Phase::PostPush => "post-push",
            Phase::PreMerge => "pre-merge",
            Phase::PostMerge => "post-merge",
            Phase::PostSwitch => "post-switch",
        }
    }

    /// The phase whose hook file is called `name` (without extension).
    pub fn from_file_name(name: &str) -> Option<Phase> {
        Phase::ALL.into_iter().find(|p| p.file_name() == name)
    }

    /// Whether a failing hook of this phase aborts the operation.
    pub fn is_pre(self) -> bool {
        matches!(self, Phase::PreSnapshot | Phase::PrePush | Phase::PreMerge)
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

/// Run the snapshot hooks of `phase` for a tree. `snapshot` is set for
/// post hooks.
pub(crate) fn run(
    repo: &Repository,
    tree: &TreeRecord,
    phase: Phase,
    branch: &str,
    snapshot: Option<&Snapshot>,
) -> Result<()> {
    let mut env = Vec::new();
    if let Some(s) = snapshot {
        env.push(("WT_SNAPSHOT", s.id.clone()));
        env.push(("WT_SNAPSHOT_ID", s.id.clone()));
    }
    run_with(repo, tree, phase, branch, &env, &[])
}

/// Run the hooks of a `post-*` phase. Nothing is returned: a hook that
/// fails, or hooks that cannot even be looked up, are reported on stderr
/// because the operation they follow is already done.
fn run_post(
    repo: &Repository,
    tree: &TreeRecord,
    phase: Phase,
    branch: &str,
    extra_env: &[(&'static str, String)],
    stdin: &[u8],
) {
    debug_assert!(!phase.is_pre());
    if let Err(e) = run_with(repo, tree, phase, branch, extra_env, stdin) {
        tracing::warn!(phase = phase.file_name(), error = %e, "hooks could not run");
        eprintln!("warning: {} hooks could not run: {e}", phase.file_name());
    }
}

/// `"1"` / `"0"` for boolean hook variables.
fn flag(value: bool) -> String {
    if value { "1" } else { "0" }.to_string()
}

/// One id or path per line (hook stdin).
fn lines<'a>(items: impl IntoIterator<Item = &'a str>) -> Vec<u8> {
    let mut out = Vec::new();
    for item in items {
        out.extend_from_slice(item.as_bytes());
        out.push(b'\n');
    }
    out
}

/// Context of the `pre-push` / `post-push` hooks of one branch push.
pub(crate) struct PushHook<'a> {
    pub(crate) tree: &'a TreeRecord,
    pub(crate) remote: &'a str,
    /// Normalized server URL of the remote.
    pub(crate) remote_url: &'a str,
    pub(crate) branch: &'a str,
    /// Server tip before the push (`None` for a new branch).
    pub(crate) old_tip: Option<&'a str>,
    pub(crate) new_tip: &'a str,
    pub(crate) forced: bool,
    /// Ids of the snapshots sent, oldest first.
    pub(crate) snapshots: &'a [String],
}

impl PushHook<'_> {
    fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("WT_REMOTE", self.remote.to_string()),
            ("WT_REMOTE_URL", self.remote_url.to_string()),
            (
                "WT_PUSH_OLD_TIP",
                self.old_tip.unwrap_or_default().to_string(),
            ),
            ("WT_PUSH_NEW_TIP", self.new_tip.to_string()),
            ("WT_PUSH_FORCE", flag(self.forced)),
            ("WT_PUSH_SNAPSHOTS", self.snapshots.len().to_string()),
        ]
    }

    fn stdin(&self) -> Vec<u8> {
        lines(self.snapshots.iter().map(String::as_str))
    }

    /// Run `pre-push`; an error aborts the push.
    pub(crate) fn pre(&self, repo: &Repository) -> Result<()> {
        run_with(
            repo,
            self.tree,
            Phase::PrePush,
            self.branch,
            &self.env(),
            &self.stdin(),
        )
    }

    /// Run `post-push`; failures are reported only.
    pub(crate) fn post(&self, repo: &Repository) {
        run_post(
            repo,
            self.tree,
            Phase::PostPush,
            self.branch,
            &self.env(),
            &self.stdin(),
        );
    }
}

/// Where a merge's source comes from when it integrates a pull or sync.
#[derive(Debug, Clone, Copy)]
pub(crate) struct MergeVia<'a> {
    pub(crate) remote: &'a str,
    /// `<remote>/<branch>`, shown as `WT_MERGE_SOURCE`.
    pub(crate) source: &'a str,
}

/// Context of the `pre-merge` / `post-merge` hooks of one merge.
#[derive(Debug, Clone)]
pub(crate) struct MergeHook {
    pub(crate) tree: TreeRecord,
    /// The branch merged into.
    pub(crate) branch: String,
    /// The source as given (`<remote>/<branch>` for a pull).
    pub(crate) source: String,
    pub(crate) source_tip: String,
    pub(crate) old_tip: Option<String>,
    pub(crate) fast_forward: bool,
    /// The remote, when the merge integrates a pull or sync.
    pub(crate) remote: Option<String>,
}

impl MergeHook {
    fn env(&self) -> Vec<(&'static str, String)> {
        let kind = if self.fast_forward {
            "fast-forward"
        } else {
            "merge"
        };
        let mut env = vec![
            ("WT_MERGE_SOURCE", self.source.clone()),
            ("WT_MERGE_SOURCE_TIP", self.source_tip.clone()),
            ("WT_MERGE_OLD_TIP", self.old_tip.clone().unwrap_or_default()),
            ("WT_MERGE_KIND", kind.to_string()),
        ];
        if let Some(remote) = &self.remote {
            env.push(("WT_REMOTE", remote.clone()));
        }
        env
    }

    /// Run `pre-merge`; an error aborts the merge.
    pub(crate) fn pre(&self, repo: &Repository) -> Result<()> {
        run_with(
            repo,
            &self.tree,
            Phase::PreMerge,
            &self.branch,
            &self.env(),
            &[],
        )
    }

    /// Run `post-merge` for `outcome`; failures are reported only.
    pub(crate) fn post(&self, repo: &Repository, outcome: &MergeOutcome) {
        let (result, new_tip, conflicts): (&str, Option<&str>, Vec<&str>) = match outcome {
            MergeOutcome::FastForward { to } => ("fast-forward", Some(to), Vec::new()),
            MergeOutcome::Merged { snapshot } => ("merged", Some(&snapshot.id), Vec::new()),
            MergeOutcome::Conflicts { conflicts } => (
                "conflicts",
                self.old_tip.as_deref(),
                conflicts.iter().map(|c| c.path.as_str()).collect(),
            ),
            // Nothing was merged: no hook.
            MergeOutcome::UpToDate => return,
        };
        let mut env = self.env();
        env.push(("WT_MERGE_RESULT", result.to_string()));
        env.push(("WT_MERGE_CONFLICTS", conflicts.len().to_string()));
        env.push(("WT_MERGE_NEW_TIP", new_tip.unwrap_or_default().to_string()));
        run_post(
            repo,
            &self.tree,
            Phase::PostMerge,
            &self.branch,
            &env,
            &lines(conflicts),
        );
    }
}

/// Context of the `post-switch` hook of one branch switch.
#[derive(Debug, Clone)]
pub(crate) struct SwitchHook {
    pub(crate) tree: TreeRecord,
    pub(crate) from: String,
    pub(crate) to: String,
    pub(crate) old_tip: Option<String>,
    pub(crate) new_tip: Option<String>,
    pub(crate) created: bool,
}

impl SwitchHook {
    /// Run `post-switch`; failures are reported only.
    pub(crate) fn post(&self, repo: &Repository) {
        let env = vec![
            ("WT_SWITCH_FROM", self.from.clone()),
            ("WT_SWITCH_TO", self.to.clone()),
            (
                "WT_SWITCH_OLD_TIP",
                self.old_tip.clone().unwrap_or_default(),
            ),
            (
                "WT_SWITCH_NEW_TIP",
                self.new_tip.clone().unwrap_or_default(),
            ),
            ("WT_SWITCH_CREATED", flag(self.created)),
        ];
        run_post(repo, &self.tree, Phase::PostSwitch, &self.to, &env, &[]);
    }
}

/// Run the hooks of `phase` for a tree with phase-specific variables
/// `extra_env` and `stdin`. A failing `pre-*` hook is an error; `post-*`
/// failures are printed.
fn run_with(
    repo: &Repository,
    tree: &TreeRecord,
    phase: Phase,
    branch: &str,
    extra_env: &[(&'static str, String)],
    stdin: &[u8],
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
    env.extend(extra_env.iter().cloned());

    for hook in hooks {
        let shown = display_path(repo.root(), &hook);
        let result = execute(&hook, &tree_dir, &env, stdin, timeout);
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
        if phase.is_pre() {
            return Err(SdkError::Other(format!(
                "{} hook failed: {detail}",
                phase.file_name()
            )));
        }
        tracing::warn!(hook = %shown, phase = phase.file_name(), "post hook failed");
        eprintln!("warning: {} hook failed: {detail}", phase.file_name());
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
    stdin: &[u8],
    timeout: Option<Duration>,
) -> std::io::Result<Outcome> {
    let mut cmd = command_for(hook);
    cmd.current_dir(cwd)
        .stdin(if stdin.is_empty() {
            Stdio::null()
        } else {
            Stdio::piped()
        })
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
    if let Some(mut pipe) = child.stdin.take() {
        // On a helper thread: a hook that never reads its stdin must not
        // block us once the pipe buffer is full. Write errors (the hook
        // exited without reading everything) are expected and ignored.
        let input = stdin.to_vec();
        std::thread::spawn(move || {
            let _ = pipe.write_all(&input);
        });
    }
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
        let names: Vec<&str> = Phase::ALL.iter().map(|p| p.file_name()).collect();
        assert_eq!(
            names,
            [
                "pre-snapshot",
                "post-snapshot",
                "pre-push",
                "post-push",
                "pre-merge",
                "post-merge",
                "post-switch"
            ]
        );
        for phase in Phase::ALL {
            assert_eq!(Phase::from_file_name(phase.file_name()), Some(phase));
            assert_eq!(phase.is_pre(), phase.file_name().starts_with("pre-"));
        }
        assert_eq!(Phase::from_file_name("pre-commit"), None);
        assert_eq!(Phase::from_file_name("pre-push.cmd"), None);
    }

    #[test]
    fn hook_stdin_is_one_item_per_line() {
        assert_eq!(lines(["a", "b"]), b"a\nb\n");
        assert!(lines(std::iter::empty()).is_empty());
        assert_eq!(flag(true), "1");
        assert_eq!(flag(false), "0");
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
