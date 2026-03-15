use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::core::hash::ContentHash;

/// The kind of change a delta represents.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DeltaKind {
    /// A new file or entry was added.
    Add,
    /// An existing file or entry was modified.
    Modify,
    /// A file or entry was deleted.
    Delete,
    /// A file or entry was renamed from one path to another.
    Rename {
        /// The original path before the rename.
        from: PathBuf,
    },
    /// A file or entry was copied from another path.
    Copy {
        /// The source path that was copied.
        from: PathBuf,
    },
}

/// A single change (delta) between two snapshots or manifest states.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    /// The path affected by this delta.
    pub path: PathBuf,

    /// The kind of change.
    pub kind: DeltaKind,

    /// The content hash before the change, if applicable.
    pub old_hash: Option<ContentHash>,

    /// The content hash after the change, if applicable.
    pub new_hash: Option<ContentHash>,

    /// The size of the content before the change, if applicable.
    pub old_size: Option<u64>,

    /// The size of the content after the change, if applicable.
    pub new_size: Option<u64>,
}

impl Delta {
    /// Create a new delta with the given path, kind, and optional hashes.
    pub fn new(
        path: PathBuf,
        kind: DeltaKind,
        old_hash: Option<ContentHash>,
        new_hash: Option<ContentHash>,
    ) -> Self {
        Self {
            path,
            kind,
            old_hash,
            new_hash,
            old_size: None,
            new_size: None,
        }
    }

    /// Create an "add" delta for a new file.
    pub fn add(path: impl Into<PathBuf>, new_hash: ContentHash, new_size: u64) -> Self {
        Self {
            path: path.into(),
            kind: DeltaKind::Add,
            old_hash: None,
            new_hash: Some(new_hash),
            old_size: None,
            new_size: Some(new_size),
        }
    }

    /// Create a "modify" delta for a changed file.
    pub fn modify(
        path: impl Into<PathBuf>,
        old_hash: ContentHash,
        new_hash: ContentHash,
        old_size: u64,
        new_size: u64,
    ) -> Self {
        Self {
            path: path.into(),
            kind: DeltaKind::Modify,
            old_hash: Some(old_hash),
            new_hash: Some(new_hash),
            old_size: Some(old_size),
            new_size: Some(new_size),
        }
    }

    /// Create a "delete" delta for a removed file.
    pub fn delete(path: impl Into<PathBuf>, old_hash: ContentHash, old_size: u64) -> Self {
        Self {
            path: path.into(),
            kind: DeltaKind::Delete,
            old_hash: Some(old_hash),
            new_hash: None,
            old_size: Some(old_size),
            new_size: None,
        }
    }

    /// Create a "rename" delta.
    pub fn rename(
        from: impl Into<PathBuf>,
        to: impl Into<PathBuf>,
        hash: ContentHash,
        size: u64,
    ) -> Self {
        Self {
            path: to.into(),
            kind: DeltaKind::Rename {
                from: from.into(),
            },
            old_hash: Some(hash),
            new_hash: Some(hash),
            old_size: Some(size),
            new_size: Some(size),
        }
    }

    /// Create a "copy" delta.
    pub fn copy(
        from: impl Into<PathBuf>,
        to: impl Into<PathBuf>,
        hash: ContentHash,
        size: u64,
    ) -> Self {
        Self {
            path: to.into(),
            kind: DeltaKind::Copy {
                from: from.into(),
            },
            old_hash: None,
            new_hash: Some(hash),
            old_size: None,
            new_size: Some(size),
        }
    }

    /// Returns `true` if this delta represents adding a new entry.
    pub fn is_add(&self) -> bool {
        matches!(self.kind, DeltaKind::Add)
    }

    /// Returns `true` if this delta represents modifying an existing entry.
    pub fn is_modify(&self) -> bool {
        matches!(self.kind, DeltaKind::Modify)
    }

