use std::collections::HashMap;

use worktree_protocol::core::hash::ContentHash;

/// An in-memory index that maps content hashes to object kind labels
/// (e.g. `"blob"`, `"manifest"`, `"snapshot"`).
///
/// The `ObjectIndex` provides fast lookups to determine whether a given
/// content-addressed object already exists in storage and what type it is,
/// without having to read the object data from disk.
///
/// In the future this will be backed by a persistent on-disk index (e.g.
/// an append-only log or SQLite database). For now it is purely in-memory.
pub struct ObjectIndex {
    /// Maps content hashes to their object kind string.
    entries: HashMap<ContentHash, String>,
}

impl ObjectIndex {
    /// Create a new, empty `ObjectIndex`.
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
        }
    }

    /// Insert a mapping from `hash` to `kind` into the index.
    ///
    /// If the hash already exists in the index, the kind is updated to the
    /// new value.
    ///
    /// # Arguments
    ///
    /// * `hash` — The content hash of the stored object.
    /// * `kind` — A label describing the object type (e.g. `"blob"`,
    ///   `"manifest"`, `"snapshot"`, `"delta"`).
    pub fn insert(&mut self, hash: ContentHash, kind: &str) {
        self.entries.insert(hash, kind.to_string());
    }

    /// Look up the object kind for the given content hash.
    ///
    /// Returns `Some(kind)` if the hash is present in the index, or `None`
    /// if it has not been indexed.
    pub fn lookup(&self, hash: &ContentHash) -> Option<String> {
        self.entries.get(hash).cloned()
    }

    /// Returns the total number of objects tracked by this index.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the index contains the given hash.
    pub fn contains(&self, hash: &ContentHash) -> bool {
        self.entries.contains_key(hash)
    }

    /// Returns `true` if the index is empty.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Remove an entry from the index by hash.
    ///
    /// Returns the kind string if the entry was present, or `None` otherwise.
    pub fn remove(&mut self, hash: &ContentHash) -> Option<String> {
        self.entries.remove(hash)
    }

    /// Clear all entries from the index.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Iterate over all `(hash, kind)` pairs in the index.
    pub fn iter(&self) -> impl Iterator<Item = (&ContentHash, &String)> {
        self.entries.iter()
    }
}

impl Default for ObjectIndex {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_protocol::core::hash::hash_bytes;

    #[test]
    fn insert_and_lookup() {
        let mut index = ObjectIndex::new();
        let hash = hash_bytes(b"hello world");

        index.insert(hash, "blob");
        assert_eq!(index.lookup(&hash), Some("blob".to_string()));
    }

    #[test]
    fn lookup_missing_returns_none() {
        let index = ObjectIndex::new();
        let hash = hash_bytes(b"nonexistent");

        assert_eq!(index.lookup(&hash), None);
    }

    #[test]
    fn count_reflects_entries() {
        let mut index = ObjectIndex::new();
        assert_eq!(index.count(), 0);

        index.insert(hash_bytes(b"a"), "blob");
        index.insert(hash_bytes(b"b"), "manifest");
        assert_eq!(index.count(), 2);
    }

    #[test]
    fn insert_overwrites_existing_kind() {
        let mut index = ObjectIndex::new();
        let hash = hash_bytes(b"data");

        index.insert(hash, "blob");
        index.insert(hash, "manifest");

        assert_eq!(index.lookup(&hash), Some("manifest".to_string()));
        assert_eq!(index.count(), 1);
    }

    #[test]
    fn contains_and_is_empty() {
        let mut index = ObjectIndex::new();
        let hash = hash_bytes(b"test");

        assert!(index.is_empty());
        assert!(!index.contains(&hash));

        index.insert(hash, "blob");
        assert!(!index.is_empty());
        assert!(index.contains(&hash));
    }

    #[test]
    fn remove_entry() {
        let mut index = ObjectIndex::new();
        let hash = hash_bytes(b"remove me");

        index.insert(hash, "blob");
        assert_eq!(index.remove(&hash), Some("blob".to_string()));
        assert!(!index.contains(&hash));
        assert_eq!(index.count(), 0);
    }

    #[test]
    fn clear_removes_all_entries() {
        let mut index = ObjectIndex::new();

        index.insert(hash_bytes(b"a"), "blob");
        index.insert(hash_bytes(b"b"), "snapshot");
        index.insert(hash_bytes(b"c"), "delta");
        assert_eq!(index.count(), 3);

        index.clear();
        assert!(index.is_empty());
    }
}
