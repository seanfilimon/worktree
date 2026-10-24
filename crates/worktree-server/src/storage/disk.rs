use std::path::PathBuf;

use worktree_protocol::core::hash::ContentHash;

use crate::error::ServerError;
use crate::storage::backend::StorageBackend;

/// A content-addressed storage backend that persists objects to disk.
///
/// Objects are stored under `root/objects/XX/YYYYYY...` where `XX` is the
/// first two hex characters of the content hash (used as a fan-out directory)
/// and `YYYYYY...` is the remaining 62 hex characters of the hash.
///
/// This layout mirrors the approach used by Git's loose object store and
/// provides a good balance between directory fan-out and lookup speed.
pub struct DiskStorage {
    /// Root directory for the storage backend. Object files live under
    /// `root/objects/`.
    pub root: PathBuf,
}

impl DiskStorage {
    /// Create a new `DiskStorage` rooted at the given directory.
    ///
    /// The directory (and its `objects/` subdirectory) will be created on
    /// first write if they do not already exist.
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Return the path to the `objects/` subdirectory.
    pub fn objects_dir(&self) -> PathBuf {
        self.root.join("objects")
    }

    /// Compute the full filesystem path for a given content hash.
    ///
    /// The path is `<root>/objects/<first 2 hex chars>/<remaining 62 hex chars>`.
    fn object_path(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.to_hex();
        let (prefix, rest) = hex.split_at(2);
        self.objects_dir().join(prefix).join(rest)
    }

    /// Compute the fan-out directory path for a given content hash.
    ///
    /// This is the `<root>/objects/<first 2 hex chars>/` directory that
    /// contains the object file.
    fn fan_out_dir(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.to_hex();
        let prefix = &hex[..2];
        self.objects_dir().join(prefix)
    }
}

impl StorageBackend for DiskStorage {
    /// Store `data` on disk under the content-addressed path derived from `hash`.
    ///
    /// Creates the fan-out directory if it does not already exist.
    fn store(&self, hash: &ContentHash, data: &[u8]) -> Result<(), ServerError> {
        let dir = self.fan_out_dir(hash);
        let path = self.object_path(hash);

        let _ = (dir, path, data);
        todo!("create fan-out directory, write data to object path atomically")
    }

    /// Retrieve the raw bytes of the object identified by `hash`.
    ///
    /// Returns `ServerError::Storage` if the object does not exist or
    /// cannot be read.
    fn retrieve(&self, hash: &ContentHash) -> Result<Vec<u8>, ServerError> {
        let path = self.object_path(hash);

        let _ = path;
        todo!("read object file from disk and return its contents")
    }

    /// Check whether an object with the given `hash` exists on disk.
    fn exists(&self, hash: &ContentHash) -> bool {
        self.object_path(hash).exists()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_protocol::core::hash::hash_bytes;

    #[test]
    fn object_path_has_correct_structure() {
        let storage = DiskStorage::new(PathBuf::from("/tmp/worktree-store"));
        let hash = hash_bytes(b"hello world");
        let path = storage.object_path(&hash);

        let hex = hash.to_hex();
        let expected_prefix = &hex[..2];
        let expected_rest = &hex[2..];

        let parent_dir = path.parent().unwrap();
        assert_eq!(
            parent_dir.file_name().unwrap().to_str().unwrap(),
            expected_prefix
        );
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            expected_rest
        );
    }

    #[test]
    fn fan_out_dir_matches_first_two_hex_chars() {
        let storage = DiskStorage::new(PathBuf::from("/data"));
        let hash = hash_bytes(b"test data");
        let dir = storage.fan_out_dir(&hash);

        let hex = hash.to_hex();
        let expected = &hex[..2];
        assert_eq!(dir.file_name().unwrap().to_str().unwrap(), expected);
    }

    #[test]
    fn exists_returns_false_for_missing_object() {
        let storage = DiskStorage::new(PathBuf::from("/nonexistent/path"));
        let hash = hash_bytes(b"does not exist");
        assert!(!storage.exists(&hash));
    }
}