    /// Returns `true` if this delta represents deleting an entry.
    pub fn is_delete(&self) -> bool {
        matches!(self.kind, DeltaKind::Delete)
    }

    /// Returns `true` if this delta represents a rename.
    pub fn is_rename(&self) -> bool {
        matches!(self.kind, DeltaKind::Rename { .. })
    }

    /// Returns `true` if this delta represents a copy.
    pub fn is_copy(&self) -> bool {
        matches!(self.kind, DeltaKind::Copy { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::hash::hash_bytes;

    #[test]
    fn test_add_delta() {
        let hash = hash_bytes(b"new content");
        let delta = Delta::add("src/main.rs", hash, 42);
        assert!(delta.is_add());
        assert!(!delta.is_modify());
        assert!(!delta.is_delete());
        assert_eq!(delta.path, PathBuf::from("src/main.rs"));
        assert_eq!(delta.old_hash, None);
        assert_eq!(delta.new_hash, Some(hash));
        assert_eq!(delta.old_size, None);
        assert_eq!(delta.new_size, Some(42));
    }

    #[test]
    fn test_modify_delta() {
        let old = hash_bytes(b"old");
        let new = hash_bytes(b"new");
        let delta = Delta::modify("file.txt", old, new, 3, 3);
        assert!(delta.is_modify());
        assert_eq!(delta.old_hash, Some(old));
        assert_eq!(delta.new_hash, Some(new));
        assert_eq!(delta.old_size, Some(3));
        assert_eq!(delta.new_size, Some(3));
    }

    #[test]
    fn test_delete_delta() {
        let hash = hash_bytes(b"gone");
        let delta = Delta::delete("removed.txt", hash, 4);
        assert!(delta.is_delete());
        assert_eq!(delta.old_hash, Some(hash));
        assert_eq!(delta.new_hash, None);
        assert_eq!(delta.new_size, None);
    }

    #[test]
    fn test_rename_delta() {
        let hash = hash_bytes(b"content");
        let delta = Delta::rename("old_name.rs", "new_name.rs", hash, 7);
        assert!(delta.is_rename());
        assert_eq!(delta.path, PathBuf::from("new_name.rs"));
        if let DeltaKind::Rename { from } = &delta.kind {
            assert_eq!(from, &PathBuf::from("old_name.rs"));
        } else {
            panic!("expected Rename kind");
        }
        assert_eq!(delta.old_hash, Some(hash));
        assert_eq!(delta.new_hash, Some(hash));
    }

    #[test]
    fn test_copy_delta() {
        let hash = hash_bytes(b"copied");
        let delta = Delta::copy("source.rs", "dest.rs", hash, 6);
        assert!(delta.is_copy());
        assert_eq!(delta.path, PathBuf::from("dest.rs"));
        if let DeltaKind::Copy { from } = &delta.kind {
            assert_eq!(from, &PathBuf::from("source.rs"));
        } else {
            panic!("expected Copy kind");
        }
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let hash = hash_bytes(b"serde test");
        let delta = Delta::add("test.txt", hash, 10);
        let json = serde_json::to_string(&delta).expect("serialize");
        let deserialized: Delta = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(delta, deserialized);
    }

    #[test]
    fn test_serde_bincode_roundtrip() {
        let hash = hash_bytes(b"bincode test");
        let delta = Delta::modify("a.txt", hash, hash, 12, 12);
        let bytes = bincode::serialize(&delta).expect("serialize");
        let deserialized: Delta = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(delta, deserialized);
    }

    #[test]
    fn test_new_constructor() {
        let old = hash_bytes(b"old");
        let new = hash_bytes(b"new");
        let delta = Delta::new(
            PathBuf::from("file.rs"),
            DeltaKind::Modify,
            Some(old),
            Some(new),
        );
        assert!(delta.is_modify());
        assert_eq!(delta.old_size, None);
        assert_eq!(delta.new_size, None);
    }
}
