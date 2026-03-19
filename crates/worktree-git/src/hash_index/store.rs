use std::collections::HashMap;
use std::fmt;

use worktree_protocol::compat::git_hash_map::{GitHash, HashIndex, HashMapping};
use worktree_protocol::core::hash::ContentHash;

/// Error type for in-memory hash index operations.
#[derive(Debug, Clone)]
pub struct HashIndexError(pub String);

impl fmt::Display for HashIndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "hash index error: {}", self.0)
    }
}

impl std::error::Error for HashIndexError {}

/// An in-memory bidirectional index mapping Git SHA-1 hashes to Worktree BLAKE3
/// content hashes and vice versa.
///
/// This implementation uses two `HashMap`s for O(1) lookups in both directions.
/// It is suitable for moderate-sized repositories; for very large repos a
/// persistent on-disk index would be preferable.
pub struct InMemoryHashIndex {
    /// Maps Git SHA-1 → Worktree BLAKE3.
    git_to_content: HashMap<GitHash, ContentHash>,
    /// Maps Worktree BLAKE3 → Git SHA-1.
    content_to_git: HashMap<ContentHash, GitHash>,
}

impl InMemoryHashIndex {
    /// Create a new, empty in-memory hash index.
    pub fn new() -> Self {
        Self {
            git_to_content: HashMap::new(),
            content_to_git: HashMap::new(),
        }
    }

    /// Return the number of mappings currently stored.
    pub fn len(&self) -> usize {
        self.git_to_content.len()
    }

    /// Return `true` if the index contains no mappings.
    pub fn is_empty(&self) -> bool {
        self.git_to_content.is_empty()
    }
}

impl Default for InMemoryHashIndex {
    fn default() -> Self {
        Self::new()
    }
}

impl HashIndex for InMemoryHashIndex {
    type Error = HashIndexError;

    fn insert(&mut self, mapping: HashMapping) -> Result<(), Self::Error> {
        self.git_to_content
            .insert(mapping.git_hash, mapping.content_hash);
        self.content_to_git
            .insert(mapping.content_hash, mapping.git_hash);
        Ok(())
    }

    fn lookup_by_git(&self, git: &GitHash) -> Option<ContentHash> {
        self.git_to_content.get(git).copied()
    }

    fn lookup_by_content(&self, content: &ContentHash) -> Option<GitHash> {
        self.content_to_git.get(content).copied()
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

        let git_hash: GitHash = "da39a3ee5e6b4b0d3255bfef95601890afd80709"
            .parse()
            .unwrap();
        let content_hash = hash_bytes(b"hello worktree");

        index
            .insert(HashMapping::new(git_hash, content_hash))
            .unwrap();

        assert_eq!(index.len(), 1);
        assert_eq!(index.lookup_by_git(&git_hash), Some(content_hash));
        assert_eq!(index.lookup_by_content(&content_hash), Some(git_hash));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let index = InMemoryHashIndex::new();
        assert_eq!(index.lookup_by_git(&GitHash::ZERO), None);
        assert_eq!(index.lookup_by_content(&ContentHash::ZERO), None);
    }
}
