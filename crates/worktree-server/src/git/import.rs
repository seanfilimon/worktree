use std::path::Path;

use crate::error::ServerError;

/// Service that orchestrates importing a Git repository into the Worktree
/// version control system.
///
/// The import process reads the Git history (commits, trees, blobs) from the
/// source repository and converts them into Worktree objects (snapshots,
/// manifests, content-addressed blobs).
pub struct GitImportService {
    /// Optional branch filter — when set, only branches matching these names
    /// are imported. An empty vec means "import all branches".
    pub branch_filter: Vec<String>,

    /// Whether to import the full history or only the latest state.
    pub full_history: bool,
}

impl GitImportService {
    /// Create a new `GitImportService` that imports all branches with full history.
    pub fn new() -> Self {
        Self {
            branch_filter: Vec::new(),
            full_history: true,
        }
    }

    /// Create a new `GitImportService` that only imports the specified branches.
    pub fn with_branches(branches: Vec<String>) -> Self {
        Self {
            branch_filter: branches,
            full_history: true,
        }
    }

    /// Create a new `GitImportService` that only imports the latest state
    /// (no history).
    pub fn shallow() -> Self {
        Self {
            branch_filter: Vec::new(),
            full_history: false,
        }
    }

    /// Import a Git repository from the given source path or URL into the
    /// Worktree system.
    ///
    /// This method orchestrates the full import pipeline:
    /// 1. Validate the source is a valid Git repository.
    /// 2. Enumerate branches (optionally filtered).
    /// 3. Walk commit history and convert objects.
    /// 4. Store converted objects in the Worktree storage backend.
    ///
    /// Uses [`worktree_git::import::repo::GitRepo`] for the low-level Git
    /// reading operations.
    pub fn import(&self, source: &str) -> Result<(), ServerError> {
        tracing::info!(
            source = source,
            full_history = self.full_history,
            branch_count = self.branch_filter.len(),
            "starting git import"
        );

        let source_path = Path::new(source);

        // Delegate to the worktree-git crate to open and validate the repository.
        let git_repo = worktree_git::import::repo::GitRepo::open(source_path).map_err(|e| {
            ServerError::Git(format!("failed to open git repo at '{}': {}", source, e))
        })?;

        // Enumerate branches available in the source repository.
        let branches = git_repo.branches().map_err(|e| {
            ServerError::Git(format!("failed to list branches for '{}': {}", source, e))
        })?;

        let branches_to_import: Vec<&String> = if self.branch_filter.is_empty() {
            branches.iter().collect()
        } else {
            branches
                .iter()
                .filter(|b| self.branch_filter.contains(b))
                .collect()
        };

        tracing::info!(
            branch_count = branches_to_import.len(),
            "discovered branches to import"
        );

        todo!("walk commit history for each branch, convert git objects into worktree snapshots, manifests, and blobs")
    }
}

impl Default for GitImportService {
    fn default() -> Self {
        Self::new()
    }
}
