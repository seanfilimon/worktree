//! Git object mapping.
//!
//! Provides [`GitObjectKind`] and [`ObjectMapping`] for bidirectional mapping
//! between Worktree's native object model and Git's object model (blobs, trees,
//! commits, tags).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

use crate::core::hash::ContentHash;

// ---------------------------------------------------------------------------
// GitObjectKind
// ---------------------------------------------------------------------------

/// The kind of Git object being mapped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitObjectKind {
    /// A Git blob (file content).
    Blob,
    /// A Git tree (directory listing).
    Tree,
    /// A Git commit.
    Commit,
    /// A Git annotated tag.
    Tag,
}

impl GitObjectKind {
    /// Returns the stable string representation of this kind.
    pub fn as_str(&self) -> &'static str {
        match self {
            GitObjectKind::Blob => "blob",
            GitObjectKind::Tree => "tree",
            GitObjectKind::Commit => "commit",
            GitObjectKind::Tag => "tag",
        }
    }

    /// Parse a Git object kind from a string.
    pub fn from_str_lossy(s: &str) -> Option<Self> {
        match s {
            "blob" => Some(GitObjectKind::Blob),
            "tree" => Some(GitObjectKind::Tree),
            "commit" => Some(GitObjectKind::Commit),
            "tag" => Some(GitObjectKind::Tag),
            _ => None,
        }
    }
}

impl fmt::Display for GitObjectKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

// ---------------------------------------------------------------------------
// ObjectMapping
// ---------------------------------------------------------------------------

/// A mapping between a Worktree content hash and a Git object identifier.
///
/// This is used to maintain a bidirectional index so that Worktree objects
/// can be exported to Git repositories and Git objects can be imported into
/// Worktree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectMapping {
    /// The Worktree BLAKE3 content hash.
    pub worktree_hash: ContentHash,

    /// The Git object SHA-1 hash as a 40-character hex string.
    pub git_sha: String,

    /// The kind of Git object.
    pub kind: GitObjectKind,

    /// The size of the object content in bytes.
    pub size: u64,
}

impl ObjectMapping {
    /// Create a new object mapping.
    pub fn new(
        worktree_hash: ContentHash,
        git_sha: impl Into<String>,
        kind: GitObjectKind,
        size: u64,
    ) -> Self {
        Self {
            worktree_hash,
            git_sha: git_sha.into(),
            kind,
            size,
        }
    }

    /// Returns `true` if the Git SHA looks like a valid 40-character hex string.
    pub fn is_valid_git_sha(&self) -> bool {
        self.git_sha.len() == 40 && self.git_sha.chars().all(|c| c.is_ascii_hexdigit())
    }
}

impl fmt::Display for ObjectMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} <-> {} ({} bytes)",
            self.kind, self.git_sha, self.worktree_hash, self.size
        )
    }
}

// ---------------------------------------------------------------------------
// ObjectMappingIndex
// ---------------------------------------------------------------------------

/// An in-memory index for looking up object mappings in both directions.
///
/// Supports lookup by Worktree content hash or by Git SHA-1 hex string.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ObjectMappingIndex {
    /// Mappings indexed by Worktree content hash.
    by_worktree_hash: HashMap<ContentHash, ObjectMapping>,
    /// Mappings indexed by Git SHA-1 hex string.
    by_git_sha: HashMap<String, ObjectMapping>,
}

impl ObjectMappingIndex {
    /// Create a new, empty index.
    pub fn new() -> Self {
        Self {
            by_worktree_hash: HashMap::new(),
            by_git_sha: HashMap::new(),
        }
    }

    /// Insert a mapping into the index.
    pub fn insert(&mut self, mapping: ObjectMapping) {
        self.by_git_sha
            .insert(mapping.git_sha.clone(), mapping.clone());
        self.by_worktree_hash
            .insert(mapping.worktree_hash, mapping);
    }

    /// Look up a mapping by its Worktree content hash.
    pub fn get_by_worktree_hash(&self, hash: &ContentHash) -> Option<&ObjectMapping> {
        self.by_worktree_hash.get(hash)
    }

    /// Look up a mapping by its Git SHA-1 hex string.
    pub fn get_by_git_sha(&self, sha: &str) -> Option<&ObjectMapping> {
        self.by_git_sha.get(sha)
    }

