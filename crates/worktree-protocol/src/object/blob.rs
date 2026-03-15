use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::hash::{hash_bytes, ContentHash};

/// A content-addressable blob of data, identified by its BLAKE3 hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Blob {
    /// The BLAKE3 hash of the content.
    pub hash: ContentHash,
    /// The size of the content in bytes.
    pub size: u64,
    /// The raw content bytes.
    pub content: Vec<u8>,
}

impl Blob {
    /// Create a new blob from raw bytes, computing its hash automatically.
    pub fn from_bytes(data: Vec<u8>) -> Self {
        let hash = hash_bytes(&data);
        let size = data.len() as u64;
        Blob {
            hash,
            size,
            content: data,
        }
    }

    /// Create a new blob by reading a file from disk.
    pub fn from_file(path: &Path) -> io::Result<Self> {
        let data = std::fs::read(path)?;
        Ok(Self::from_bytes(data))
    }

    /// Returns `true` if the blob contains no data.
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
    }

    /// Verify that the stored hash matches a freshly computed hash of the content.
    ///
    /// Returns `true` if the hash is valid, `false` if it has been corrupted.
    pub fn verify(&self) -> bool {
        let computed = hash_bytes(&self.content);
        self.hash == computed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_from_bytes_basic() {
        let data = b"hello blob".to_vec();
        let blob = Blob::from_bytes(data.clone());
        assert_eq!(blob.size, data.len() as u64);
        assert_eq!(blob.content, data);
        assert_ne!(blob.hash, ContentHash::ZERO);
    }

    #[test]
    fn test_from_bytes_empty() {
        let blob = Blob::from_bytes(Vec::new());
        assert!(blob.is_empty());
        assert_eq!(blob.size, 0);
    }

    #[test]
    fn test_is_empty() {
        let empty = Blob::from_bytes(Vec::new());
        assert!(empty.is_empty());

        let non_empty = Blob::from_bytes(vec![1, 2, 3]);
        assert!(!non_empty.is_empty());
    }

    #[test]
    fn test_verify_valid() {
        let blob = Blob::from_bytes(b"verify me".to_vec());
        assert!(blob.verify());
    }

    #[test]
    fn test_verify_corrupted() {
        let mut blob = Blob::from_bytes(b"original".to_vec());
        // Tamper with the content without updating the hash
        blob.content = b"tampered".to_vec();
        assert!(!blob.verify());
    }

    #[test]
    fn test_deterministic_hash() {
        let blob1 = Blob::from_bytes(b"same content".to_vec());
        let blob2 = Blob::from_bytes(b"same content".to_vec());
        assert_eq!(blob1.hash, blob2.hash);
    }

    #[test]
    fn test_different_content_different_hash() {
        let blob1 = Blob::from_bytes(b"content a".to_vec());
        let blob2 = Blob::from_bytes(b"content b".to_vec());
        assert_ne!(blob1.hash, blob2.hash);
    }

    #[test]
    fn test_from_file() {
        let dir = std::env::temp_dir().join("worktree_blob_test");
        std::fs::create_dir_all(&dir).unwrap();
        let file_path = dir.join("test_blob.bin");
        let data = b"file content for blob";
        {
            let mut f = std::fs::File::create(&file_path).unwrap();
            f.write_all(data).unwrap();
        }

        let blob = Blob::from_file(&file_path).unwrap();
        assert_eq!(blob.content, data.to_vec());
        assert_eq!(blob.size, data.len() as u64);
        assert!(blob.verify());

        // Cleanup
        let _ = std::fs::remove_file(&file_path);
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn test_from_file_not_found() {
        let result = Blob::from_file(Path::new("/nonexistent/path/to/blob"));
        assert!(result.is_err());
    }

    #[test]
    fn test_serde_json_roundtrip() {
        let blob = Blob::from_bytes(b"serde test".to_vec());
        let json = serde_json::to_string(&blob).unwrap();
        let deserialized: Blob = serde_json::from_str(&json).unwrap();
        assert_eq!(blob, deserialized);
        assert!(deserialized.verify());
    }

    #[test]
    fn test_serde_bincode_roundtrip() {
        let blob = Blob::from_bytes(b"bincode test".to_vec());
        let encoded = bincode::serialize(&blob).unwrap();
        let decoded: Blob = bincode::deserialize(&encoded).unwrap();
        assert_eq!(blob, decoded);
        assert!(decoded.verify());
    }
}
