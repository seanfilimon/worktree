use std::collections::HashMap;
use std::path::Path;

use worktree_protocol::core::hash::ContentHash;
use worktree_protocol::core::id::SnapshotId;
use worktree_protocol::object::blob::Blob;
use worktree_protocol::object::manifest::{EntryKind, Manifest};
use worktree_protocol::object::snapshot::Snapshot;

use crate::error::{GitCompatError, Result};

#[derive(Default)]
struct TreeNode {
    entries: HashMap<String, TreeEntry>,
}

enum TreeEntry {
    File { oid: git2::Oid, mode: i32 },
    Dir(TreeNode),
}

/// Converts Worktree objects (snapshots, manifests, blobs) into their
/// corresponding Git representations and writes them into a Git repository.
pub struct WorktreeToGitConverter {
    /// The Git repository we are writing objects into.
    repo: git2::Repository,
    /// Maps Worktree ContentHash to Git Oid for blobs and trees.
    pub content_map: HashMap<ContentHash, git2::Oid>,
    /// Maps Worktree SnapshotId to Git Oid for commits.
    pub commit_map: HashMap<SnapshotId, git2::Oid>,
}

impl WorktreeToGitConverter {
    /// Create a new converter targeting the given Git repository path.
    ///
    /// The path should point to an already-initialised Git repository
    /// (either bare or with a working directory).
    pub fn new(repo_path: &Path) -> Result<Self> {
        let repo = git2::Repository::open(repo_path)?;
        Ok(Self {
            repo,
            content_map: HashMap::new(),
            commit_map: HashMap::new(),
        })
    }

