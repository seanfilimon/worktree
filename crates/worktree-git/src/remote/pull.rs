use std::path::{Path, PathBuf};

use tracing::info;

use crate::error::Result;

/// Handles pulling (fetching + merging) from a Git remote into a local repository.
pub struct GitPull {
    /// Path to the local repository.
    repo_path: PathBuf,
}

impl GitPull {
    /// Create a new `GitPull` targeting the repository at the given path.
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
        }
    }

    /// Return the path to the local repository.
    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    /// Pull changes from the specified remote URL and branch into the local repository.
    ///
    /// This fetches the remote branch and fast-forwards (or merges) the local branch
    /// to match the remote state.
    pub fn pull(&self, remote_url: &str, branch: &str) -> Result<()> {
        info!(
            remote_url = remote_url,
            branch = branch,
            repo = %self.repo_path.display(),
            "pulling from remote"
        );
        todo!("implement pull: fetch from remote and merge into local branch")
    }

    /// Fetch from the remote without merging, returning the fetched commit OID.
    pub fn fetch_only(&self, remote_url: &str, branch: &str) -> Result<git2::Oid> {
        info!(
            remote_url = remote_url,
            branch = branch,
            repo = %self.repo_path.display(),
            "fetching from remote (no merge)"
        );
        todo!("implement fetch-only: retrieve remote refs without merging")
    }
}
