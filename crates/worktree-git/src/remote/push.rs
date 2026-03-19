use std::path::{Path, PathBuf};

use tracing::info;

use crate::error::Result;
use crate::remote::auth::GitAuth;

/// Handles pushing Worktree-exported branches to a Git remote.
#[derive(Debug)]
pub struct GitPush {
    /// Path to the local Git repository used for pushing.
    repo_path: PathBuf,
    /// Optional authentication configuration.
    auth: Option<GitAuth>,
}

impl GitPush {
    /// Create a new `GitPush` targeting the given local repository path.
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        Self {
            repo_path: repo_path.into(),
            auth: None,
        }
    }

    /// Attach authentication credentials to this push operation.
    pub fn with_auth(mut self, auth: GitAuth) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Return the local repository path.
    pub fn repo_path(&self) -> &Path {
        &self.repo_path
    }

    /// Push the given branch to the specified remote URL.
    ///
    /// This opens the local repository, locates the named remote (or creates
    /// a transient one for the given URL), and pushes the refspec for `branch`.
    ///
    /// # Errors
    ///
    /// Returns an error if the repository cannot be opened, the remote is
    /// unreachable, or authentication fails.
    pub fn push(&self, remote_url: &str, branch: &str) -> Result<()> {
        info!(
            repo = %self.repo_path.display(),
            remote_url,
            branch,
            "pushing branch to remote"
        );
        todo!("implement push: open repo, resolve remote, push refspec")
    }

    /// Push all local branches to the specified remote URL.
    ///
    /// # Errors
    ///
    /// Returns an error if any branch fails to push.
    pub fn push_all(&self, remote_url: &str) -> Result<()> {
        info!(
            repo = %self.repo_path.display(),
            remote_url,
            "pushing all branches to remote"
        );
        todo!("implement push_all: enumerate local branches and push each")
    }
}
