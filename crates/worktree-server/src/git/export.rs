use std::path::Path;

use crate::error::ServerError;

/// Controls how a Worktree tree is exported to a Git repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportMode {
    /// Export the complete history — every snapshot becomes a Git commit.
    Full,

    /// Squash all snapshots into a single Git commit.
    Squashed,

    /// Export only the most recent `n` snapshots as Git commits.
    Shallow(usize),

    /// Export only the current state of a single tree (no history at all),
    /// producing a Git repo with one initial commit.
    SingleTree,
}

impl std::fmt::Display for ExportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportMode::Full => write!(f, "full"),
            ExportMode::Squashed => write!(f, "squashed"),
            ExportMode::Shallow(n) => write!(f, "shallow({})", n),
            ExportMode::SingleTree => write!(f, "single-tree"),
        }
    }
}

/// Service responsible for exporting a Worktree tree into a standard Git
/// repository on disk.
///
/// The export process reads snapshots, blobs, and branch metadata from the
/// Worktree storage layer and writes them out as Git objects, refs, and
/// working-tree files using `worktree_git::export`.
pub struct GitExportService {
    _private: (),
}

impl GitExportService {
    /// Create a new `GitExportService`.
    pub fn new() -> Self {
        Self { _private: () }
    }

    /// Export the tree identified by `tree_id` into a Git repository at
    /// `output`, using the given [`ExportMode`] to control how much history
    /// is preserved.
    ///
    /// # Arguments
    ///
    /// * `tree_id` — The identifier of the Worktree tree to export.
    /// * `output`  — Filesystem path where the Git repository will be created.
    /// * `mode`    — Controls the depth / shape of exported history.
    ///
    /// # Errors
    ///
    /// Returns [`ServerError::Git`] if the tree cannot be found, the output
    /// path is not writable, or the underlying git export fails.
    pub fn export(
        &self,
        tree_id: &str,
        output: &Path,
        mode: ExportMode,
    ) -> Result<(), ServerError> {
        let _ = (tree_id, output, mode);
        todo!("read snapshots from storage, convert to git objects, and write to output path")
    }
}

impl Default for GitExportService {
    fn default() -> Self {
        Self::new()
    }
}
