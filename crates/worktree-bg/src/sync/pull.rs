use crate::error::BgError;

/// Represents a pull (fetch + integrate) operation from a remote Worktree server.
///
/// A pull downloads new objects (snapshots, blobs, manifests) from the remote
/// and integrates them into the local tree, fast-forwarding or merging as
/// necessary.
pub struct PullOperation {
    /// The remote address or name to pull from (e.g. `"origin"` or a URL).
    pub remote: String,

    /// Optional branch name to pull. If `None`, pulls the current/default branch.
    pub branch: Option<String>,

    /// Whether to attempt a fast-forward only (fail if diverged).
    pub fast_forward_only: bool,
}

impl PullOperation {
    /// Create a new `PullOperation` targeting the given remote.
    pub fn new(remote: impl Into<String>) -> Self {
        Self {
            remote: remote.into(),
            branch: None,
            fast_forward_only: false,
        }
    }

    /// Set the branch to pull.
    pub fn with_branch(mut self, branch: impl Into<String>) -> Self {
        self.branch = Some(branch.into());
        self
    }

    /// Restrict the pull to fast-forward merges only.
    pub fn fast_forward_only(mut self) -> Self {
        self.fast_forward_only = true;
        self
    }

    /// Execute the pull operation.
    ///
    /// This will:
    /// 1. Connect to the remote using the configured transport.
    /// 2. Negotiate which objects are missing locally.
    /// 3. Download missing objects (snapshots, blobs, deltas).
    /// 4. Integrate the remote branch tip into the local branch.
    ///
    /// Returns `Ok(())` on success, or a `BgError` if any step fails.
    pub async fn execute(&self) -> Result<(), BgError> {
        tracing::info!(
            remote = %self.remote,
            branch = ?self.branch,
            fast_forward_only = self.fast_forward_only,
            "executing pull operation"
        );

        todo!("connect to remote, negotiate missing objects, download, and integrate")
    }
}
