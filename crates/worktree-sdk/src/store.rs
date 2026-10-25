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

use worktree_protocol::proto::sync as wire;

use crate::error::{Result, SdkError};
use crate::repo::fsutil;

pub use worktree_protocol::core::hash::ContentHash as ObjectHash;

/// BLAKE3 hash of `data`.
pub fn hash_bytes(data: &[u8]) -> ObjectHash {
    worktree_protocol::core::hash::hash_bytes(data)
}

/// Files larger than this (8 MiB) are stored as content-defined chunks
/// (ENGINEERING_PLAN §13.2).
pub const CHUNK_THRESHOLD: u64 = 8 * 1024 * 1024;

/// Whether a file of `len` bytes is stored chunked.
pub fn is_chunked_size(len: u64) -> bool {
    len > CHUNK_THRESHOLD
}

/// One chunk of a large file (a blob object).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChunkRef {
    pub hash: ObjectHash,
    pub size: u64,
}

/// A large file split into content-defined chunks; its content is the
/// concatenation of the chunks. Stored as a protobuf `ChunkList` object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkList {
    /// Total file size in bytes.
    pub size: u64,
    pub chunks: Vec<ChunkRef>,
}

impl ChunkList {
    /// Canonical protobuf encoding (`worktree.sync.v1.ChunkList`).
    pub fn to_bytes(&self) -> Vec<u8> {
        use prost::Message;
        wire::ChunkList {
            size: self.size,
            chunks: self
                .chunks
                .iter()
                .map(|c| wire::Chunk {
                    hash: Some(wire::ContentHash {
                        value: c.hash.as_bytes().to_vec(),
                    }),
                    size: c.size,
                })
                .collect(),
        }
        .encode_to_vec()
    }

    /// Decode and validate (32-byte hashes, chunk sizes sum to `size`).
    pub fn from_bytes(data: &[u8]) -> std::result::Result<Self, String> {
        use prost::Message;
        let rec = wire::ChunkList::decode(data).map_err(|e| format!("invalid chunk list: {e}"))?;
        let mut chunks = Vec::with_capacity(rec.chunks.len());
        let mut total = 0u64;
        for (i, c) in rec.chunks.iter().enumerate() {
            let bytes: [u8; 32] = c
                .hash
                .as_ref()
                .map(|h| h.value.as_slice())
                .unwrap_or_default()
                .try_into()
                .map_err(|_| format!("chunk {i} has a malformed hash"))?;
            total = total
                .checked_add(c.size)
                .ok_or_else(|| "chunk sizes overflow".to_string())?;
            chunks.push(ChunkRef {
                hash: ObjectHash::from_bytes(bytes),
                size: c.size,
            });
        }
        if total != rec.size {
            return Err(format!(
                "chunk sizes sum to {total}, the list says {}",
                rec.size
            ));
        }
        Ok(Self {
            size: rec.size,
            chunks,
        })
    }
}

/// File content hashed the way a manifest entry references it, ready to be
/// stored with [`ObjectStore::put_prepared`].
pub(crate) struct PreparedFile {
    pub(crate) hash: ObjectHash,
    pub(crate) chunked: bool,
    /// For chunked files: the encoded chunk list and each chunk's
    /// `(offset, len)` in the content.
    chunks: Option<SplitFile>,
}

/// Encoded chunk list, the list, and each chunk's `(offset, len)`.
type SplitFile = (Vec<u8>, ChunkList, Vec<(usize, usize)>);

/// Hash `data` as a file: a blob, or (above [`CHUNK_THRESHOLD`]) FastCDC
/// chunks plus a chunk list.
pub(crate) fn prepare_file(data: &[u8]) -> PreparedFile {
    if !is_chunked_size(data.len() as u64) {
        return PreparedFile {
            hash: hash_bytes(data),
            chunked: false,
            chunks: None,
        };
    }
    let ranges = crate::scan::chunk_boundaries(data);
    let list = ChunkList {
        size: data.len() as u64,
        chunks: ranges
            .iter()
            .map(|&(offset, len)| ChunkRef {
                hash: hash_bytes(&data[offset..offset + len]),
                size: len as u64,
            })
            .collect(),
    };
    let encoded = list.to_bytes();
    PreparedFile {
        hash: hash_bytes(&encoded),
        chunked: true,
        chunks: Some((encoded, list, ranges)),
    }
}