    /// Remove a mapping by its Worktree content hash.
    ///
    /// Returns the removed mapping if it existed.
    pub fn remove_by_worktree_hash(&mut self, hash: &ContentHash) -> Option<ObjectMapping> {
        if let Some(mapping) = self.by_worktree_hash.remove(hash) {
            self.by_git_sha.remove(&mapping.git_sha);
            Some(mapping)
        } else {
            None
        }
    }

    /// Remove a mapping by its Git SHA.
    ///
    /// Returns the removed mapping if it existed.
    pub fn remove_by_git_sha(&mut self, sha: &str) -> Option<ObjectMapping> {
        if let Some(mapping) = self.by_git_sha.remove(sha) {
            self.by_worktree_hash.remove(&mapping.worktree_hash);
            Some(mapping)
        } else {
            None
        }
    }

    /// Returns `true` if the index contains a mapping for the given Worktree hash.
    pub fn contains_worktree_hash(&self, hash: &ContentHash) -> bool {
        self.by_worktree_hash.contains_key(hash)
    }

    /// Returns `true` if the index contains a mapping for the given Git SHA.
    pub fn contains_git_sha(&self, sha: &str) -> bool {
        self.by_git_sha.contains_key(sha)
    }

    /// Returns the number of mappings in the index.
    pub fn len(&self) -> usize {
        self.by_worktree_hash.len()
    }

