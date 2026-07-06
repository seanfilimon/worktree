//! Content-addressable object store.
//!
//! Layout per Storage.md:
//!
//! ```text
//! <store>/objects/<kind>/<2-hex-prefix>/<62-hex-rest>
//! ```
//!
//! Writes are atomic (temp file + rename) and idempotent — identical
//! content lands on the same path, so `put` of an existing object is a
//! no-op (automatic deduplication). Reads always verify the frame hash.

use super::ObjectKind;
use crate::codec;
use crate::error::{Result, StoreError};
use std::io::Write;
use std::path::PathBuf;
use worktree_protocol::core::hash::ContentHash;

/// Handle to the `objects/` half of a store.
#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

impl ObjectStore {
    /// Open the object store rooted at `<store>/objects`.
    pub fn new(objects_root: PathBuf) -> Self {
        Self { root: objects_root }
    }

    fn path_for(&self, kind: ObjectKind, hex: &str) -> PathBuf {
        self.root
            .join(kind.dir_name())
            .join(&hex[..2])
            .join(&hex[2..])
    }

    /// Store `payload` as an object of `kind`; returns its content address.
    ///
    /// No-op (besides hashing) when the object already exists.
    pub fn put(&self, kind: ObjectKind, payload: &[u8]) -> Result<ContentHash> {
        let hash = worktree_protocol::core::hash::hash_bytes(payload);
        let hex = hash.to_hex();
        let path = self.path_for(kind, &hex);
        if path.exists() {
            return Ok(hash);
        }

        let parent = path.parent().expect("object path has parent");
        std::fs::create_dir_all(parent)?;

        let frame = codec::encode(kind, payload)?;
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(&frame)?;
        tmp.flush()?;
        // Losing the race to another writer is fine — same content.
        match tmp.persist_noclobber(&path) {
            Ok(_) => {}
            Err(e) if path.exists() => drop(e),
            Err(e) => return Err(e.error.into()),
        }
        Ok(hash)
    }

    /// Load and verify the object addressed by `hash`.
    pub fn get(&self, kind: ObjectKind, hash: &ContentHash) -> Result<Vec<u8>> {
        let hex = hash.to_hex();
        let path = self.path_for(kind, &hex);
        let frame = std::fs::read(&path).map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                StoreError::ObjectNotFound(format!("{}/{hex}", kind.dir_name()))
            } else {
                StoreError::Io(e)
            }
        })?;
        let (frame_kind, payload) = codec::decode(&frame, Some(hash.as_bytes()))?;
        if frame_kind != kind {
            return Err(StoreError::Corrupt(format!(
                "object {hex} stored under {} but frame says {}",
                kind.dir_name(),
                frame_kind.dir_name()
            )));
        }
        Ok(payload)
    }

    /// True if the object exists (no verification).
    pub fn contains(&self, kind: ObjectKind, hash: &ContentHash) -> bool {
        self.path_for(kind, &hash.to_hex()).exists()
    }

    /// Number of objects stored under `kind`.
    pub fn count(&self, kind: ObjectKind) -> usize {
        self.iter_hex(kind).len()
    }

    /// All object addresses (hex) stored under `kind`.
    pub fn iter_hex(&self, kind: ObjectKind) -> Vec<String> {
        let mut out = Vec::new();
        let kind_dir = self.root.join(kind.dir_name());
        let Ok(prefixes) = std::fs::read_dir(&kind_dir) else {
            return out;
        };
        for prefix in prefixes.filter_map(|e| e.ok()) {
            let prefix_name = prefix.file_name().to_string_lossy().into_owned();
            let Ok(entries) = std::fs::read_dir(prefix.path()) else {
                continue;
            };
            for entry in entries.filter_map(|e| e.ok()) {
                out.push(format!(
                    "{prefix_name}{}",
                    entry.file_name().to_string_lossy()
                ));
            }
        }
        out
    }

    /// Verify every object of `kind`; returns `(hex, error)` per corrupt object.
    pub fn verify_kind(&self, kind: ObjectKind) -> Vec<(String, String)> {
        let mut corrupt = Vec::new();
        for hex in self.iter_hex(kind) {
            let Ok(hash) = hex.parse::<ContentHash>() else {
                corrupt.push((hex, "invalid object file name".into()));
                continue;
            };
            if let Err(e) = self.get(kind, &hash) {
                corrupt.push((hex, e.to_string()));
            }
        }
        corrupt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> (tempfile::TempDir, ObjectStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::new(dir.path().join("objects"));
        (dir, store)
    }

    #[test]
    fn put_get_roundtrip() {
        let (_dir, store) = store();
        let hash = store.put(ObjectKind::Blob, b"file contents").unwrap();
        assert!(store.contains(ObjectKind::Blob, &hash));
        assert_eq!(
            store.get(ObjectKind::Blob, &hash).unwrap(),
            b"file contents"
        );
    }

    #[test]
    fn identical_content_dedups() {
        let (_dir, store) = store();
        let a = store.put(ObjectKind::Blob, b"same").unwrap();
        let b = store.put(ObjectKind::Blob, b"same").unwrap();
        assert_eq!(a, b);
        assert_eq!(store.count(ObjectKind::Blob), 1);
    }

    #[test]
    fn missing_object_is_not_found() {
        let (_dir, store) = store();
        let hash = worktree_protocol::core::hash::hash_bytes(b"never stored");
        assert!(matches!(
            store.get(ObjectKind::Blob, &hash),
            Err(StoreError::ObjectNotFound(_))
        ));
    }

    #[test]
    fn corruption_is_detected_on_read_and_verify() {
        let (_dir, store) = store();
        let hash = store
            .put(ObjectKind::Snapshot, b"precious history")
            .unwrap();

        // Flip a byte in the stored frame.
        let hex = hash.to_hex();
        let path = store.path_for(ObjectKind::Snapshot, &hex);
        let mut bytes = std::fs::read(&path).unwrap();
        let mid = bytes.len() / 2;
        bytes[mid] ^= 0xff;
        std::fs::write(&path, bytes).unwrap();

        assert!(matches!(
            store.get(ObjectKind::Snapshot, &hash),
            Err(StoreError::Corrupt(_))
        ));
        let corrupt = store.verify_kind(ObjectKind::Snapshot);
        assert_eq!(corrupt.len(), 1);
        assert_eq!(corrupt[0].0, hex);
    }

    #[test]
    fn kind_mismatch_is_corrupt() {
        let (_dir, store) = store();
        let hash = store.put(ObjectKind::Blob, b"blob bytes").unwrap();
        // Copy the blob frame into the snapshots namespace under the same hash.
        let hex = hash.to_hex();
        let src = store.path_for(ObjectKind::Blob, &hex);
        let dst = store.path_for(ObjectKind::Snapshot, &hex);
        std::fs::create_dir_all(dst.parent().unwrap()).unwrap();
        std::fs::copy(&src, &dst).unwrap();

        assert!(matches!(
            store.get(ObjectKind::Snapshot, &hash),
            Err(StoreError::Corrupt(_))
        ));
    }
}
