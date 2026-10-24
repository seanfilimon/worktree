//! Git hash bridging.
//!
//! Provides [`GitHash`], [`HashMapping`], and the [`HashIndex`] trait for
//! bidirectional mapping between Worktree's BLAKE3 content hashes and Git's
//! SHA-1 object hashes.
//!
//! This module is the low-level hash bridging layer used by the higher-level
//! [`super::git_object_map`] module. It focuses purely on hash-to-hash
//! correspondence without carrying object kind or size metadata.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use crate::core::hash::ContentHash;

// ---------------------------------------------------------------------------
// GitHash
// ---------------------------------------------------------------------------

/// A Git SHA-1 hash stored as a 20-byte array.
///
/// This is the raw binary representation of Git's SHA-1 object identifier.
/// It can be converted to/from the standard 40-character lowercase hex string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GitHash([u8; 20]);

impl GitHash {
    /// The zero hash (all bytes zero).
    pub const ZERO: GitHash = GitHash([0u8; 20]);

    /// Construct from a raw 20-byte array.
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        GitHash(bytes)
    }

    /// Return a reference to the underlying byte array.
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.0
    }

    /// Encode the hash as a lowercase 40-character hexadecimal string.
    pub fn to_hex(&self) -> String {
        let mut s = String::with_capacity(40);
        for byte in &self.0 {
            s.push_str(&format!("{:02x}", byte));
        }
        s
    }

    /// Parse a 40-character hex string into a `GitHash`.
    pub fn from_hex(hex: &str) -> Result<Self, GitHashParseError> {
        hex.parse()
    }

    /// Returns `true` if this is the zero hash.
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 20]
    }
}

impl Default for GitHash {
    fn default() -> Self {
        GitHash::ZERO
    }
}

impl fmt::Display for GitHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for byte in &self.0 {
            write!(f, "{:02x}", byte)?;
        }
        Ok(())
    }
}

/// Error returned when parsing a hex string into a [`GitHash`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitHashParseError {
    /// The hex string had an unexpected length (expected 40 characters).
    InvalidLength(usize),
    /// The string contained non-hex characters.
    InvalidHex,
}

impl fmt::Display for GitHashParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GitHashParseError::InvalidLength(len) => {
                write!(f, "expected 40 hex characters, got {}", len)
            }
            GitHashParseError::InvalidHex => write!(f, "invalid hex character"),
        }
    }
}

impl std::error::Error for GitHashParseError {}

impl FromStr for GitHash {
    type Err = GitHashParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 40 {
            return Err(GitHashParseError::InvalidLength(s.len()));
        }
        let mut bytes = [0u8; 20];
        for i in 0..20 {
            let hex_byte = &s[i * 2..i * 2 + 2];
            bytes[i] = u8::from_str_radix(hex_byte, 16)
                .map_err(|_| GitHashParseError::InvalidHex)?;
        }
        Ok(GitHash(bytes))
    }
}

// ---------------------------------------------------------------------------
// HashMapping
// ---------------------------------------------------------------------------

/// A single mapping between a Worktree BLAKE3 content hash and a Git SHA-1 hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HashMapping {
    /// The Worktree BLAKE3 content hash (32 bytes).
    pub blake3: ContentHash,
    /// The Git SHA-1 hash (20 bytes).
    pub sha1: GitHash,
}

impl HashMapping {
    /// Create a new hash mapping.
    pub fn new(blake3: ContentHash, sha1: GitHash) -> Self {
        Self { blake3, sha1 }
    }

    /// Create a mapping from a BLAKE3 hash and a 40-character SHA-1 hex string.
    ///
    /// Returns `None` if the hex string is not a valid SHA-1 hash.
    pub fn from_hex(blake3: ContentHash, sha1_hex: &str) -> Option<Self> {
        let sha1 = GitHash::from_hex(sha1_hex).ok()?;
        Some(Self { blake3, sha1 })
    }
}

impl fmt::Display for HashMapping {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "blake3:{} <-> sha1:{}", self.blake3, self.sha1)
    }
}

// ---------------------------------------------------------------------------
// HashIndex trait
// ---------------------------------------------------------------------------