    /// Returns `true` if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.by_worktree_hash.is_empty()
    }

    /// Iterate over all mappings.
    pub fn iter(&self) -> impl Iterator<Item = &ObjectMapping> {
        self.by_worktree_hash.values()
    }

    /// Return all mappings of a given kind.
    pub fn mappings_by_kind(&self, kind: GitObjectKind) -> Vec<&ObjectMapping> {
        self.by_worktree_hash
            .values()
            .filter(|m| m.kind == kind)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;

    fn sample_mapping() -> ObjectMapping {
        ObjectMapping::new(
            hash_bytes(b"blob content"),
            "aabbccdd00112233445566778899aabbccddeeff",
            GitObjectKind::Blob,
            12,
        )
    }

    // ── GitObjectKind ───────────────────────────────────────────────────

    #[test]
    fn test_kind_display() {
        assert_eq!(GitObjectKind::Blob.to_string(), "blob");
        assert_eq!(GitObjectKind::Tree.to_string(), "tree");
        assert_eq!(GitObjectKind::Commit.to_string(), "commit");
        assert_eq!(GitObjectKind::Tag.to_string(), "tag");
    }

    #[test]
    fn test_kind_as_str() {
        let kinds = [
            GitObjectKind::Blob,
            GitObjectKind::Tree,
            GitObjectKind::Commit,
            GitObjectKind::Tag,
        ];
        for kind in &kinds {
            assert_eq!(kind.to_string(), kind.as_str());
        }
    }

    #[test]
    fn test_kind_from_str_lossy() {
        assert_eq!(
            GitObjectKind::from_str_lossy("blob"),
            Some(GitObjectKind::Blob)
        );
        assert_eq!(
            GitObjectKind::from_str_lossy("tree"),
            Some(GitObjectKind::Tree)
        );
        assert_eq!(
            GitObjectKind::from_str_lossy("commit"),
            Some(GitObjectKind::Commit)
        );
        assert_eq!(
            GitObjectKind::from_str_lossy("tag"),
            Some(GitObjectKind::Tag)
        );
        assert_eq!(GitObjectKind::from_str_lossy("unknown"), None);
        assert_eq!(GitObjectKind::from_str_lossy(""), None);
    }

    #[test]
    fn test_kind_serde_roundtrip() {
        for kind in &[
            GitObjectKind::Blob,
            GitObjectKind::Tree,
            GitObjectKind::Commit,
            GitObjectKind::Tag,
        ] {
            let json = serde_json::to_string(kind).expect("serialize");
            let deserialized: GitObjectKind =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(*kind, deserialized);
        }
    }

    #[test]
    fn test_kind_copy_and_hash() {
        use std::collections::HashSet;

        let k = GitObjectKind::Commit;
        let k2 = k;
        assert_eq!(k, k2);

        let mut set = HashSet::new();
        set.insert(GitObjectKind::Blob);
        set.insert(GitObjectKind::Blob);
        set.insert(GitObjectKind::Tree);
        assert_eq!(set.len(), 2);
    }

    // ── ObjectMapping ───────────────────────────────────────────────────

    #[test]
    fn test_object_mapping_new() {
        let m = sample_mapping();
        assert_eq!(m.kind, GitObjectKind::Blob);
        assert_eq!(m.size, 12);
        assert!(m.is_valid_git_sha());
    }

    #[test]
    fn test_object_mapping_invalid_git_sha() {
        let m = ObjectMapping::new(
            hash_bytes(b"test"),
            "short",
            GitObjectKind::Blob,
            5,
        );
        assert!(!m.is_valid_git_sha());

        let m2 = ObjectMapping::new(
            hash_bytes(b"test"),
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz",
            GitObjectKind::Blob,
            5,
        );
        assert!(!m2.is_valid_git_sha());
    }

    #[test]
    fn test_object_mapping_display() {
        let m = sample_mapping();
        let display = m.to_string();
        assert!(display.contains("blob"));
        assert!(display.contains("aabbccdd00112233445566778899aabbccddeeff"));
        assert!(display.contains("12 bytes"));
    }

    #[test]
    fn test_object_mapping_serde_json_roundtrip() {
        let m = sample_mapping();
        let json = serde_json::to_string(&m).expect("serialize");
        let deserialized: ObjectMapping = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, deserialized);
    }

    #[test]
    fn test_object_mapping_serde_bincode_roundtrip() {
        let m = sample_mapping();
        let encoded = bincode::serialize(&m).expect("serialize");
        let decoded: ObjectMapping = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(m, decoded);
    }

    #[test]
    fn test_object_mapping_clone() {
        let m = sample_mapping();
        let cloned = m.clone();
        assert_eq!(m, cloned);
    }

    // ── ObjectMappingIndex ──────────────────────────────────────────────

    #[test]
    fn test_index_new_is_empty() {
        let index = ObjectMappingIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_index_insert_and_lookup() {
        let mut index = ObjectMappingIndex::new();
        let m = sample_mapping();

        index.insert(m.clone());
        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());

        let by_wt = index.get_by_worktree_hash(&m.worktree_hash);
        assert!(by_wt.is_some());
        assert_eq!(by_wt.unwrap(), &m);

        let by_git = index.get_by_git_sha(&m.git_sha);
        assert!(by_git.is_some());
        assert_eq!(by_git.unwrap(), &m);
    }

    #[test]
    fn test_index_contains() {
        let mut index = ObjectMappingIndex::new();
        let m = sample_mapping();

        assert!(!index.contains_worktree_hash(&m.worktree_hash));
        assert!(!index.contains_git_sha(&m.git_sha));

        index.insert(m.clone());

        assert!(index.contains_worktree_hash(&m.worktree_hash));
        assert!(index.contains_git_sha(&m.git_sha));
    }

    #[test]
    fn test_index_remove_by_worktree_hash() {
        let mut index = ObjectMappingIndex::new();
        let m = sample_mapping();

        index.insert(m.clone());
        assert_eq!(index.len(), 1);

        let removed = index.remove_by_worktree_hash(&m.worktree_hash);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap(), m);
        assert!(index.is_empty());
        assert!(!index.contains_git_sha(&m.git_sha));
    }

    #[test]
    fn test_index_remove_by_git_sha() {
        let mut index = ObjectMappingIndex::new();
        let m = sample_mapping();

        index.insert(m.clone());
        assert_eq!(index.len(), 1);

        let removed = index.remove_by_git_sha(&m.git_sha);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap(), m);
        assert!(index.is_empty());
        assert!(!index.contains_worktree_hash(&m.worktree_hash));
    }

    #[test]
    fn test_index_remove_nonexistent() {
        let mut index = ObjectMappingIndex::new();
        let hash = hash_bytes(b"nope");

        let removed = index.remove_by_worktree_hash(&hash);
        assert!(removed.is_none());

        let removed = index.remove_by_git_sha("0000000000000000000000000000000000000000");
        assert!(removed.is_none());
    }

    #[test]
    fn test_index_multiple_entries() {
        let mut index = ObjectMappingIndex::new();

        let m1 = ObjectMapping::new(
            hash_bytes(b"blob1"),
            "1111111111111111111111111111111111111111",
            GitObjectKind::Blob,
            10,
        );
        let m2 = ObjectMapping::new(
            hash_bytes(b"tree1"),
            "2222222222222222222222222222222222222222",
            GitObjectKind::Tree,
            200,
        );
        let m3 = ObjectMapping::new(
            hash_bytes(b"commit1"),
            "3333333333333333333333333333333333333333",
            GitObjectKind::Commit,
            300,
        );

        index.insert(m1.clone());
        index.insert(m2.clone());
        index.insert(m3.clone());

        assert_eq!(index.len(), 3);

        assert_eq!(index.get_by_worktree_hash(&m1.worktree_hash).unwrap(), &m1);
        assert_eq!(index.get_by_git_sha(&m2.git_sha).unwrap(), &m2);
        assert_eq!(index.get_by_worktree_hash(&m3.worktree_hash).unwrap(), &m3);
    }

    #[test]
    fn test_index_overwrite() {
        let mut index = ObjectMappingIndex::new();
        let hash = hash_bytes(b"content");

        let m1 = ObjectMapping::new(
            hash,
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            GitObjectKind::Blob,
            10,
        );
        let m2 = ObjectMapping::new(
            hash,
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            GitObjectKind::Blob,
            10,
        );

        index.insert(m1.clone());
        assert_eq!(index.get_by_worktree_hash(&hash).unwrap().git_sha, m1.git_sha);

        index.insert(m2.clone());
        assert_eq!(index.get_by_worktree_hash(&hash).unwrap().git_sha, m2.git_sha);
    }

    #[test]
    fn test_index_iter() {
        let mut index = ObjectMappingIndex::new();

        let m1 = ObjectMapping::new(
            hash_bytes(b"iter1"),
            "1111111111111111111111111111111111111111",
            GitObjectKind::Blob,
            1,
        );
        let m2 = ObjectMapping::new(
            hash_bytes(b"iter2"),
            "2222222222222222222222222222222222222222",
            GitObjectKind::Tree,
            2,
        );

        index.insert(m1);
        index.insert(m2);

        let all: Vec<_> = index.iter().collect();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_index_mappings_by_kind() {
        let mut index = ObjectMappingIndex::new();

        let m1 = ObjectMapping::new(
            hash_bytes(b"blob_a"),
            "1111111111111111111111111111111111111111",
            GitObjectKind::Blob,
            10,
        );
        let m2 = ObjectMapping::new(
            hash_bytes(b"tree_a"),
            "2222222222222222222222222222222222222222",
            GitObjectKind::Tree,
            20,
        );
        let m3 = ObjectMapping::new(
            hash_bytes(b"blob_b"),
            "3333333333333333333333333333333333333333",
            GitObjectKind::Blob,
            30,
        );
        let m4 = ObjectMapping::new(
            hash_bytes(b"commit_a"),
            "4444444444444444444444444444444444444444",
            GitObjectKind::Commit,
            40,
        );

        index.insert(m1);
        index.insert(m2);
        index.insert(m3);
        index.insert(m4);

        let blobs = index.mappings_by_kind(GitObjectKind::Blob);
        assert_eq!(blobs.len(), 2);

        let trees = index.mappings_by_kind(GitObjectKind::Tree);
        assert_eq!(trees.len(), 1);

        let commits = index.mappings_by_kind(GitObjectKind::Commit);
        assert_eq!(commits.len(), 1);

        let tags = index.mappings_by_kind(GitObjectKind::Tag);
        assert!(tags.is_empty());
    }

    #[test]
    fn test_index_default() {
        let index = ObjectMappingIndex::default();
        assert!(index.is_empty());
    }

    #[test]
    fn test_index_serde_json_roundtrip() {
        let mut index = ObjectMappingIndex::new();
        index.insert(ObjectMapping::new(
            hash_bytes(b"serde_idx"),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            GitObjectKind::Blob,
            42,
        ));
        index.insert(ObjectMapping::new(
            hash_bytes(b"serde_idx2"),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            GitObjectKind::Commit,
            100,
        ));

        let json = serde_json::to_string(&index).expect("serialize");
        let deserialized: ObjectMappingIndex =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.len(), 2);
        assert!(deserialized.contains_git_sha("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"));
        assert!(deserialized.contains_git_sha("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"));
    }

    #[test]
    fn test_lookup_nonexistent_returns_none() {
        let index = ObjectMappingIndex::new();
        let hash = hash_bytes(b"nonexistent");

        assert!(index.get_by_worktree_hash(&hash).is_none());
        assert!(index.get_by_git_sha("0000000000000000000000000000000000000000").is_none());
    }
}
