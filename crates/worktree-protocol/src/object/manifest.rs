use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::hash::{hash_bytes, ContentHash};
use crate::core::id::TreeId;

/// The kind of entry in a manifest (file or directory).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A directory.
    Directory,
    /// A symbolic link.
    Symlink,
}

/// A single entry within a [`Manifest`], representing one tracked path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestEntry {
    /// Relative path from the tree root.
    pub path: PathBuf,
    /// The kind of entry (file, directory, symlink).
    pub kind: EntryKind,
    /// Content hash of the blob (zero hash for directories).
    pub hash: ContentHash,
    /// Size in bytes (0 for directories).
    pub size: u64,
    /// Whether the file is marked executable.
    pub executable: bool,
}

impl ManifestEntry {
    /// Create a new file entry.
    pub fn file(path: impl Into<PathBuf>, hash: ContentHash, size: u64) -> Self {
        ManifestEntry {
            path: path.into(),
            kind: EntryKind::File,
            hash,
            size,
            executable: false,
        }
    }

    /// Create a new directory entry.
    pub fn directory(path: impl Into<PathBuf>) -> Self {
        ManifestEntry {
            path: path.into(),
            kind: EntryKind::Directory,
            hash: ContentHash::ZERO,
            size: 0,
            executable: false,
        }
    }

    /// Create a new symlink entry.
    pub fn symlink(path: impl Into<PathBuf>, target_hash: ContentHash) -> Self {
        ManifestEntry {
            path: path.into(),
            kind: EntryKind::Symlink,
            hash: target_hash,
            size: 0,
            executable: false,
        }
    }

    /// Return a copy of this entry with the executable flag set.
    pub fn with_executable(mut self, executable: bool) -> Self {
        self.executable = executable;
        self
    }
}

/// A manifest is a complete listing of every file and directory in a tree at a
/// given point in time. It is content-addressable via its computed hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The tree this manifest belongs to.
    pub tree_id: TreeId,
    /// The entries in this manifest.
    pub entries: Vec<ManifestEntry>,
    /// When this manifest was created.
    pub created_at: DateTime<Utc>,
}

impl Manifest {
    /// Create a new, empty manifest for the given tree.
    pub fn new(tree_id: TreeId) -> Self {
        Manifest {
            tree_id,
            entries: Vec::new(),
            created_at: Utc::now(),
        }
    }

    /// Add an entry to this manifest.
    pub fn add_entry(&mut self, entry: ManifestEntry) {
        self.entries.push(entry);
    }

    /// Sort entries by their path for deterministic hashing.
    pub fn sort_entries(&mut self) {
        self.entries.sort_by(|a, b| a.path.cmp(&b.path));
    }

    /// Find an entry by its path.
    pub fn find_entry(&self, path: &std::path::Path) -> Option<&ManifestEntry> {
        self.entries.iter().find(|e| e.path == path)
    }

    /// Compute a deterministic content hash over the sorted manifest entries.
    ///
    /// The hash is derived from the serialized (path, kind-tag, hash, size, executable)
    /// tuples of each entry, sorted by path. The tree id and timestamp are NOT
    /// included — two manifests with identical file content produce the same hash
    /// regardless of when or in which tree they were built.
    pub fn compute_hash(&self) -> ContentHash {
        let mut sorted = self.entries.clone();
        sorted.sort_by(|a, b| a.path.cmp(&b.path));

        let mut hasher = blake3::Hasher::new();
        for entry in &sorted {
            // Path bytes (UTF-8 lossy is fine; paths are always relative & valid).
            let path_bytes = entry.path.to_string_lossy();
            hasher.update(path_bytes.as_bytes());
            hasher.update(&[0xFF]); // separator

            // Kind tag
            let kind_tag: u8 = match entry.kind {
                EntryKind::File => 0,
                EntryKind::Directory => 1,
                EntryKind::Symlink => 2,
            };
            hasher.update(&[kind_tag]);

            // Content hash
            hasher.update(entry.hash.as_bytes());

            // Size (little-endian)
            hasher.update(&entry.size.to_le_bytes());

            // Executable flag
            hasher.update(&[entry.executable as u8]);
        }

        let result = hasher.finalize();
        ContentHash::from_bytes(*result.as_bytes())
    }

    /// Return the number of entries in the manifest.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether the manifest is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes as compute_hash;
    use crate::core::id::TreeId;

