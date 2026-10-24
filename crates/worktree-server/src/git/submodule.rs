use std::path::Path;

use crate::error::ServerError;

/// Service responsible for detecting and converting Git submodules into
/// Worktree nested trees.
///
/// When importing a Git repository that contains submodules, this service
/// reads the `.gitmodules` file, resolves each submodule's path and URL,
/// and converts them into first-class Worktree child trees linked to the
/// parent tree.
pub struct SubmoduleService {
    _private: (),
}

impl SubmoduleService {
    /// Create a new `SubmoduleService`.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Scan the Git repository at `repo_path` for submodules and convert each
    /// one into a Worktree nested tree.
    ///
    /// The method reads `.gitmodules` to discover submodule definitions, then
    /// for each submodule:
    /// 1. Resolves the submodule path relative to `repo_path`.
    /// 2. Clones or reads the submodule content.
    /// 3. Creates a corresponding Worktree child tree.
    /// 4. Links the child tree to the parent tree.
    ///
    /// # Returns
    ///
    /// A `Vec<String>` containing the names of all successfully converted
    /// submodules.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Git`] if the repository path is invalid, the
    /// `.gitmodules` file cannot be parsed, or any submodule conversion fails.
    pub fn convert_submodules(&self, repo_path: &Path) -> Result<Vec<String>, ServerError> {
        tracing::info!(
            repo_path = %repo_path.display(),
            "scanning for git submodules to convert"
        );

        let gitmodules_path = repo_path.join(".gitmodules");
        if !gitmodules_path.exists() {
            tracing::debug!(
                repo_path = %repo_path.display(),
                "no .gitmodules file found — nothing to convert"
            );
            return Ok(Vec::new());
        }

        todo!("parse .gitmodules, resolve submodule paths, and convert each into a worktree child tree")
    }
}

impl Default for SubmoduleService {
    fn default() -> Self {
        Self::new()
    }
}