/// A trait for bidirectional hash lookup between BLAKE3 and SHA-1.
///
/// Implementations can use in-memory maps, on-disk databases, or any other
/// storage backend. The trait provides the minimal interface needed by the
/// compatibility layer.
pub trait HashIndex {
    /// Look up the Git SHA-1 hash for a given BLAKE3 content hash.
    fn get_sha1(&self, blake3: &ContentHash) -> Option<GitHash>;

    /// Look up the BLAKE3 content hash for a given Git SHA-1 hash.
    fn get_blake3(&self, sha1: &GitHash) -> Option<ContentHash>;

    /// Insert a hash mapping. Returns `true` if the mapping was newly inserted,
    /// `false` if it already existed.
    fn insert(&mut self, mapping: HashMapping) -> bool;

    /// Remove a mapping by its BLAKE3 hash. Returns the removed mapping if
    /// it existed.
    fn remove_by_blake3(&mut self, blake3: &ContentHash) -> Option<HashMapping>;

    /// Remove a mapping by its Git SHA-1 hash. Returns the removed mapping if
    /// it existed.
    fn remove_by_sha1(&mut self, sha1: &GitHash) -> Option<HashMapping>;

    /// Returns `true` if the index contains a mapping for the given BLAKE3 hash.
    fn contains_blake3(&self, blake3: &ContentHash) -> bool {
        self.get_sha1(blake3).is_some()
    }

    /// Returns `true` if the index contains a mapping for the given SHA-1 hash.
    fn contains_sha1(&self, sha1: &GitHash) -> bool {
        self.get_blake3(sha1).is_some()
    }

    /// Returns the number of mappings in the index.
    fn len(&self) -> usize;

    /// Returns `true` if the index is empty.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// InMemoryHashIndex
// ---------------------------------------------------------------------------

/// An in-memory implementation of [`HashIndex`] backed by two `HashMap`s.
///
/// This is suitable for tests and small-scale usage. For production use with
/// large repositories, a persistent on-disk implementation should be used.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InMemoryHashIndex {
    blake3_to_sha1: HashMap<ContentHash, GitHash>,
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

    /// Create an in-memory hash index pre-populated with the given mappings.
    pub fn from_mappings(mappings: impl IntoIterator<Item = HashMapping>) -> Self {
        let mut index = Self::new();
        for mapping in mappings {
            index.insert(mapping);
        }
        index
    }

