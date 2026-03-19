use std::path::Path;

use worktree_protocol::object::blob::Blob;
use worktree_protocol::object::manifest::Manifest;
use worktree_protocol::object::snapshot::Snapshot;

use crate::error::Result;

/// Converts Worktree objects (snapshots, manifests, blobs) into their
/// corresponding Git representations and writes them into a Git repository.
pub struct WorktreeToGitConverter {
    /// The Git repository we are writing objects into.
    repo: git2::Repository,
}

impl WorktreeToGitConverter {
    /// Create a new converter targeting the given Git repository path.
    ///
    /// The path should point to an already-initialised Git repository
    /// (either bare or with a working directory).
    pub fn new(repo_path: &Path) -> Result<Self> {
        let repo = git2::Repository::open(repo_path)?;
        Ok(Self { repo })
    }

    /// Wrap an already-opened `git2::Repository`.
    pub fn from_repo(repo: git2::Repository) -> Self {
        Self { repo }
    }

    /// Return a reference to the underlying `git2::Repository`.
    pub fn repo(&self) -> &git2::Repository {
        &self.repo
    }

    /// Convert a Worktree [`Snapshot`] into a Git commit and write it to the
    /// repository's object database.
    ///
    /// The snapshot's manifest hash is used to locate the corresponding Git
    /// tree object, and parent snapshot IDs are resolved to their Git commit
    /// counterparts via the hash index.
    pub fn convert_snapshot(&self, _snapshot: &Snapshot) -> Result<git2::Oid> {
        todo!("convert Worktree Snapshot → Git commit object")
    }

    /// Convert a Worktree [`Manifest`] into a Git tree object and write it to
    /// the repository's object database.
    ///
    /// Each manifest entry is mapped to a Git tree entry with the appropriate
    /// file mode and object id.
    pub fn convert_manifest(&self, _manifest: &Manifest) -> Result<git2::Oid> {
        todo!("convert Worktree Manifest → Git tree object")
    }

    /// Convert a Worktree [`Blob`] into a Git blob and write it to the
    /// repository's object database.
    ///
    /// Returns the `git2::Oid` of the newly written blob.
    pub fn convert_blob(&self, _blob: &Blob) -> Result<git2::Oid> {
        todo!("convert Worktree Blob → Git blob object")
    }
}
