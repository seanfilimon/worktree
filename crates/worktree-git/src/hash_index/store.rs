use std::collections::HashMap;

use worktree_protocol::compat::git_hash_map::{GitHash, HashIndex, HashMapping};
use worktree_protocol::core::hash::ContentHash;

/// An in-memory bidirectional index mapping Git SHA-1 hashes to Worktree BLAKE3
/// content hashes and vice versa.
///
/// This implementation uses two `HashMap`s for O(1) lookups in both directions.
/// It is suitable for moderate-sized repositories; for very large repos a
/// persistent on-disk index would be preferable.
pub struct InMemoryHashIndex {
    /// Maps Worktree BLAKE3 → Git SHA-1.
    blake3_to_sha1: HashMap<ContentHash, GitHash>,
    /// Maps Git SHA-1 → Worktree BLAKE3.
    sha1_to_blake3: HashMap<GitHash, ContentHash>,
}

impl InMemoryHashIndex {
    /// Create a new, empty in-memory hash index.
    pub fn new() -> Self {
        Self {
            blake3_to_sha1: HashMap::new(),
            sha1_to_blake3: HashMap::new(),
        }
    }

    /// Return `true` if the index contains no mappings.
    pub fn is_empty(&self) -> bool {
        self.blake3_to_sha1.is_empty()
    }
}

impl Default for InMemoryHashIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl HashIndex for InMemoryHashIndex {
    fn get_sha1(&self, blake3: &ContentHash) -> Option<GitHash> {
        self.blake3_to_sha1.get(blake3).copied()
    }

    fn get_blake3(&self, sha1: &GitHash) -> Option<ContentHash> {
        self.sha1_to_blake3.get(sha1).copied()
    }

    fn insert(&mut self, mapping: HashMapping) -> bool {
        let is_new = !self.blake3_to_sha1.contains_key(&mapping.blake3);
        self.blake3_to_sha1.insert(mapping.blake3, mapping.sha1);
        self.sha1_to_blake3.insert(mapping.sha1, mapping.blake3);
        is_new
    }

    fn remove_by_blake3(&mut self, blake3: &ContentHash) -> Option<HashMapping> {
        if let Some(sha1) = self.blake3_to_sha1.remove(blake3) {
            self.sha1_to_blake3.remove(&sha1);
            Some(HashMapping::new(*blake3, sha1))
        } else {
            None
        }
    }

    fn remove_by_sha1(&mut self, sha1: &GitHash) -> Option<HashMapping> {
        if let Some(blake3) = self.sha1_to_blake3.remove(sha1) {
            self.blake3_to_sha1.remove(&blake3);
            Some(HashMapping::new(blake3, *sha1))
        } else {
            None
        }
    }

    fn len(&self) -> usize {
        self.blake3_to_sha1.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_protocol::core::hash::hash_bytes;

    #[test]
    fn empty_index() {
        let index = InMemoryHashIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn insert_and_lookup_roundtrip() {
        let mut index = InMemoryHashIndex::new();

        let sha1: GitHash = "da39a3ee5e6b4b0d3255bfef95601890afd80709".parse().unwrap();
        let blake3 = hash_bytes(b"hello worktree");

        assert!(index.insert(HashMapping::new(blake3, sha1)));
        assert!(!index.insert(HashMapping::new(blake3, sha1)));

        assert_eq!(index.len(), 1);
        assert_eq!(index.get_sha1(&blake3), Some(sha1));
        assert_eq!(index.get_blake3(&sha1), Some(blake3));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let index = InMemoryHashIndex::new();
        assert_eq!(index.get_blake3(&GitHash::ZERO), None);
        assert_eq!(index.get_sha1(&ContentHash::ZERO), None);
    }

    #[test]
    fn remove_by_blake3() {
        let mut index = InMemoryHashIndex::new();
        let sha1: GitHash = "da39a3ee5e6b4b0d3255bfef95601890afd80709".parse().unwrap();
        let blake3 = hash_bytes(b"hello worktree");
        index.insert(HashMapping::new(blake3, sha1));

        let removed = index.remove_by_blake3(&blake3).unwrap();
        assert_eq!(removed.sha1, sha1);
        assert_eq!(removed.blake3, blake3);
        assert_eq!(index.len(), 0);
        assert_eq!(index.get_sha1(&blake3), None);
        assert_eq!(index.get_blake3(&sha1), None);
    }

    #[test]
    fn remove_by_sha1() {
        let mut index = InMemoryHashIndex::new();
        let sha1: GitHash = "da39a3ee5e6b4b0d3255bfef95601890afd80709".parse().unwrap();
        let blake3 = hash_bytes(b"hello worktree");
        index.insert(HashMapping::new(blake3, sha1));

        let removed = index.remove_by_sha1(&sha1).unwrap();
        assert_eq!(removed.sha1, sha1);
        assert_eq!(removed.blake3, blake3);
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn remove_missing_returns_none() {
        let mut index = InMemoryHashIndex::new();
        assert!(index.remove_by_blake3(&ContentHash::ZERO).is_none());
        assert!(index.remove_by_sha1(&GitHash::ZERO).is_none());
    }
}