    fn sample_manifest() -> Manifest {
        let tree_id = TreeId::new();
        let mut m = Manifest::new(tree_id);

        let hash_a = compute_hash(b"file a content");
        let hash_b = compute_hash(b"file b content");

        m.add_entry(ManifestEntry::file("src/main.rs", hash_a, 100));
        m.add_entry(ManifestEntry::file("src/lib.rs", hash_b, 200));
        m.add_entry(ManifestEntry::directory("src"));
        m
    }

    #[test]
    fn test_new_manifest_is_empty() {
        let m = Manifest::new(TreeId::new());
        assert!(m.is_empty());
        assert_eq!(m.len(), 0);
    }

    #[test]
    fn test_add_entry() {
        let m = sample_manifest();
        assert_eq!(m.len(), 3);
        assert!(!m.is_empty());
    }

    #[test]
    fn test_find_entry() {
        let m = sample_manifest();
        let found = m.find_entry(std::path::Path::new("src/main.rs"));
        assert!(found.is_some());
        assert_eq!(found.unwrap().kind, EntryKind::File);
        assert_eq!(found.unwrap().size, 100);

        let not_found = m.find_entry(std::path::Path::new("missing.txt"));
        assert!(not_found.is_none());
    }

    #[test]
    fn test_sort_entries() {
        let mut m = sample_manifest();
        m.sort_entries();
        let paths: Vec<_> = m.entries.iter().map(|e| e.path.clone()).collect();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("src"),
                PathBuf::from("src/lib.rs"),
                PathBuf::from("src/main.rs"),
            ]
        );
    }

    #[test]
    fn test_compute_hash_deterministic() {
        let m = sample_manifest();
        let h1 = m.compute_hash();
        let h2 = m.compute_hash();
        assert_eq!(h1, h2);
    }

    #[test]
    fn test_compute_hash_order_independent() {
        let tree_id = TreeId::new();
        let hash_a = compute_hash(b"aaa");
        let hash_b = compute_hash(b"bbb");

        let mut m1 = Manifest::new(tree_id);
        m1.add_entry(ManifestEntry::file("a.txt", hash_a, 10));
        m1.add_entry(ManifestEntry::file("b.txt", hash_b, 20));

        let mut m2 = Manifest::new(tree_id);
        m2.add_entry(ManifestEntry::file("b.txt", hash_b, 20));
        m2.add_entry(ManifestEntry::file("a.txt", hash_a, 10));

        // compute_hash sorts internally, so order should not matter
        assert_eq!(m1.compute_hash(), m2.compute_hash());
    }

    #[test]
    fn test_compute_hash_differs_on_content_change() {
        let tree_id = TreeId::new();
        let hash_a = compute_hash(b"version 1");
        let hash_b = compute_hash(b"version 2");

        let mut m1 = Manifest::new(tree_id);
        m1.add_entry(ManifestEntry::file("file.txt", hash_a, 10));

        let mut m2 = Manifest::new(tree_id);
        m2.add_entry(ManifestEntry::file("file.txt", hash_b, 10));

        assert_ne!(m1.compute_hash(), m2.compute_hash());
    }

    #[test]
    fn test_directory_entry_has_zero_hash() {
        let entry = ManifestEntry::directory("some/dir");
        assert_eq!(entry.hash, ContentHash::ZERO);
        assert_eq!(entry.size, 0);
        assert_eq!(entry.kind, EntryKind::Directory);
    }

    #[test]
    fn test_executable_flag() {
        let hash = compute_hash(b"script");
        let entry = ManifestEntry::file("run.sh", hash, 50).with_executable(true);
        assert!(entry.executable);

        let tree_id = TreeId::new();
        let mut m1 = Manifest::new(tree_id);
        m1.add_entry(ManifestEntry::file("run.sh", hash, 50).with_executable(false));

        let mut m2 = Manifest::new(tree_id);
        m2.add_entry(ManifestEntry::file("run.sh", hash, 50).with_executable(true));

        assert_ne!(m1.compute_hash(), m2.compute_hash());
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let m = sample_manifest();
        let json = serde_json::to_string(&m).expect("serialize");
        let deserialized: Manifest = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m.entries.len(), deserialized.entries.len());
        assert_eq!(m.compute_hash(), deserialized.compute_hash());
    }
}
