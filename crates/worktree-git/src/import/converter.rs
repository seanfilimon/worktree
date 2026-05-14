use std::cell::RefCell;
use std::collections::HashMap;

use chrono::{DateTime, Utc};
use uuid::Uuid;
use worktree_protocol::core::hash::{hash_bytes, ContentHash};
use worktree_protocol::core::id::{AccountId, SnapshotId, TreeId};
use worktree_protocol::object::blob::Blob;
use worktree_protocol::object::manifest::{Manifest, ManifestEntry};
use worktree_protocol::object::snapshot::Snapshot;

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
    /// Cache mapping git blob OIDs to their Worktree ContentHash and size.
    blob_cache: RefCell<HashMap<git2::Oid, (ContentHash, u64)>>,
    /// Cache mapping git tree OIDs to their parsed Worktree Manifests.
    manifest_cache: RefCell<HashMap<git2::Oid, Manifest>>,
}

impl<'repo> GitToWorktreeConverter<'repo> {
    /// Create a new converter targeting the given Worktree tree.
    pub fn new(repo: &'repo GitRepo, tree_id: TreeId) -> Self {
        Self {
            repo,
            tree_id,
            blob_cache: RefCell::new(HashMap::new()),
            manifest_cache: RefCell::new(HashMap::new()),
        }
    }

    /// Return a reference to the underlying [`GitRepo`].
    pub fn repo(&self) -> &GitRepo {
        self.repo
    }

    /// Convert a `git2::Commit` into a Worktree `Snapshot`.
    ///
    /// The commit message, author, timestamp, and parent linkage are mapped to
    /// the corresponding `Snapshot` fields. The commit's tree is converted
    /// separately via [`convert_tree`](Self::convert_tree) (though this method
    /// uses it internally to resolve the manifest hash).
    pub fn convert_commit(&self, commit: &git2::Commit) -> Result<Snapshot> {
        let tree = commit.tree()?;
        let manifest = self.convert_tree(&tree)?;
        let manifest_hash = manifest.compute_hash();

        let parents: Vec<SnapshotId> = commit.parent_ids().map(oid_to_snapshot_id).collect();

        let author_email = commit.author().email().unwrap_or("").to_string();
        let account_id = email_to_account_id(&author_email);

        let message = commit.message().unwrap_or("").to_string();

        let timestamp_secs = commit.time().seconds();
        let timestamp = DateTime::from_timestamp(timestamp_secs, 0).unwrap_or_else(Utc::now);

        let snapshot = Snapshot {
            id: oid_to_snapshot_id(commit.id()),
            tree_id: self.tree_id,
            manifest_hash,
            parents,
            message,
            author: account_id,
            timestamp,
            auto_generated: false,
        };

        Ok(snapshot)
    }

    /// Convert a `git2::Tree` into a Worktree `Manifest`.
    ///
    /// Each tree entry is mapped to a `ManifestEntry` with the appropriate
    /// `EntryKind` (blob, sub-tree, symlink, etc.) and content hash.
    pub fn convert_tree(&self, tree: &git2::Tree) -> Result<Manifest> {
        let oid = tree.id();
        if let Some(manifest) = self.manifest_cache.borrow().get(&oid) {
            return Ok(manifest.clone());
        }

        let mut manifest = Manifest::new(self.tree_id);
        self.walk_git_tree(tree, std::path::Path::new(""), &mut manifest)?;
        manifest.sort_entries();

        self.manifest_cache
            .borrow_mut()
            .insert(oid, manifest.clone());
        Ok(manifest)
    }

    /// Recursively traverse a Git tree and populate the Manifest with flat paths.
    fn walk_git_tree(
        &self,
        tree: &git2::Tree,
        current_path: &std::path::Path,
        manifest: &mut Manifest,
    ) -> Result<()> {
        for entry in tree.iter() {
            let name = entry.name().unwrap_or("?");
            let entry_path = current_path.join(name);

            match entry.kind() {
                Some(git2::ObjectType::Blob) => {
                    let (hash, size) = self.get_blob_info(entry.id())?;
                    let is_symlink = entry.filemode() == 0o120000;
                    let is_exec = entry.filemode() == 0o100755;

                    let m_entry = if is_symlink {
                        ManifestEntry::symlink(entry_path, hash)
                    } else {
                        ManifestEntry::file(entry_path, hash, size).with_executable(is_exec)
                    };
                    manifest.add_entry(m_entry);
                }
                Some(git2::ObjectType::Tree) => {
                    manifest.add_entry(ManifestEntry::directory(entry_path.clone()));
                    let subtree = self.repo.inner().find_tree(entry.id())?;
                    self.walk_git_tree(&subtree, &entry_path, manifest)?;
                }
                Some(git2::ObjectType::Commit) => {
                    // Submodule mount point is represented as a directory in Worktree
                    manifest.add_entry(ManifestEntry::directory(entry_path));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Convert a `git2::Blob` into a Worktree `Blob` metadata object.
    ///
    /// A BLAKE3 content hash is computed. The caller is responsible for reading the content.
    pub fn convert_blob(&self, blob: &git2::Blob) -> Result<Blob> {
        let content = blob.content();
        let hash = hash_bytes(content);
        Ok(Blob {
            hash,
            size: content.len() as u64,
        })
    }

    /// Internal helper to get hash and size of a blob, using a cache to avoid re-reading and re-hashing.
    fn get_blob_info(&self, oid: git2::Oid) -> Result<(ContentHash, u64)> {
        if let Some(info) = self.blob_cache.borrow().get(&oid) {
            return Ok(*info);
        }

        let blob = self.repo.inner().find_blob(oid)?;
        let content = blob.content();
        let hash = hash_bytes(content);
        let size = content.len() as u64;

        self.blob_cache.borrow_mut().insert(oid, (hash, size));
        Ok((hash, size))
    }

    /// Return the `TreeId` that this converter assigns to new snapshots.
    pub fn tree_id(&self) -> TreeId {
        self.tree_id
    }
}

/// Helper to deterministically map a Git Commit OID to a Worktree SnapshotId.
fn oid_to_snapshot_id(oid: git2::Oid) -> SnapshotId {
    let mut bytes = [0u8; 16];
    let oid_bytes = oid.as_bytes();
    bytes.copy_from_slice(&oid_bytes[0..16]);
    SnapshotId::from_uuid(Uuid::from_bytes(bytes))
}

/// Helper to deterministically map an email string to a Worktree AccountId.
fn email_to_account_id(email: &str) -> AccountId {
    let hash = hash_bytes(email.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&hash.as_bytes()[0..16]);
    AccountId::from_uuid(Uuid::from_bytes(bytes))
}
