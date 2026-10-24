//! Content-addressed object store under `.wt/objects/`.
//!
//! Objects are stored raw (uncompressed) at `objects/<hh>/<62 hex>` where the
//! hash is BLAKE3 of the exact bytes. The same bytes and hashes are used on
//! the wire and on the server, so push/pull is a byte copy.
//!
//! Writes go to a uniquely named temp file inside the fan-out directory and
//! are then renamed into place, so readers never observe partial objects and
//! any number of threads or processes may write concurrently. Because the
//! store is content-addressed, a rename that loses a race against another
//! writer of the same object is harmless and treated as success.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::error::{Result, SdkError};
use crate::repo::fsutil;

pub use worktree_protocol::core::hash::ContentHash as ObjectHash;

/// BLAKE3 hash of `data`.
pub fn hash_bytes(data: &[u8]) -> ObjectHash {
    worktree_protocol::core::hash::hash_bytes(data)
}

/// A directory of immutable, content-addressed objects.
#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

impl ObjectStore {
    /// Open (or lazily create) a store rooted at `root` (e.g. `.wt/objects`).
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// The store's root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Store `data`, returning its hash. Idempotent; writes are atomic
    /// (temp file + rename) and safe under concurrent writers.
    pub fn put(&self, data: &[u8]) -> Result<ObjectHash> {
        let hash = hash_bytes(data);
        self.write_object(&hash, data)?;
        Ok(hash)
    }

    /// Store `data` under an expected hash; `CorruptObject` on mismatch.
    pub fn put_with_hash(&self, hash: &ObjectHash, data: &[u8]) -> Result<()> {
        let actual = hash_bytes(data);
        if actual != *hash {
            return Err(SdkError::CorruptObject(
                hash.to_hex(),
                format!("content hashes to {}", actual.to_hex()),
            ));
        }
        self.write_object(hash, data)
    }

