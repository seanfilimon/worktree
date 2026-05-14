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
}

impl Blob {
    /// Create a new blob metadata struct.
    pub fn new(hash: ContentHash, size: u64) -> Self {
        Blob { hash, size }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_metadata() {
        let hash = ContentHash::ZERO;
        let blob = Blob::new(hash, 1024);
        assert_eq!(blob.size, 1024);
        assert_eq!(blob.hash, hash);
    }
}
