use crate::error::BgError;
use crate::sync::transport::Transport;

/// Represents a push operation that transfers local snapshots and objects
/// to a remote Worktree server or storage backend.
///
/// A `PushOperation` encapsulates the source tree, target remote, and
/// transport configuration needed to perform the transfer.
pub struct PushOperation {
    /// The tree identifier to push from.
    pub tree_id: String,

    /// The branch name to push.
    pub branch: String,

    /// The remote name or URL to push to.
    pub remote: String,

    /// The transport to use for the push.
    pub transport: Transport,

    /// Whether to force-push, overwriting the remote branch tip even if
    /// it has diverged from the local branch.
    pub force: bool,
}

impl PushOperation {
    /// Create a new `PushOperation` targeting the given remote and branch.
    pub fn new(
        tree_id: impl Into<String>,
        branch: impl Into<String>,
        remote: impl Into<String>,
        transport: Transport,
    ) -> Self {
        Self {
            tree_id: tree_id.into(),
            branch: branch.into(),
            remote: remote.into(),
            transport,
            force: false,
        }
    }

    /// Set whether this push should be a force-push.
    pub fn with_force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    /// Execute the push operation, transferring objects and updating the
    /// remote branch pointer.
    ///
    /// # Errors
    ///
    /// Returns a `BgError` if the transport connection fails, the remote
    /// rejects the push (e.g. non-fast-forward without `force`), or any
    /// objects fail to transfer.
    pub async fn execute(&self) -> Result<(), BgError> {
        tracing::info!(
            tree_id = %self.tree_id,
            branch = %self.branch,
            remote = %self.remote,
            force = self.force,
            "Executing push operation"
        );

        todo!("connect via transport, negotiate missing objects, transfer data, update remote ref")
    }
}