    /// Read an object; `ObjectNotFound` if absent, `CorruptObject` if the
    /// stored bytes no longer match their hash.
    pub fn get(&self, hash: &ObjectHash) -> Result<Vec<u8>> {
        let data = match fs::read(self.path_of(hash)) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SdkError::ObjectNotFound(hash.to_hex()))
            }
            Err(e) => return Err(e.into()),
        };
        let actual = hash_bytes(&data);
        if actual != *hash {
            return Err(SdkError::CorruptObject(
                hash.to_hex(),
                format!("stored content hashes to {}", actual.to_hex()),
            ));
        }
        Ok(data)
    }

    /// Whether an object is present.
    pub fn has(&self, hash: &ObjectHash) -> bool {
        self.path_of(hash).is_file()
    }

    /// Filesystem path where `hash` is (or would be) stored.
    pub fn path_of(&self, hash: &ObjectHash) -> PathBuf {
        let hex = hash.to_hex();
        let (dir, file) = hex.split_at(2);
        self.root.join(dir).join(file)
    }

    /// All stored object hashes (unordered).
    pub fn list(&self) -> Result<Vec<ObjectHash>> {
        let mut out = Vec::new();
        self.for_each_object(|hash, _| out.push(hash))?;
        Ok(out)
    }

    /// (object count, total bytes).
    pub fn stats(&self) -> Result<(u64, u64)> {
        let mut count = 0u64;
        let mut bytes = 0u64;
        self.for_each_object(|_, len| {
            count += 1;
            bytes += len;
        })?;
        Ok((count, bytes))
    }

    /// Store `data` whose hash the caller has just computed (skips
    /// re-hashing).
    pub(crate) fn put_hashed(&self, hash: &ObjectHash, data: &[u8]) -> Result<()> {
        debug_assert_eq!(hash_bytes(data), *hash);
        self.write_object(hash, data)
    }

    fn write_object(&self, hash: &ObjectHash, data: &[u8]) -> Result<()> {
        let target = self.path_of(hash);
        if target.is_file() {
            return Ok(());
        }
        let tmp = fsutil::write_temp(&target, data)?;
        match fsutil::rename_with_retry(&tmp, &target) {
            Ok(()) => Ok(()),
            // Another writer stored the same object first (Windows refuses
            // to rename onto an existing, possibly open, file).
            Err(_) if target.is_file() => {
                let _ = fs::remove_file(&tmp);
                Ok(())
            }
            Err(e) => {
                let _ = fs::remove_file(&tmp);
                Err(e.into())
            }
        }
    }

    /// Visit every well-formed object file with its size. Temp files and
    /// stray entries are skipped.
    fn for_each_object(&self, mut f: impl FnMut(ObjectHash, u64)) -> Result<()> {
        let fanout = match fs::read_dir(&self.root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for dir in fanout {
            let dir = dir?;
            let dir_name = dir.file_name();
            let Some(prefix) = dir_name.to_str() else {
                continue;
            };
            if prefix.len() != 2 || !dir.file_type()?.is_dir() {
                continue;
            }
            for obj in fs::read_dir(dir.path())? {
                let obj = obj?;
                let obj_name = obj.file_name();
                let Some(rest) = obj_name.to_str() else {
                    continue;
                };
                if rest.len() != 62 {
                    continue;
                }
                let Ok(hash) = format!("{prefix}{rest}").parse::<ObjectHash>() else {
                    continue;
                };
                let meta = obj.metadata()?;
                if meta.is_file() {
                    f(hash, meta.len());
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_roundtrip_and_layout() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path().join("objects"));
        let h = store.put(b"hello").unwrap();
        assert_eq!(h, hash_bytes(b"hello"));
        assert!(store.has(&h));
        assert_eq!(store.get(&h).unwrap(), b"hello");
        let hex = h.to_hex();
        let expected = dir.path().join("objects").join(&hex[..2]).join(&hex[2..]);
        assert_eq!(store.path_of(&h), expected);
        assert!(expected.is_file());
    }

    #[test]
    fn put_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let a = store.put(b"same").unwrap();
        let b = store.put(b"same").unwrap();
        assert_eq!(a, b);
        assert_eq!(store.stats().unwrap(), (1, 4));
    }

    #[test]
    fn missing_object_is_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let h = hash_bytes(b"absent");
        assert!(!store.has(&h));
        assert!(matches!(store.get(&h), Err(SdkError::ObjectNotFound(_))));
    }

    #[test]
    fn put_with_hash_rejects_mismatch() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let wrong = hash_bytes(b"other");
        assert!(matches!(
            store.put_with_hash(&wrong, b"data"),
            Err(SdkError::CorruptObject(..))
        ));
        assert!(!store.has(&wrong));
        let right = hash_bytes(b"data");
        store.put_with_hash(&right, b"data").unwrap();
        assert!(store.has(&right));
    }

    #[test]
    fn get_detects_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let h = store.put(b"original").unwrap();
        fs::write(store.path_of(&h), b"tampered").unwrap();
        assert!(matches!(store.get(&h), Err(SdkError::CorruptObject(..))));
    }

    #[test]
    fn list_and_stats_skip_stray_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        assert!(store.list().unwrap().is_empty());
        let a = store.put(b"a").unwrap();
        let b = store.put(b"bb").unwrap();
        fs::write(dir.path().join("README"), b"not an object").unwrap();
        fs::write(store.path_of(&a).with_file_name(".tmp-x"), b"junk").unwrap();
        let mut listed = store.list().unwrap();
        listed.sort();
        let mut expected = vec![a, b];
        expected.sort();
        assert_eq!(listed, expected);
        assert_eq!(store.stats().unwrap(), (2, 3));
    }

    #[test]
    fn stats_of_missing_root_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path().join("does-not-exist"));
        assert_eq!(store.stats().unwrap(), (0, 0));
    }
}