    /// Wrap an already-opened `git2::Repository`.
    pub fn from_repo(repo: git2::Repository) -> Self {
        Self {
            repo,
            content_map: HashMap::new(),
            commit_map: HashMap::new(),
        }
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
    pub fn convert_snapshot(&mut self, snapshot: &Snapshot) -> Result<git2::Oid> {
        let tree_oid = self
            .content_map
            .get(&snapshot.manifest_hash)
            .copied()
            .ok_or_else(|| {
                let hash_hex = snapshot
                    .manifest_hash
                    .as_bytes()
                    .iter()
                    .map(|b| format!("{:02x}", b))
                    .collect::<String>();
                GitCompatError::ExportError(format!(
                "Manifest hash {} not found in converter content map. Call convert_manifest first.",
                hash_hex
            ))
            })?;

        let tree = self.repo.find_tree(tree_oid)?;

        let mut parent_commits = Vec::new();
        for parent_id in &snapshot.parents {
            let parent_oid = self.commit_map.get(parent_id).copied().ok_or_else(|| {
                GitCompatError::ExportError(format!(
                    "Parent snapshot {} not found in converter commit map",
                    parent_id
                ))
            })?;
            let parent_commit = self.repo.find_commit(parent_oid)?;
            parent_commits.push(parent_commit);
        }

        let parent_refs: Vec<&git2::Commit> = parent_commits.iter().collect();

        let timestamp = snapshot.timestamp.timestamp();
        let time = git2::Time::new(timestamp, 0); // 0 offset = UTC

        let signature = self
            .repo
            .signature()
            .and_then(|sig| {
                git2::Signature::new(
                    sig.name().unwrap_or("unknown"),
                    sig.email().unwrap_or("unknown@worktree.local"),
                    &time,
                )
            })
            .unwrap_or_else(|_| {
                let author_str = snapshot.author.to_string();
                let email = format!("{}@worktree.local", author_str);
                git2::Signature::new(&author_str, &email, &time).unwrap()
            });

        let oid = self.repo.commit(
            None, // Do not update any reference automatically
            &signature,
            &signature,
            &snapshot.message,
            &tree,
            &parent_refs,
        )?;

        self.commit_map.insert(snapshot.id, oid);
        Ok(oid)
    }

    /// Convert a Worktree [`Manifest`] into a Git tree object and write it to
    /// the repository's object database.
    ///
    /// Each manifest entry is mapped to a Git tree entry with the appropriate
    /// file mode and object id.
    pub fn convert_manifest(&mut self, manifest: &Manifest) -> Result<git2::Oid> {
        let mut root = TreeNode::default();

        for entry in &manifest.entries {
            let components: Vec<String> = entry
                .path
                .iter()
                .map(|c| c.to_string_lossy().into_owned())
                .filter(|c| !c.is_empty())
                .collect();

            if components.is_empty() {
                continue;
            }

            let mut current = &mut root;
            for (i, comp) in components.iter().enumerate() {
                let is_last = i == components.len() - 1;

                if is_last {
                    match entry.kind {
                        EntryKind::File => {
                            let oid =
                                self.content_map.get(&entry.hash).copied().ok_or_else(|| {
                                    let hash_hex = entry
                                        .hash
                                        .as_bytes()
                                        .iter()
                                        .map(|b| format!("{:02x}", b))
                                        .collect::<String>();
                                    GitCompatError::ExportError(format!(
                                        "Blob hash {} not found in content map for path {}",
                                        hash_hex,
                                        entry.path.display()
                                    ))
                                })?;
                            let mode = if entry.executable { 0o100755 } else { 0o100644 };
                            current
                                .entries
                                .insert(comp.clone(), TreeEntry::File { oid, mode });
                        }
                        EntryKind::Symlink => {
                            let oid =
                                self.content_map.get(&entry.hash).copied().ok_or_else(|| {
                                    let hash_hex = entry
                                        .hash
                                        .as_bytes()
                                        .iter()
                                        .map(|b| format!("{:02x}", b))
                                        .collect::<String>();
                                    GitCompatError::ExportError(format!(
                                        "Blob hash {} not found in content map for symlink path {}",
                                        hash_hex,
                                        entry.path.display()
                                    ))
                                })?;
                            let mode = 0o120000;
                            current
                                .entries
                                .insert(comp.clone(), TreeEntry::File { oid, mode });
                        }
                        EntryKind::Directory => {
                            current
                                .entries
                                .entry(comp.clone())
                                .or_insert_with(|| TreeEntry::Dir(TreeNode::default()));
                        }
                    }
                } else {
                    let next = current
                        .entries
                        .entry(comp.clone())
                        .or_insert_with(|| TreeEntry::Dir(TreeNode::default()));
                    match next {
                        TreeEntry::Dir(ref mut node) => {
                            current = node;
                        }
                        TreeEntry::File { .. } => {
                            return Err(GitCompatError::ExportError(format!(
                                "Path conflict: intermediate component '{}' in path '{}' is a file",
                                comp,
                                entry.path.display()
                            )));
                        }
                    }
                }
            }
        }

        let tree_oid = self.write_tree(&root)?;

        let manifest_hash = manifest.compute_hash();
        self.content_map.insert(manifest_hash, tree_oid);

        Ok(tree_oid)
    }

    fn write_tree(&self, node: &TreeNode) -> Result<git2::Oid> {
        let mut builder = self.repo.treebuilder(None)?;

        // Sort entries by name to ensure stable Git trees and deterministic hashes
        let mut sorted_entries: Vec<_> = node.entries.iter().collect();
        sorted_entries.sort_by(|a, b| a.0.cmp(b.0));

        for (name, entry) in sorted_entries {
            match entry {
                TreeEntry::File { oid, mode } => {
                    builder.insert(name, *oid, *mode)?;
                }
                TreeEntry::Dir(child_node) => {
                    let child_oid = self.write_tree(child_node)?;
                    builder.insert(name, child_oid, 0o040000)?;
                }
            }
        }

        let oid = builder.write()?;
        Ok(oid)
    }

    /// Convert a Worktree [`Blob`] into a Git blob and write it to the
    /// repository's object database.
    ///
    /// Returns the `git2::Oid` of the newly written blob.
    pub fn convert_blob<R: std::io::Read>(
        &mut self,
        blob: &Blob,
        mut reader: R,
    ) -> Result<git2::Oid> {
        let mut stream = self.repo.blob_writer(None)?;
        std::io::copy(&mut reader, &mut stream)
            .map_err(|e| GitCompatError::ExportError(format!("Failed to stream blob: {}", e)))?;
        let oid = stream.commit()?;
        self.content_map.insert(blob.hash, oid);
        Ok(oid)
    }
}
