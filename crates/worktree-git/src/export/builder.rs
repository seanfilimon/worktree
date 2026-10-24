use std::path::Path;

use git2::Repository;
use tracing::info;

use crate::error::Result;

/// Builder for creating a valid Git repository from Worktree data.
///
/// This struct orchestrates the creation of a `.git/` directory structure
/// and populates it with objects converted from Worktree snapshots,
/// manifests, and blobs.
pub struct GitRepoBuilder {
    /// Optional initial branch name (defaults to "main").
    initial_branch: String,
    /// Whether to create a bare repository.
    bare: bool,
}

impl GitRepoBuilder {
    /// Create a new `GitRepoBuilder` with default settings.
    pub fn new() -> Self {
        Self {
            initial_branch: "main".to_string(),
            bare: false,
        }
    }

    /// Set the initial branch name for the new repository.
    pub fn with_initial_branch(mut self, branch: impl Into<String>) -> Self {
        self.initial_branch = branch.into();
        self
    }

    /// Configure whether to create a bare repository.
    pub fn bare(mut self, bare: bool) -> Self {
        self.bare = bare;
        self
    }

    /// Build and initialize a new Git repository at the given path.
    ///
    /// Creates a valid `.git/` directory (or bare repo) using `git2::Repository::init`.
    /// Returns `Ok(())` on success.
    pub fn build(&self, output_path: &Path) -> Result<()> {
        info!(
            path = %output_path.display(),
            branch = %self.initial_branch,
            bare = self.bare,
            "initializing git repository"
        );

        let repo = if self.bare {
            Repository::init_bare(output_path)?
        } else {
            Repository::init(output_path)?
        };

        // Set HEAD to point to the configured initial branch.
        repo.set_head(&format!("refs/heads/{}", self.initial_branch))?;

        info!(
            path = %output_path.display(),
            "git repository initialized successfully"
        );

        Ok(())
    }
}

impl Default for GitRepoBuilder {
    fn default() -> Self {
        Self::new()
    }
}
