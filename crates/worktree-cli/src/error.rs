//! CLI error type: a message, an optional hint, and an exit code class.

use std::fmt;
use std::io::Write;

use colored::Colorize;
use worktree_git::error::GitCompatError;
use worktree_sdk::SdkError;

pub type Result<T> = std::result::Result<T, CliError>;

/// Which exit code an error maps to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    /// Exit code 1: the command failed.
    Failure,
    /// Exit code 2: the command line was invalid.
    Usage,
    /// Exit code 3: a merge stopped with conflicts.
    Conflicts,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    kind: ErrorKind,
    message: String,
    hint: Option<String>,
}

impl CliError {
    pub fn failure(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Failure,
            message: message.into(),
            hint: None,
        }
    }

    pub fn usage(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Usage,
            message: message.into(),
            hint: None,
        }
    }

    pub fn conflicts(message: impl Into<String>) -> Self {
        Self {
            kind: ErrorKind::Conflicts,
            message: message.into(),
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn exit_code(&self) -> u8 {
        match self.kind {
            ErrorKind::Failure => 1,
            ErrorKind::Usage => 2,
            ErrorKind::Conflicts => 3,
        }
    }

    /// Print `error: ...` (and `hint: ...`) to stderr.
    pub fn report(&self) {
        let mut stderr = std::io::stderr().lock();
        // Nothing sensible can be done if stderr itself is gone.
        let _ = writeln!(stderr, "{} {}", "error:".red().bold(), self.message);
        if let Some(hint) = &self.hint {
            let _ = writeln!(stderr, "{} {}", "hint:".yellow().bold(), hint);
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CliError {}

impl From<SdkError> for CliError {
    fn from(err: SdkError) -> Self {
        let hint = sdk_hint(&err);
        let kind = match err {
            SdkError::MergeConflict(_) => ErrorKind::Conflicts,
            _ => ErrorKind::Failure,
        };
        Self {
            kind,
            message: err.to_string(),
            hint,
        }
    }
}

fn sdk_hint(err: &SdkError) -> Option<String> {
    let hint = match err {
        SdkError::NotAWorktree => "run `wt init` to create a worktree here".into(),
        SdkError::AlreadyInitialized => "use `wt status` to inspect the existing worktree".into(),
        SdkError::TreeNotFound(_) => "list trees with `wt tree list`".into(),
        SdkError::BranchNotFound(_) => "list branches with `wt branch --all`".into(),
        SdkError::SnapshotNotFound(_) | SdkError::InvalidRevision(_) => {
            "revisions are HEAD, HEAD~N, a branch, origin/<branch>, a tag, or a snapshot id prefix"
                .into()
        }
        SdkError::AmbiguousId(_) => "use more characters of the snapshot id".into(),
        SdkError::NoChanges => "use --allow-empty to create a snapshot anyway".into(),
        SdkError::DirtyWorkingTree(_) => {
            "snapshot your changes with `wt snapshot -m <message>` or use --force to discard them"
                .into()
        }
        SdkError::MergeConflict(_) => {
            "resolve the conflict markers, then run `wt merge --continue` (or `wt merge --abort`)"
                .into()
        }
        SdkError::SecretsDetected(_) => {
            "remove the secrets, mark the line with `wt:allow-secret`, or use --no-scan".into()
        }
        SdkError::TagExists(_) => "delete it first with `wt tag delete <name>`".into(),
        SdkError::TagNotFound(_) => "list tags with `wt tag list`".into(),
        SdkError::RemoteNotConfigured(_) => {
            "add one with `wt remote add <name> <url> --tenant <tenant>`".into()
        }
        SdkError::NotLoggedIn(server) => format!("run `wt login {server}`"),
        SdkError::PushRejected(_) => {
            "run `wt pull` to integrate the remote changes, then push again".into()
        }
        SdkError::DaemonNotRunning => "start it with `wt daemon start`".into(),
        // tenant-lifecycle (§16.4)
        SdkError::TenantMoved { new, .. } => format!(
            "use the new slug; inside a worktree, update the remote with `wt tenant switch {new}`"
        ),
        _ => return None,
    };
    Some(hint)
}

impl From<GitCompatError> for CliError {
    fn from(err: GitCompatError) -> Self {
        Self::failure(err.to_string())
    }
}

impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::failure(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_follow_the_contract() {
        assert_eq!(CliError::failure("x").exit_code(), 1);
        assert_eq!(CliError::usage("x").exit_code(), 2);
        assert_eq!(CliError::conflicts("x").exit_code(), 3);
    }

    #[test]
    fn merge_conflict_maps_to_exit_code_3() {
        let err = CliError::from(SdkError::MergeConflict("a.txt".into()));
        assert_eq!(err.kind, ErrorKind::Conflicts);
        assert_eq!(err.exit_code(), 3);
        assert!(err
            .hint
            .as_deref()
            .is_some_and(|h| h.contains("wt merge --continue")));
    }

    #[test]
    fn helpful_hints_for_common_errors() {
        let cases = [
            (SdkError::NotAWorktree, "wt init"),
            (SdkError::DirtyWorkingTree(2), "--force"),
            (
                SdkError::NotLoggedIn("http://localhost:7420".into()),
                "wt login http://localhost:7420",
            ),
            (SdkError::NoChanges, "--allow-empty"),
            (SdkError::DaemonNotRunning, "wt daemon start"),
            (SdkError::PushRejected("diverged".into()), "wt pull"),
            (
                SdkError::RemoteNotConfigured("origin".into()),
                "wt remote add",
            ),
            (SdkError::SecretsDetected("a.env".into()), "--no-scan"),
            (
                SdkError::TenantMoved {
                    old: "acme".into(),
                    new: "acme-inc".into(),
                },
                "wt tenant switch acme-inc",
            ),
        ];
        for (err, needle) in cases {
            let message = err.to_string();
            let cli = CliError::from(err);
            assert_eq!(cli.exit_code(), 1, "{message}");
            assert_eq!(cli.to_string(), message);
            let hint = cli.hint.clone().unwrap_or_default();
            assert!(
                hint.contains(needle),
                "hint {hint:?} should mention {needle:?}"
            );
        }
    }

    #[test]
    fn errors_without_hints() {
        let err = CliError::from(SdkError::Other("boom".into()));
        assert_eq!(err.hint, None);
        assert_eq!(err.to_string(), "boom");
    }

    #[test]
    fn io_and_git_errors_are_failures() {
        let io = CliError::from(std::io::Error::other("disk full"));
        assert_eq!(io.exit_code(), 1);
        let git = CliError::from(GitCompatError::ImportError("bad".into()));
        assert_eq!(git.exit_code(), 1);
        assert!(git.to_string().contains("bad"));
    }

    #[test]
    fn with_hint_overrides() {
        let err = CliError::usage("bad spec").with_hint("use TENANT/TREE");
        assert_eq!(err.hint.as_deref(), Some("use TENANT/TREE"));
        assert_eq!(err.exit_code(), 2);
    }
}
