//! Content-addressed object store under `.wt/objects/`.
//!
//! Objects are stored raw (uncompressed) at `objects/<hh>/<62 hex>` where the
//! hash is BLAKE3 of the exact bytes. The same bytes and hashes are used on
//! the wire and on the server, so push/pull is a byte copy.

use std::path::{Path, PathBuf};

use crate::error::Result;

pub use worktree_protocol::core::hash::ContentHash as ObjectHash;

/// BLAKE3 hash of `data`.
pub fn hash_bytes(data: &[u8]) -> ObjectHash {
    worktree_protocol::core::hash::hash_bytes(data)
}

#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

impl ObjectStore {
    /// Open (or lazily create) a store rooted at `root` (e.g. `.wt/objects`).
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Store `data`, returning its hash. Idempotent; writes are atomic
    /// (temp file + rename) and safe under concurrent writers.
    pub fn put(&self, data: &[u8]) -> Result<ObjectHash> {
        let _ = data;
        todo!("sdk-core: ObjectStore::put")
    }

    /// Store `data` under an expected hash; `CorruptObject` on mismatch.
    pub fn put_with_hash(&self, hash: &ObjectHash, data: &[u8]) -> Result<()> {
        let _ = (hash, data);
        todo!("sdk-core: ObjectStore::put_with_hash")
    }

    /// Read an object; `ObjectNotFound` if absent.
    pub fn get(&self, hash: &ObjectHash) -> Result<Vec<u8>> {
        let _ = hash;
        todo!("sdk-core: ObjectStore::get")
    }

    pub fn has(&self, hash: &ObjectHash) -> bool {
        let _ = hash;
        todo!("sdk-core: ObjectStore::has")
    }

    /// Filesystem path where `hash` is (or would be) stored.
    pub fn path_of(&self, hash: &ObjectHash) -> PathBuf {
        let _ = hash;
        todo!("sdk-core: ObjectStore::path_of")
    }

    /// All stored object hashes (unordered).
    pub fn list(&self) -> Result<Vec<ObjectHash>> {
        todo!("sdk-core: ObjectStore::list")
    }

    /// (object count, total bytes).
    pub fn stats(&self) -> Result<(u64, u64)> {
        todo!("sdk-core: ObjectStore::stats")
    }
}