/// The entry hash and `chunked` flag `data` would get from
/// [`ObjectStore::put_file`], without storing anything.
pub fn file_hash(data: &[u8]) -> (ObjectHash, bool) {
    let prepared = prepare_file(data);
    (prepared.hash, prepared.chunked)
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

    /// The full content of a manifest entry. Chunked entries
    /// (`entry.chunked`, ENGINEERING_PLAN §13.2) are reassembled from their
    /// chunk list; use this instead of `get(&entry.hash)` for file content.
    pub fn read_entry(&self, entry: &crate::model::ManifestEntry) -> Result<Vec<u8>> {
        if !entry.chunked {
            return self.get(&entry.hash);
        }
        let list = self.read_chunk_list(&entry.hash)?;
        let size = usize::try_from(list.size).map_err(|_| {
            SdkError::Other(format!(
                "{}: {} bytes do not fit in memory",
                entry.path, list.size
            ))
        })?;
        let mut out = Vec::with_capacity(size);
        for chunk in &list.chunks {
            let data = self.get(&chunk.hash)?;
            if data.len() as u64 != chunk.size {
                return Err(SdkError::CorruptObject(
                    entry.hash.to_hex(),
                    format!(
                        "chunk {} is {} bytes, the chunk list says {}",
                        chunk.hash.to_hex(),
                        data.len(),
                        chunk.size
                    ),
                ));
            }
            out.extend_from_slice(&data);
        }
        Ok(out)
    }

    /// Store file content the way a manifest entry references it: files of
    /// at most [`CHUNK_THRESHOLD`] bytes as one blob, larger files as
    /// FastCDC chunks plus a [`ChunkList`]. Returns the entry hash and
    /// whether the entry is chunked.
    pub fn put_file(&self, data: &[u8]) -> Result<(ObjectHash, bool)> {
        let prepared = prepare_file(data);
        self.put_prepared(&prepared, data)?;
        Ok((prepared.hash, prepared.chunked))
    }

    /// Store `data` as hashed by [`prepare_file`] (chunks before the chunk
    /// list, so a stored list always has its chunks).
    pub(crate) fn put_prepared(&self, prepared: &PreparedFile, data: &[u8]) -> Result<()> {
        match &prepared.chunks {
            None => self.write_object(&prepared.hash, data),
            Some((encoded, list, ranges)) => {
                for (chunk, &(offset, len)) in list.chunks.iter().zip(ranges) {
                    self.write_object(&chunk.hash, &data[offset..offset + len])?;
                }
                self.write_object(&prepared.hash, encoded)
            }
        }
    }

    /// Decode the chunk-list object `hash`.
    pub fn read_chunk_list(&self, hash: &ObjectHash) -> Result<ChunkList> {
        let data = self.get(hash)?;
        ChunkList::from_bytes(&data).map_err(|e| SdkError::CorruptObject(hash.to_hex(), e))
    }

    /// Every object needed to materialize `entry`: its blob, or its chunk
    /// list followed by the chunks. A chunked entry whose chunk list is not
    /// stored locally yields just the list hash.
    pub fn entry_objects(&self, entry: &crate::model::ManifestEntry) -> Result<Vec<ObjectHash>> {
        if !entry.chunked || !self.has(&entry.hash) {
            return Ok(vec![entry.hash]);
        }
        let list = self.read_chunk_list(&entry.hash)?;
        let mut out = Vec::with_capacity(list.chunks.len() + 1);
        out.push(entry.hash);
        out.extend(list.chunks.iter().map(|c| c.hash));
        Ok(out)
    }

    /// Whether the full content of `entry` is stored locally.
    pub fn has_entry(&self, entry: &crate::model::ManifestEntry) -> bool {
        if !entry.chunked {
            return self.has(&entry.hash);
        }
        match self.read_chunk_list(&entry.hash) {
            Ok(list) => list.chunks.iter().all(|c| self.has(&c.hash)),
            Err(_) => false,
        }
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

    fn entry(
        path: &str,
        hash: ObjectHash,
        size: u64,
        chunked: bool,
    ) -> crate::model::ManifestEntry {
        crate::model::ManifestEntry {
            path: path.into(),
            hash,
            size,
            executable: false,
            chunked,
        }
    }

    /// 12 MiB + a tail: four copies of the same 3 MiB pseudo-random block.
    fn repeated_content() -> Vec<u8> {
        let mut state = 0x1234_5678_9abc_def1u64;
        let mut block = Vec::with_capacity(3 * 1024 * 1024);
        while block.len() < 3 * 1024 * 1024 {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            block.extend_from_slice(&state.to_le_bytes());
        }
        let mut data = Vec::new();
        for _ in 0..4 {
            data.extend_from_slice(&block);
        }
        data.extend_from_slice(b"tail");
        data
    }

    #[test]
    fn small_files_are_single_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let data = vec![7u8; CHUNK_THRESHOLD as usize];
        let (h, chunked) = store.put_file(&data).unwrap();
        assert!(!chunked, "exactly 8 MiB is not chunked");
        assert_eq!(h, hash_bytes(&data));
        assert_eq!(file_hash(&data), (h, false));
        let e = entry("f", h, data.len() as u64, false);
        assert_eq!(store.read_entry(&e).unwrap(), data);
        assert_eq!(store.entry_objects(&e).unwrap(), vec![h]);
        assert!(store.has_entry(&e));
    }

    #[test]
    fn large_files_are_chunked_deduplicated_and_reassembled() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let data = repeated_content();
        let (h, chunked) = store.put_file(&data).unwrap();
        assert!(chunked);
        assert_eq!(file_hash(&data), (h, true));

        let list = store.read_chunk_list(&h).unwrap();
        assert_eq!(list.size, data.len() as u64);
        assert!(list.chunks.len() >= 4, "{} chunks", list.chunks.len());
        assert!(list
            .chunks
            .iter()
            .all(|c| c.size <= crate::scan::MAX_CHUNK_SIZE as u64));
        let distinct: std::collections::HashSet<_> = list.chunks.iter().map(|c| c.hash).collect();
        assert!(
            distinct.len() < list.chunks.len(),
            "repeated blocks must share chunks ({} distinct of {})",
            distinct.len(),
            list.chunks.len()
        );
        // Stored: one list + the distinct chunks; total well below 2x content.
        let (count, bytes) = store.stats().unwrap();
        assert_eq!(count as usize, distinct.len() + 1);
        assert!(bytes < data.len() as u64, "dedupe saves space: {bytes}");

        let e = entry("big.bin", h, data.len() as u64, true);
        assert_eq!(store.read_entry(&e).unwrap(), data);
        let objects = store.entry_objects(&e).unwrap();
        assert_eq!(objects[0], h);
        assert_eq!(objects.len(), list.chunks.len() + 1);
        assert!(store.has_entry(&e));

        // Losing a chunk is detected.
        fs::remove_file(store.path_of(&list.chunks[1].hash)).unwrap();
        assert!(!store.has_entry(&e));
        assert!(matches!(
            store.read_entry(&e),
            Err(SdkError::ObjectNotFound(_))
        ));
    }

    #[test]
    fn chunk_list_encoding_round_trips_and_validates() {
        let list = ChunkList {
            size: 5,
            chunks: vec![
                ChunkRef {
                    hash: hash_bytes(b"ab"),
                    size: 2,
                },
                ChunkRef {
                    hash: hash_bytes(b"cde"),
                    size: 3,
                },
            ],
        };
        assert_eq!(ChunkList::from_bytes(&list.to_bytes()).unwrap(), list);
        let bad = ChunkList { size: 6, ..list };
        assert!(ChunkList::from_bytes(&bad.to_bytes()).is_err());
        assert!(ChunkList::from_bytes(b"\xff\xff").is_err());

        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let missing = entry("x", hash_bytes(b"nope"), 9, true);
        assert!(!store.has_entry(&missing));
        assert_eq!(store.entry_objects(&missing).unwrap(), vec![missing.hash]);
    }

    #[test]
    fn stats_of_missing_root_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path().join("does-not-exist"));
        assert_eq!(store.stats().unwrap(), (0, 0));
    }
}
