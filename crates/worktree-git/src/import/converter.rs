use worktree_protocol::object::blob::Blob;
use worktree_protocol::object::manifest::Manifest;
use worktree_protocol::object::snapshot::Snapshot;
use worktree_protocol::core::id::TreeId;

use crate::error::Result;
use crate::import::repo::GitRepo;

/// Converts Git objects (commits, trees, blobs) into their Worktree equivalents.
///
/// The converter holds a reference to the underlying `git2::Repository` (via `GitRepo`)
/// and a default `TreeId` that converted snapshots will be assigned to.
pub struct GitToWorktreeConverter<'repo> {
    /// The Git repository being converted from.
    repo: &'repo GitRepo,
    /// The Worktree tree ID to assign to converted snapshots.
    tree_id: TreeId,
}

impl<'repo> GitToWorktreeConverter<'repo> {
    /// Create a new converter targeting the given Worktree tree.
    pub fn new(repo: &'repo GitRepo, tree_id: TreeId) -> Self {
        Self { repo, tree_id }
    }

    /// Return a reference to the underlying [`GitRepo`].
    pub fn repo(&self) -> &GitRepo {
        self.repo
    }

    /// Convert a `git2::Commit` into a Worktree `Snapshot`.
    ///
    /// The commit message, author, timestamp, and parent linkage are mapped to
    /// the corresponding `Snapshot` fields. The commit's tree is converted
    /// separately via [`convert_tree`](Self::convert_tree).
    pub fn convert_commit(&self, _commit: &git2::Commit) -> Result<Snapshot> {
        todo!("convert git2::Commit → Snapshot: map author, message, parents, tree hash")
    }

    /// Convert a `git2::Tree` into a Worktree `Manifest`.
    ///
    /// Each tree entry is mapped to a `ManifestEntry` with the appropriate
    /// `EntryKind` (blob, sub-tree, symlink, etc.) and content hash.
    pub fn convert_tree(&self, _tree: &git2::Tree) -> Result<Manifest> {
        todo!("convert git2::Tree → Manifest: iterate entries, map kinds and hashes")
    }

    /// Convert a `git2::Blob` into a Worktree `Blob`.
    ///
    /// The blob content is copied and a BLAKE3 content hash is computed.
    pub fn convert_blob(&self, _blob: &git2::Blob) -> Result<Blob> {
        todo!("convert git2::Blob → Blob: copy content bytes, compute BLAKE3 hash")
    }

    /// Return the `TreeId` that this converter assigns to new snapshots.
    pub fn tree_id(&self) -> TreeId {
        self.tree_id
    }
}
