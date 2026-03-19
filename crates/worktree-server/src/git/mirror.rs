use crate::error::ServerError;

/// Service that maintains a continuous mirror between a Worktree tree and a
/// Git remote branch.
///
/// When mirroring is active, every new snapshot created on the Worktree side
/// is automatically exported as a Git commit and pushed to the configured
/// remote branch. Conversely, new commits appearing on the remote branch
/// can be pulled and converted into Worktree snapshots (bidirectional mode
/// is planned for the future).
pub struct GitMirrorService {
    /// Active mirrors, keyed by `(tree_id, remote, branch)`.
    active_mirrors: Vec<MirrorEntry>,
}

/// Internal bookkeeping for a single active mirror.
#[derive(Debug, Clone)]
struct MirrorEntry {
    /// The Worktree tree being mirrored.
    tree_id: String,
    /// The Git remote name or URL.
    remote: String,
    /// The Git branch to mirror to/from.
    branch: String,
}

impl GitMirrorService {
    /// Create a new `GitMirrorService` with no active mirrors.
    pub fn new() -> Self {
        Self {
            active_mirrors: Vec::new(),
        }
    }

    /// Return the number of currently active mirrors.
    pub fn active_count(&self) -> usize {
        self.active_mirrors.len()
    }

    /// Check whether a mirror is already active for the given tree, remote,
    /// and branch combination.
    pub fn is_mirroring(&self, tree_id: &str, remote: &str, branch: &str) -> bool {
        self.active_mirrors.iter().any(|entry| {
            entry.tree_id == tree_id && entry.remote == remote && entry.branch == branch
        })
    }

    /// Start mirroring a Worktree tree to a Git remote branch.
    ///
    /// This sets up a background task that watches for new snapshots on the
    /// given tree and automatically pushes them as Git commits to the
    /// specified remote and branch.
    ///
    /// # Arguments
    ///
    /// * `tree_id` — The identifier of the Worktree tree to mirror.
    /// * `remote`  — The Git remote name or URL to push to.
    /// * `branch`  — The Git branch name to mirror (e.g. `"main"`).
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Git`] if a mirror is already active for the
    /// same `(tree_id, remote, branch)` triple, or if the initial setup
    /// (validation, remote reachability check) fails.
    pub fn start_mirror(
        &mut self,
        tree_id: &str,
        remote: &str,
        branch: &str,
    ) -> Result<(), ServerError> {
        if self.is_mirroring(tree_id, remote, branch) {
            return Err(ServerError::Git(format!(
                "mirror already active for tree '{}' -> {}:{}",
                tree_id, remote, branch
            )));
        }

        tracing::info!(
            tree_id = %tree_id,
            remote = %remote,
            branch = %branch,
            "starting git mirror"
        );

        self.active_mirrors.push(MirrorEntry {
            tree_id: tree_id.to_string(),
            remote: remote.to_string(),
            branch: branch.to_string(),
        });

        todo!("spawn background task to watch for new snapshots and push as git commits")
    }

    /// Stop an active mirror for the given tree, remote, and branch.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Git`] if no matching mirror is currently active.
    pub fn stop_mirror(
        &mut self,
        tree_id: &str,
        remote: &str,
        branch: &str,
    ) -> Result<(), ServerError> {
        let idx = self
            .active_mirrors
            .iter()
            .position(|entry| {
                entry.tree_id == tree_id && entry.remote == remote && entry.branch == branch
            })
            .ok_or_else(|| {
                ServerError::Git(format!(
                    "no active mirror for tree '{}' -> {}:{}",
                    tree_id, remote, branch
                ))
            })?;

        self.active_mirrors.remove(idx);

        tracing::info!(
            tree_id = %tree_id,
            remote = %remote,
            branch = %branch,
            "stopped git mirror"
        );

        todo!("cancel the background mirror task and wait for graceful shutdown")
    }
}

impl Default for GitMirrorService {
    fn default() -> Self {
        Self::new()
    }
}
