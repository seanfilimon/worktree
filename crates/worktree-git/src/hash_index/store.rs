use std::collections::HashMap;

use git2::Oid;
use worktree_protocol::compat::git_hash_map::{GitHash, HashIndex, HashMapping};
use worktree_protocol::core::hash::ContentHash;

use crate::error::{GitCompatError, Result};

/// An in-memory bidirectional index mapping Git SHA-1 hashes to Worktree BLAKE3
/// content hashes and vice versa.
///
/// This implementation uses two `HashMap`s for O(1) lookups in both directions.
/// Export uses it to write each distinct blob to Git only once.
#[derive(Debug, Default)]
pub struct InMemoryHashIndex {
    /// Maps Worktree BLAKE3 -> Git SHA-1.
    blake3_to_sha1: HashMap<ContentHash, GitHash>,
    /// Maps Git SHA-1 -> Worktree BLAKE3.
    sha1_to_blake3: HashMap<GitHash, ContentHash>,
}

impl InMemoryHashIndex {
    /// Create a new, empty in-memory hash index.
    pub fn new() -> Self {
        Self::default()
    }

    /// Git object id recorded for `blake3`.
    pub fn oid_for(&self, blake3: &ContentHash) -> Option<Oid> {
        self.get_sha1(blake3).map(|sha1| oid_from_git_hash(&sha1))
    }

    /// Record that `blake3` corresponds to the Git object `oid`.
    pub fn insert_oid(&mut self, blake3: ContentHash, oid: Oid) -> Result<bool> {
        Ok(self.insert(HashMapping::new(blake3, git_hash_from_oid(oid)?)))
    }
}

/// Convert a libgit2 object id into a protocol [`GitHash`] (SHA-1 only).
pub fn git_hash_from_oid(oid: Oid) -> Result<GitHash> {
    let bytes: [u8; 20] = oid.as_bytes().try_into().map_err(|_| {
        GitCompatError::HashIndexError(format!("{oid} is not a 20-byte SHA-1 object id"))
    })?;
    Ok(GitHash::from_bytes(bytes))
}

/// Convert a protocol [`GitHash`] into a libgit2 object id.
pub fn oid_from_git_hash(hash: &GitHash) -> Oid {
    // A 20-byte slice is always a valid SHA-1 oid.
    Oid::from_bytes(hash.as_bytes()).unwrap_or_else(|_| Oid::zero())
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
        let sha1 = self.blake3_to_sha1.remove(blake3)?;
        self.sha1_to_blake3.remove(&sha1);
        Some(HashMapping::new(*blake3, sha1))
    }

    fn remove_by_sha1(&mut self, sha1: &GitHash) -> Option<HashMapping> {
        let blake3 = self.sha1_to_blake3.remove(sha1)?;
        self.blake3_to_sha1.remove(&blake3);
        Some(HashMapping::new(blake3, *sha1))
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

        let is_new = index.insert(HashMapping::new(blake3, sha1));
        assert!(is_new);

        assert_eq!(index.len(), 1);
        assert_eq!(index.get_sha1(&blake3), Some(sha1));
        assert_eq!(index.get_blake3(&sha1), Some(blake3));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let index = InMemoryHashIndex::new();
        assert_eq!(index.get_sha1(&ContentHash::ZERO), None);
        assert_eq!(index.get_blake3(&GitHash::ZERO), None);
        assert_eq!(index.oid_for(&ContentHash::ZERO), None);
    }

    #[test]
    fn remove_in_both_directions() {
        let mut index = InMemoryHashIndex::new();
        let a = hash_bytes(b"a");
        let b = hash_bytes(b"b");
        let sha_a: GitHash = "1111111111111111111111111111111111111111".parse().unwrap();
        let sha_b: GitHash = "2222222222222222222222222222222222222222".parse().unwrap();
        index.insert(HashMapping::new(a, sha_a));
        index.insert(HashMapping::new(b, sha_b));

        assert_eq!(index.remove_by_blake3(&a), Some(HashMapping::new(a, sha_a)));
        assert_eq!(index.get_blake3(&sha_a), None);
        assert_eq!(
            index.remove_by_sha1(&sha_b),
            Some(HashMapping::new(b, sha_b))
        );
        assert_eq!(index.get_sha1(&b), None);
        assert!(index.is_empty());
    }

    #[test]
    fn oid_conversions_round_trip() {
        let oid = Oid::from_str("da39a3ee5e6b4b0d3255bfef95601890afd80709").unwrap();
        let hash = git_hash_from_oid(oid).unwrap();
        assert_eq!(hash.to_hex(), oid.to_string());
        assert_eq!(oid_from_git_hash(&hash), oid);

        let mut index = InMemoryHashIndex::new();
        let blake3 = hash_bytes(b"content");
        assert!(index.insert_oid(blake3, oid).unwrap());
        assert!(!index.insert_oid(blake3, oid).unwrap());
        assert_eq!(index.oid_for(&blake3), Some(oid));
    }
}
