use serde::{Deserialize, Serialize};
use crate::core::hash::{ContentHash, hash_bytes};

/// A chunk of a large file
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub hash: ContentHash,
    pub offset: u64,
    pub size: u64,
}

/// Manifest describing how a large file is split into chunks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkManifest {
    pub file_hash: ContentHash,
    pub file_size: u64,
    pub chunks: Vec<Chunk>,
    pub chunk_algorithm: ChunkAlgorithm,
}

/// Algorithm used for chunking
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChunkAlgorithm {
    FastCdc,
    FixedSize,
}

impl Default for ChunkAlgorithm {
    fn default() -> Self {
        ChunkAlgorithm::FastCdc
    }
}

/// Configuration for large file handling
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFileConfig {
    pub threshold: u64,
    pub min_chunk_size: u64,
    pub avg_chunk_size: u64,
    pub max_chunk_size: u64,
    pub algorithm: ChunkAlgorithm,
}

impl Default for LargeFileConfig {
    fn default() -> Self {
        Self {
            threshold: 10 * 1024 * 1024,  // 10MB
            min_chunk_size: 1024 * 1024,  // 1MB
            avg_chunk_size: 4 * 1024 * 1024,  // 4MB
            max_chunk_size: 16 * 1024 * 1024, // 16MB
            algorithm: ChunkAlgorithm::FastCdc,
        }
    }
}

impl LargeFileConfig {
    pub fn is_large_file(&self, size: u64) -> bool {
        size >= self.threshold
    }
}

/// A stub reference for a large file that hasn't been downloaded yet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LargeFileStub {
    pub path: String,
    pub file_hash: ContentHash,
    pub file_size: u64,
    pub manifest_hash: ContentHash,
    pub materialized: bool,
}

impl LargeFileStub {
    pub fn new(path: &str, file_hash: ContentHash, file_size: u64, manifest_hash: ContentHash) -> Self {
        Self {
            path: path.to_string(),
            file_hash,
            file_size,
            manifest_hash,
            materialized: false,
        }
    }

    pub fn mark_materialized(&mut self) {
        self.materialized = true;
    }
}

/// Simple fixed-size chunker (FastCDC would need a proper implementation)
pub fn chunk_data(data: &[u8], config: &LargeFileConfig) -> ChunkManifest {
    let file_hash = hash_bytes(data);
    let file_size = data.len() as u64;
    let chunk_size = config.avg_chunk_size as usize;
    let mut chunks = Vec::new();
    let mut offset = 0usize;

    while offset < data.len() {
        let end = (offset + chunk_size).min(data.len());
        let chunk_data = &data[offset..end];
        let chunk_hash = hash_bytes(chunk_data);
        chunks.push(Chunk {
            hash: chunk_hash,
            offset: offset as u64,
            size: (end - offset) as u64,
        });
        offset = end;
    }

    ChunkManifest {
        file_hash,
        file_size,
        chunks,
        chunk_algorithm: config.algorithm.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_large_file() {
        let config = LargeFileConfig::default();
        assert!(!config.is_large_file(1024)); // 1KB
        assert!(!config.is_large_file(5 * 1024 * 1024)); // 5MB
        assert!(config.is_large_file(10 * 1024 * 1024)); // 10MB
        assert!(config.is_large_file(100 * 1024 * 1024)); // 100MB
    }

    #[test]
    fn test_chunk_data() {
        let config = LargeFileConfig {
            avg_chunk_size: 10,
            ..Default::default()
        };
        let data = vec![0u8; 35];
        let manifest = chunk_data(&data, &config);
        assert_eq!(manifest.file_size, 35);
        assert_eq!(manifest.chunks.len(), 4); // 10+10+10+5
        assert_eq!(manifest.chunks[0].size, 10);
        assert_eq!(manifest.chunks[3].size, 5);
    }

    #[test]
    fn test_stub() {
        let mut stub = LargeFileStub::new(
            "assets/large.bin",
            ContentHash::ZERO,
            1024 * 1024 * 50,
            ContentHash::ZERO,
        );
        assert!(!stub.materialized);
        stub.mark_materialized();
        assert!(stub.materialized);
    }
}