    /// Iterate over all mappings in the index.
    pub fn iter(&self) -> impl Iterator<Item = HashMapping> + '_ {
        self.blake3_to_sha1
            .iter()
            .map(|(blake3, sha1)| HashMapping::new(*blake3, *sha1))
    }

    /// Return all mappings as a `Vec`.
    pub fn all_mappings(&self) -> Vec<HashMapping> {
        self.iter().collect()
    }

    /// Drain all mappings from the index, returning them.
    pub fn drain(&mut self) -> Vec<HashMapping> {
        let mappings: Vec<HashMapping> = self.iter().collect();
        self.blake3_to_sha1.clear();
        self.sha1_to_blake3.clear();
        mappings
    }

    /// Merge another index into this one.
    ///
    /// On conflict (same BLAKE3 or SHA-1 with different counterpart), the
    /// incoming mapping overwrites the existing one.
    pub fn merge(&mut self, other: &InMemoryHashIndex) {
        for mapping in other.iter() {
            self.insert(mapping);
        }
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
    use crate::core::hash::hash_bytes;

    fn sample_git_hash() -> GitHash {
        GitHash::from_hex("aabbccdd00112233445566778899aabbccddeeff").unwrap()
    }

    fn sample_git_hash_2() -> GitHash {
        GitHash::from_hex("1111111111111111111111111111111111111111").unwrap()
    }

    fn sample_mapping() -> HashMapping {
        HashMapping::new(hash_bytes(b"worktree content"), sample_git_hash())
    }

    fn sample_mapping_2() -> HashMapping {
        HashMapping::new(hash_bytes(b"second content"), sample_git_hash_2())
    }

    // ── GitHash ─────────────────────────────────────────────────────────

    #[test]
    fn test_git_hash_from_bytes() {
        let bytes = [42u8; 20];
        let h = GitHash::from_bytes(bytes);
        assert_eq!(h.as_bytes(), &bytes);
    }

    #[test]
    fn test_git_hash_zero() {
        let h = GitHash::ZERO;
        assert!(h.is_zero());
        assert_eq!(h.as_bytes(), &[0u8; 20]);
        assert_eq!(h.to_hex(), "0000000000000000000000000000000000000000");
    }

    #[test]
    fn test_git_hash_default_is_zero() {
        assert_eq!(GitHash::default(), GitHash::ZERO);
    }

    #[test]
    fn test_git_hash_to_hex() {
        let h = sample_git_hash();
        assert_eq!(h.to_hex(), "aabbccdd00112233445566778899aabbccddeeff");
        assert_eq!(h.to_hex().len(), 40);
    }

    #[test]
    fn test_git_hash_display() {
        let h = sample_git_hash();
        let display = format!("{}", h);
        assert_eq!(display, "aabbccdd00112233445566778899aabbccddeeff");
    }

    #[test]
    fn test_git_hash_from_hex() {
        let hex = "aabbccdd00112233445566778899aabbccddeeff";
        let h = GitHash::from_hex(hex).unwrap();
        assert_eq!(h.to_hex(), hex);
    }

    #[test]
    fn test_git_hash_roundtrip_hex() {
        let h = sample_git_hash();
        let hex = h.to_hex();
        let parsed: GitHash = hex.parse().unwrap();
        assert_eq!(h, parsed);
    }

    #[test]
    fn test_git_hash_display_roundtrip() {
        let h = sample_git_hash();
        let display = format!("{}", h);
        let parsed: GitHash = display.parse().unwrap();
        assert_eq!(h, parsed);
    }

    #[test]
    fn test_git_hash_from_str_invalid_length() {
        let result = "abcd".parse::<GitHash>();
        assert!(result.is_err());
        match result.unwrap_err() {
            GitHashParseError::InvalidLength(len) => assert_eq!(len, 4),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn test_git_hash_from_str_invalid_hex() {
        let bad = "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz";
        assert_eq!(bad.len(), 40);
        let result = bad.parse::<GitHash>();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), GitHashParseError::InvalidHex);
    }

    #[test]
    fn test_git_hash_is_not_zero() {
        let h = sample_git_hash();
        assert!(!h.is_zero());
    }

    #[test]
    fn test_git_hash_ordering() {
        let h1 = GitHash::from_hex("0000000000000000000000000000000000000001").unwrap();
        let h2 = GitHash::from_hex("0000000000000000000000000000000000000002").unwrap();
        assert!(h1 < h2);
        assert!(h2 > h1);
    }

    #[test]
    fn test_git_hash_clone_copy() {
        let h = sample_git_hash();
        let copy = h;
        assert_eq!(h, copy);
        let clone = h.clone();
        assert_eq!(h, clone);
    }

    #[test]
    fn test_git_hash_hash_set() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(sample_git_hash());
        set.insert(sample_git_hash());
        set.insert(sample_git_hash_2());
        assert_eq!(set.len(), 2);
    }

    #[test]
    fn test_git_hash_serde_json_roundtrip() {
        let h = sample_git_hash();
        let json = serde_json::to_string(&h).expect("serialize");
        let deserialized: GitHash = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(h, deserialized);
    }

    #[test]
    fn test_git_hash_serde_bincode_roundtrip() {
        let h = sample_git_hash();
        let encoded = bincode::serialize(&h).expect("serialize");
        let decoded: GitHash = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(h, decoded);
    }

    #[test]
    fn test_git_hash_parse_error_display() {
        let err = GitHashParseError::InvalidLength(10);
        assert_eq!(err.to_string(), "expected 40 hex characters, got 10");

        let err = GitHashParseError::InvalidHex;
        assert_eq!(err.to_string(), "invalid hex character");
    }

    // ── HashMapping ─────────────────────────────────────────────────────

    #[test]
    fn test_hash_mapping_new() {
        let blake3 = hash_bytes(b"test");
        let sha1 = sample_git_hash();
        let m = HashMapping::new(blake3, sha1);
        assert_eq!(m.blake3, blake3);
        assert_eq!(m.sha1, sha1);
    }

    #[test]
    fn test_hash_mapping_from_hex() {
        let blake3 = hash_bytes(b"test");
        let m = HashMapping::from_hex(blake3, "aabbccdd00112233445566778899aabbccddeeff");
        assert!(m.is_some());
        let m = m.unwrap();
        assert_eq!(m.blake3, blake3);
        assert_eq!(m.sha1, sample_git_hash());
    }

    #[test]
    fn test_hash_mapping_from_hex_invalid() {
        let blake3 = hash_bytes(b"test");
        let m = HashMapping::from_hex(blake3, "invalid");
        assert!(m.is_none());
    }

    #[test]
    fn test_hash_mapping_display() {
        let m = sample_mapping();
        let display = m.to_string();
        assert!(display.contains("blake3:"));
        assert!(display.contains("sha1:"));
        assert!(display.contains("<->"));
    }

    #[test]
    fn test_hash_mapping_equality() {
        let m1 = sample_mapping();
        let m2 = sample_mapping();
        assert_eq!(m1, m2);

        let m3 = sample_mapping_2();
        assert_ne!(m1, m3);
    }

    #[test]
    fn test_hash_mapping_clone_copy() {
        let m = sample_mapping();
        let copy = m;
        assert_eq!(m, copy);
    }

    #[test]
    fn test_hash_mapping_serde_json_roundtrip() {
        let m = sample_mapping();
        let json = serde_json::to_string(&m).expect("serialize");
        let deserialized: HashMapping = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(m, deserialized);
    }

    #[test]
    fn test_hash_mapping_serde_bincode_roundtrip() {
        let m = sample_mapping();
        let encoded = bincode::serialize(&m).expect("serialize");
        let decoded: HashMapping = bincode::deserialize(&encoded).expect("deserialize");
        assert_eq!(m, decoded);
    }

    #[test]
    fn test_hash_mapping_hash() {
        use std::collections::HashSet;
        let mut set = HashSet::new();
        set.insert(sample_mapping());
        set.insert(sample_mapping());
        set.insert(sample_mapping_2());
        assert_eq!(set.len(), 2);
    }

    // ── InMemoryHashIndex ───────────────────────────────────────────────

    #[test]
    fn test_index_new_is_empty() {
        let index = InMemoryHashIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);
    }

    #[test]
    fn test_index_default_is_empty() {
        let index = InMemoryHashIndex::default();
        assert!(index.is_empty());
    }

    #[test]
    fn test_index_insert_and_lookup() {
        let mut index = InMemoryHashIndex::new();
        let m = sample_mapping();

        let is_new = index.insert(m);
        assert!(is_new);
        assert_eq!(index.len(), 1);
        assert!(!index.is_empty());

        let sha1 = index.get_sha1(&m.blake3);
        assert_eq!(sha1, Some(m.sha1));

        let blake3 = index.get_blake3(&m.sha1);
        assert_eq!(blake3, Some(m.blake3));
    }

    #[test]
    fn test_index_insert_duplicate_returns_false() {
        let mut index = InMemoryHashIndex::new();
        let m = sample_mapping();

        let is_new = index.insert(m);
        assert!(is_new);

        let is_new = index.insert(m);
        assert!(!is_new);
        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_index_contains() {
        let mut index = InMemoryHashIndex::new();
        let m = sample_mapping();

        assert!(!index.contains_blake3(&m.blake3));
        assert!(!index.contains_sha1(&m.sha1));

        index.insert(m);

        assert!(index.contains_blake3(&m.blake3));
        assert!(index.contains_sha1(&m.sha1));
    }

    #[test]
    fn test_index_remove_by_blake3() {
        let mut index = InMemoryHashIndex::new();
        let m = sample_mapping();

        index.insert(m);
        assert_eq!(index.len(), 1);

        let removed = index.remove_by_blake3(&m.blake3);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap(), m);
        assert!(index.is_empty());
        assert!(!index.contains_sha1(&m.sha1));
    }

    #[test]
    fn test_index_remove_by_sha1() {
        let mut index = InMemoryHashIndex::new();
        let m = sample_mapping();

        index.insert(m);
        assert_eq!(index.len(), 1);

        let removed = index.remove_by_sha1(&m.sha1);
        assert!(removed.is_some());
        assert_eq!(removed.unwrap(), m);
        assert!(index.is_empty());
        assert!(!index.contains_blake3(&m.blake3));
    }

    #[test]
    fn test_index_remove_nonexistent() {
        let mut index = InMemoryHashIndex::new();
        let blake3 = hash_bytes(b"nope");
        let sha1 = sample_git_hash();

        assert!(index.remove_by_blake3(&blake3).is_none());
        assert!(index.remove_by_sha1(&sha1).is_none());
    }

    #[test]
    fn test_index_multiple_entries() {
        let mut index = InMemoryHashIndex::new();
        let m1 = sample_mapping();
        let m2 = sample_mapping_2();

        index.insert(m1);
        index.insert(m2);
        assert_eq!(index.len(), 2);

        assert_eq!(index.get_sha1(&m1.blake3), Some(m1.sha1));
        assert_eq!(index.get_sha1(&m2.blake3), Some(m2.sha1));

        assert_eq!(index.get_blake3(&m1.sha1), Some(m1.blake3));
        assert_eq!(index.get_blake3(&m2.sha1), Some(m2.blake3));
    }

    #[test]
    fn test_index_overwrite() {
        let mut index = InMemoryHashIndex::new();
        let blake3 = hash_bytes(b"content");
        let sha1_a = sample_git_hash();
        let sha1_b = sample_git_hash_2();

        let m1 = HashMapping::new(blake3, sha1_a);
        let m2 = HashMapping::new(blake3, sha1_b);

        index.insert(m1);
        assert_eq!(index.get_sha1(&blake3), Some(sha1_a));

        index.insert(m2);
        assert_eq!(index.get_sha1(&blake3), Some(sha1_b));
    }

    #[test]
    fn test_index_from_mappings() {
        let m1 = sample_mapping();
        let m2 = sample_mapping_2();

        let index = InMemoryHashIndex::from_mappings(vec![m1, m2]);
        assert_eq!(index.len(), 2);
        assert!(index.contains_blake3(&m1.blake3));
        assert!(index.contains_blake3(&m2.blake3));
    }

    #[test]
    fn test_index_iter() {
        let mut index = InMemoryHashIndex::new();
        let m1 = sample_mapping();
        let m2 = sample_mapping_2();

        index.insert(m1);
        index.insert(m2);

        let all: Vec<HashMapping> = index.iter().collect();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&m1));
        assert!(all.contains(&m2));
    }

    #[test]
    fn test_index_all_mappings() {
        let mut index = InMemoryHashIndex::new();
        let m1 = sample_mapping();
        let m2 = sample_mapping_2();

        index.insert(m1);
        index.insert(m2);

        let all = index.all_mappings();
        assert_eq!(all.len(), 2);
        assert!(all.contains(&m1));
        assert!(all.contains(&m2));
    }

    #[test]
    fn test_index_drain() {
        let mut index = InMemoryHashIndex::new();
        let m1 = sample_mapping();
        let m2 = sample_mapping_2();

        index.insert(m1);
        index.insert(m2);
        assert_eq!(index.len(), 2);

        let drained = index.drain();
        assert_eq!(drained.len(), 2);
        assert!(index.is_empty());
        assert!(drained.contains(&m1));
        assert!(drained.contains(&m2));
    }

    #[test]
    fn test_index_merge() {
        let mut index1 = InMemoryHashIndex::new();
        let m1 = sample_mapping();
        index1.insert(m1);

        let mut index2 = InMemoryHashIndex::new();
        let m2 = sample_mapping_2();
        index2.insert(m2);

        index1.merge(&index2);
        assert_eq!(index1.len(), 2);
        assert!(index1.contains_blake3(&m1.blake3));
        assert!(index1.contains_blake3(&m2.blake3));
    }

    #[test]
    fn test_index_merge_overwrite() {
        let blake3 = hash_bytes(b"shared content");
        let sha1_a = sample_git_hash();
        let sha1_b = sample_git_hash_2();

        let mut index1 = InMemoryHashIndex::new();
        index1.insert(HashMapping::new(blake3, sha1_a));

        let mut index2 = InMemoryHashIndex::new();
        index2.insert(HashMapping::new(blake3, sha1_b));

        index1.merge(&index2);
        // The merge should overwrite with index2's value.
        assert_eq!(index1.get_sha1(&blake3), Some(sha1_b));
    }

    #[test]
    fn test_index_serde_json_roundtrip() {
        let mut index = InMemoryHashIndex::new();
        index.insert(sample_mapping());
        index.insert(sample_mapping_2());

        let json = serde_json::to_string(&index).expect("serialize");
        let deserialized: InMemoryHashIndex =
            serde_json::from_str(&json).expect("deserialize");

        assert_eq!(deserialized.len(), 2);
        assert!(deserialized.contains_blake3(&sample_mapping().blake3));
        assert!(deserialized.contains_blake3(&sample_mapping_2().blake3));
    }

    #[test]
    fn test_index_serde_bincode_roundtrip() {
        let mut index = InMemoryHashIndex::new();
        index.insert(sample_mapping());

        let encoded = bincode::serialize(&index).expect("serialize");
        let decoded: InMemoryHashIndex =
            bincode::deserialize(&encoded).expect("deserialize");

        assert_eq!(decoded.len(), 1);
        let m = sample_mapping();
        assert_eq!(decoded.get_sha1(&m.blake3), Some(m.sha1));
    }

    #[test]
    fn test_lookup_nonexistent_returns_none() {
        let index = InMemoryHashIndex::new();
        let blake3 = hash_bytes(b"nonexistent");
        let sha1 = sample_git_hash();

        assert!(index.get_sha1(&blake3).is_none());
        assert!(index.get_blake3(&sha1).is_none());
    }

    #[test]
    fn test_trait_default_methods() {
        let mut index = InMemoryHashIndex::new();
        let m = sample_mapping();

        // Test contains_blake3 and contains_sha1 (default implementations)
        assert!(!index.contains_blake3(&m.blake3));
        assert!(!index.contains_sha1(&m.sha1));
        assert!(index.is_empty());

        index.insert(m);

        assert!(index.contains_blake3(&m.blake3));
        assert!(index.contains_sha1(&m.sha1));
        assert!(!index.is_empty());
    }

    #[test]
    fn test_bidirectional_consistency() {
        let mut index = InMemoryHashIndex::new();
        let m1 = sample_mapping();
        let m2 = sample_mapping_2();

        index.insert(m1);
        index.insert(m2);

        // For every blake3 -> sha1 mapping, the reverse must also hold.
        for mapping in index.iter() {
            let sha1 = index.get_sha1(&mapping.blake3).unwrap();
            let blake3 = index.get_blake3(&sha1).unwrap();
            assert_eq!(blake3, mapping.blake3);
        }
    }

    #[test]
    fn test_remove_maintains_bidirectional_consistency() {
        let mut index = InMemoryHashIndex::new();
        let m1 = sample_mapping();
        let m2 = sample_mapping_2();

        index.insert(m1);
        index.insert(m2);

        index.remove_by_blake3(&m1.blake3);

        // m1 should be gone from both directions.
        assert!(index.get_sha1(&m1.blake3).is_none());
        assert!(index.get_blake3(&m1.sha1).is_none());

        // m2 should still be present.
        assert_eq!(index.get_sha1(&m2.blake3), Some(m2.sha1));
        assert_eq!(index.get_blake3(&m2.sha1), Some(m2.blake3));
    }

    #[test]
    fn test_large_index() {
        let mut index = InMemoryHashIndex::new();
        let mut mappings = Vec::new();

        for i in 0u32..100 {
            let blake3 = hash_bytes(&i.to_le_bytes());
            let mut sha1_bytes = [0u8; 20];
            sha1_bytes[0..4].copy_from_slice(&i.to_le_bytes());
            let sha1 = GitHash::from_bytes(sha1_bytes);
            let m = HashMapping::new(blake3, sha1);
            mappings.push(m);
            index.insert(m);
        }

        assert_eq!(index.len(), 100);

        for m in &mappings {
            assert_eq!(index.get_sha1(&m.blake3), Some(m.sha1));
            assert_eq!(index.get_blake3(&m.sha1), Some(m.blake3));
        }
    }
}
