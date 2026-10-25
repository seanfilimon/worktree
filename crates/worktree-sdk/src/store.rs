//! Content-addressed object store under `.wt/objects/`.
//!
//! Objects live at `objects/<hh>/<62 hex>` where the hash is BLAKE3 of the
//! object's (uncompressed) content. The same content and hashes are used on
//! the wire and on the server.
//!
//! Stored encoding (ENGINEERING_PLAN §14.6, `specs/storage/Compression.md`):
//! a file is either the raw content or the 4-byte magic `WTZ1` followed by
//! one zstd frame of the content. Writers compress when the object is at
//! least [`COMPRESS_MIN_SIZE`] bytes and compression saves at least 10 %;
//! content that itself starts with the magic is always compressed so that a
//! leading `WTZ1` is unambiguous. Readers accept both forms. Object ids never
//! depend on the encoding, so stores may mix raw and compressed files and no
//! migration is needed. [`ObjectStore::stats`], [`ObjectStore::size`] and
//! [`ObjectStore::objects`] report stored (on-disk) bytes.
//!
//! Writes go to a uniquely named temp file inside the fan-out directory and
//! are then renamed into place, so readers never observe partial objects and
//! any number of threads or processes may write concurrently. Because the
//! store is content-addressed, a rename that loses a race against another
//! writer of the same object is harmless and treated as success.

use std::borrow::Cow;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use worktree_protocol::proto::sync as wire;

use crate::error::{Result, SdkError};
use crate::repo::fsutil;

pub use worktree_protocol::core::hash::ContentHash as ObjectHash;

/// BLAKE3 hash of `data`.
pub fn hash_bytes(data: &[u8]) -> ObjectHash {
    worktree_protocol::core::hash::hash_bytes(data)
}

// --- storage (ENGINEERING_PLAN §14.6): WTZ1 object encoding ---

/// Magic prefix of a zstd-compressed object file.
pub const COMPRESSED_MAGIC: [u8; 4] = *b"WTZ1";
/// Objects smaller than this are always stored raw (unless their content
/// starts with [`COMPRESSED_MAGIC`]).
pub const COMPRESS_MIN_SIZE: usize = 512;
/// zstd level used by writers (readers accept any level).
pub const COMPRESSION_LEVEL: i32 = 3;

/// Whether a file of `stored_len` bytes holding content of `content_len`
/// bytes saves at least 10 %.
fn saves_enough(stored_len: usize, content_len: usize) -> bool {
    (stored_len as u128) * 10 <= (content_len as u128) * 9
}

/// The stored form of `content`: `WTZ1` + a zstd frame when the object is
/// at least [`COMPRESS_MIN_SIZE`] bytes and that saves at least 10 %, else
/// the raw content. Content starting with the magic is always compressed.
pub fn encode_object(content: &[u8]) -> io::Result<Cow<'_, [u8]>> {
    let must = content.starts_with(&COMPRESSED_MAGIC);
    if !must && content.len() < COMPRESS_MIN_SIZE {
        return Ok(Cow::Borrowed(content));
    }
    let frame = zstd::bulk::compress(content, COMPRESSION_LEVEL)?;
    let stored_len = COMPRESSED_MAGIC.len() + frame.len();
    if !must && !saves_enough(stored_len, content.len()) {
        return Ok(Cow::Borrowed(content));
    }
    let mut out = Vec::with_capacity(stored_len);
    out.extend_from_slice(&COMPRESSED_MAGIC);
    out.extend_from_slice(&frame);
    Ok(Cow::Owned(out))
}

/// [`encode_object`] restricted to the mandatory case: only content that
/// starts with the magic is compressed (writers with compression off).
fn encode_object_uncompressed(content: &[u8]) -> io::Result<Cow<'_, [u8]>> {
    if content.starts_with(&COMPRESSED_MAGIC) {
        encode_object(content)
    } else {
        Ok(Cow::Borrowed(content))
    }
}

/// The content of a stored object file: the zstd frame after a `WTZ1`
/// prefix decompressed, anything else returned as is. Use
/// [`ObjectStore::get`] where the hash is known: it also accepts legacy raw
/// objects whose content happens to start with the magic.
pub fn decode_object(stored: &[u8]) -> io::Result<Cow<'_, [u8]>> {
    match stored.strip_prefix(&COMPRESSED_MAGIC) {
        Some(frame) => Ok(Cow::Owned(zstd::stream::decode_all(frame)?)),
        None => Ok(Cow::Borrowed(stored)),
    }
}

/// Decode `stored` and check it against `hash`.
fn decode_verified(hash: &ObjectHash, stored: Vec<u8>) -> Result<Vec<u8>> {
    if stored.starts_with(&COMPRESSED_MAGIC) {
        if let Ok(content) = zstd::stream::decode_all(&stored[COMPRESSED_MAGIC.len()..]) {
            if hash_bytes(&content) == *hash {
                return Ok(content);
            }
        }
        // Fall through: a raw object written before §14.6 whose content
        // begins with the magic.
    }
    let actual = hash_bytes(&stored);
    if actual != *hash {
        let detail = if stored.starts_with(&COMPRESSED_MAGIC) {
            "stored object does not decode to content with this hash".to_string()
        } else {
            format!("stored content hashes to {}", actual.to_hex())
        };
        return Err(SdkError::CorruptObject(hash.to_hex(), detail));
    }
    Ok(stored)
}

/// One stored object file as listed by [`ObjectStore::objects`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoredObject {
    pub hash: ObjectHash,
    /// Size of the file on disk (compressed size for `WTZ1` files).
    pub stored_size: u64,
    /// Last modification time of the file, when the platform reports it.
    pub modified: Option<SystemTime>,
}

// --- end storage ---

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
    /// Compress new objects (WTZ1); readers always accept both encodings.
    compress: bool,
}

impl ObjectStore {
    /// Open (or lazily create) a store rooted at `root` (e.g. `.wt/objects`).
    /// New objects are compressed when worthwhile (see the module docs).
    pub fn open(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            compress: true,
        }
    }

    /// The same store with compression of new objects switched on or off.
    /// Existing objects are read either way.
    pub fn with_compression(mut self, compress: bool) -> Self {
        self.compress = compress;
        self
    }

    /// Whether new objects are compressed.
    pub fn compresses(&self) -> bool {
        self.compress
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
        let stored = match fs::read(self.path_of(hash)) {
            Ok(d) => d,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SdkError::ObjectNotFound(hash.to_hex()))
            }
            Err(e) => return Err(e.into()),
        };
        decode_verified(hash, stored)
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

    /// Filesystem path where `hash` is (or would be) stored. The file holds
    /// the *stored* encoding (possibly `WTZ1`-compressed): read objects with
    /// [`ObjectStore::get`], never directly.
    pub fn path_of(&self, hash: &ObjectHash) -> PathBuf {
        let hex = hash.to_hex();
        let (dir, file) = hex.split_at(2);
        self.root.join(dir).join(file)
    }

    /// All stored object hashes (unordered).
    pub fn list(&self) -> Result<Vec<ObjectHash>> {
        let mut out = Vec::new();
        self.for_each_object(|obj| out.push(obj.hash))?;
        Ok(out)
    }

    /// (object count, total stored bytes).
    pub fn stats(&self) -> Result<(u64, u64)> {
        let mut count = 0u64;
        let mut bytes = 0u64;
        self.for_each_object(|obj| {
            count += 1;
            bytes += obj.stored_size;
        })?;
        Ok((count, bytes))
    }

    // --- storage (ENGINEERING_PLAN §14.6): sizes, deletion, compaction ---

    /// Every stored object with its stored size and mtime (unordered).
    pub fn objects(&self) -> Result<Vec<StoredObject>> {
        let mut out = Vec::new();
        self.for_each_object(|obj| out.push(obj))?;
        Ok(out)
    }

    /// Stored (on-disk) size of `hash`; `ObjectNotFound` if absent.
    pub fn size(&self, hash: &ObjectHash) -> Result<u64> {
        match fs::metadata(self.path_of(hash)) {
            Ok(m) if m.is_file() => Ok(m.len()),
            Ok(_) => Err(SdkError::ObjectNotFound(hash.to_hex())),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                Err(SdkError::ObjectNotFound(hash.to_hex()))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Whether the stored file of `hash` uses the `WTZ1` encoding.
    pub fn is_compressed(&self, hash: &ObjectHash) -> Result<bool> {
        let mut file = match fs::File::open(self.path_of(hash)) {
            Ok(f) => f,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(SdkError::ObjectNotFound(hash.to_hex()))
            }
            Err(e) => return Err(e.into()),
        };
        let mut head = [0u8; 4];
        let mut filled = 0;
        while filled < head.len() {
            match file.read(&mut head[filled..])? {
                0 => break,
                n => filled += n,
            }
        }
        Ok(filled == head.len() && head == COMPRESSED_MAGIC)
    }

    /// Delete the object `hash`. Returns whether a file was removed.
    pub fn delete(&self, hash: &ObjectHash) -> Result<bool> {
        match fs::remove_file(self.path_of(hash)) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Rewrite a raw object in the compressed encoding when that saves at
    /// least 10 % (no-op for compressed objects, small objects and stores
    /// with compression off). Returns `(old, new)` stored sizes when the
    /// file was rewritten. The content is verified before rewriting.
    pub fn recompress(&self, hash: &ObjectHash) -> Result<Option<(u64, u64)>> {
        if !self.compress || self.is_compressed(hash)? {
            return Ok(None);
        }
        let old = self.size(hash)?;
        if old < COMPRESS_MIN_SIZE as u64 {
            return Ok(None);
        }
        let content = self.get(hash)?;
        let encoded = encode_object(&content)?;
        if let Cow::Borrowed(_) = encoded {
            return Ok(None);
        }
        let target = self.path_of(hash);
        let tmp = fsutil::write_temp(&target, &encoded)?;
        if let Err(e) = fsutil::rename_with_retry(&tmp, &target) {
            let _ = fs::remove_file(&tmp);
            return Err(e.into());
        }
        Ok(Some((old, encoded.len() as u64)))
    }

    /// Stale temp files (interrupted writes) in the fan-out directories that
    /// were last modified before `cutoff`, with their sizes.
    pub fn stale_temp_files(&self, cutoff: SystemTime) -> Result<Vec<(PathBuf, u64)>> {
        let mut out = Vec::new();
        for dir in self.fanout_dirs()? {
            for entry in fs::read_dir(&dir)? {
                let entry = entry?;
                let name = entry.file_name();
                let Some(name) = name.to_str() else { continue };
                if !name.starts_with(fsutil::TEMP_PREFIX) {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let old = meta.modified().map(|m| m < cutoff).unwrap_or(false);
                if meta.is_file() && old {
                    out.push((entry.path(), meta.len()));
                }
            }
        }
        Ok(out)
    }

    /// The two-hex fan-out directories that exist.
    fn fanout_dirs(&self) -> Result<Vec<PathBuf>> {
        let fanout = match fs::read_dir(&self.root) {
            Ok(rd) => rd,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut out = Vec::new();
        for dir in fanout {
            let dir = dir?;
            let name = dir.file_name();
            let Some(prefix) = name.to_str() else {
                continue;
            };
            if prefix.len() == 2
                && prefix.bytes().all(|b| b.is_ascii_hexdigit())
                && dir.file_type()?.is_dir()
            {
                out.push(dir.path());
            }
        }
        Ok(out)
    }

    // --- end storage ---

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
        let encoded = if self.compress {
            encode_object(data)?
        } else {
            encode_object_uncompressed(data)?
        };
        let tmp = fsutil::write_temp(&target, &encoded)?;
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

    /// Visit every well-formed object file with its stored size and mtime.
    /// Temp files and stray entries are skipped.
    fn for_each_object(&self, mut f: impl FnMut(StoredObject)) -> Result<()> {
        for dir in self.fanout_dirs()? {
            let Some(prefix) = dir.file_name().and_then(|n| n.to_str()).map(str::to_string) else {
                continue;
            };
            let entries = match fs::read_dir(&dir) {
                Ok(rd) => rd,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };
            for obj in entries {
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
                let meta = match obj.metadata() {
                    Ok(m) => m,
                    // Deleted concurrently (e.g. by `wt gc`).
                    Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                    Err(e) => return Err(e.into()),
                };
                if meta.is_file() {
                    f(StoredObject {
                        hash,
                        stored_size: meta.len(),
                        modified: meta.modified().ok(),
                    });
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

    // --- storage (ENGINEERING_PLAN §14.6): WTZ1 encoding ---

    /// Shared with `objstore/encoding_test.go`: content of the compatibility
    /// vectors (1296 bytes).
    fn vector_content() -> Vec<u8> {
        "W0rkTree WTZ1 compatibility vector: the quick brown fox jumps over the lazy dog.\n"
            .repeat(16)
            .into_bytes()
    }

    /// BLAKE3 of [`vector_content`].
    const VECTOR_HASH: &str = "bcde4961df8b572156be61dd0a297a62f829f582348332d65ddfa88ef34a3c18";

    /// [`vector_content`] as encoded by the Go server
    /// (`github.com/klauspost/compress/zstd`, `EncodeObject`).
    const GO_VECTOR: &str = "57545a3128b52ffd641004c50200c4045730726b547265652057545a3120636f6d7061746962696c69747920766563746f723a2074686520717569636b2062726f776e20666f78206a756d7073206f7665726c617a7920646f672e0a0200bc50e1a320046506b07d279f";

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn noise(len: usize) -> Vec<u8> {
        let mut state = 0x9e37_79b9_7f4a_7c15u64;
        (0..len)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                (state >> 24) as u8
            })
            .collect()
    }

    #[test]
    fn go_encoded_vector_decodes() {
        let stored = unhex(GO_VECTOR);
        assert!(stored.starts_with(&COMPRESSED_MAGIC));
        let content = decode_object(&stored).unwrap();
        assert_eq!(content.as_ref(), vector_content().as_slice());
        assert_eq!(hash_bytes(&content).to_hex(), VECTOR_HASH);

        // Placed in a store by hand, the server's file is readable as is.
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let h: ObjectHash = VECTOR_HASH.parse().unwrap();
        fs::create_dir_all(store.path_of(&h).parent().unwrap()).unwrap();
        fs::write(store.path_of(&h), &stored).unwrap();
        assert_eq!(store.get(&h).unwrap(), vector_content());
        assert!(store.is_compressed(&h).unwrap());
        assert_eq!(store.size(&h).unwrap(), stored.len() as u64);
    }

    #[test]
    fn rust_encoded_vector_decodes() {
        // RUST_VECTOR was produced by `encode_object` and is decoded by
        // objstore's TestRustEncodedVector; encoder output may change with
        // zstd versions, decoding must not.
        let content = vector_content();
        let stored = unhex(RUST_VECTOR);
        assert_eq!(decode_object(&stored).unwrap(), content);
        let encoded = encode_object(&content).unwrap();
        assert!(encoded.starts_with(&COMPRESSED_MAGIC));
        assert_eq!(decode_object(&encoded).unwrap(), content);
    }

    /// [`vector_content`] as encoded by this crate (zstd 1.5.7, level 3).
    const RUST_VECTOR: &str = "57545a3128b52ffd601004c50200c4045730726b547265652057545a3120636f6d7061746962696c69747920766563746f723a2074686520717569636b2062726f776e20666f78206a756d7073206f7665726c617a7920646f672e0a0200bc50e1a320046506";

    #[test]
    fn compressible_objects_are_stored_compressed() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let content = vector_content();
        let h = store.put(&content).unwrap();
        assert_eq!(h.to_hex(), VECTOR_HASH, "hash is over the content");
        assert!(store.is_compressed(&h).unwrap());
        let stored = fs::read(store.path_of(&h)).unwrap();
        assert!(stored.starts_with(&COMPRESSED_MAGIC));
        assert!(stored.len() < content.len() / 2);
        assert_eq!(store.get(&h).unwrap(), content);
        assert_eq!(store.size(&h).unwrap(), stored.len() as u64);
        assert_eq!(store.stats().unwrap(), (1, stored.len() as u64));
        let objs = store.objects().unwrap();
        assert_eq!(objs.len(), 1);
        assert_eq!(objs[0].stored_size, stored.len() as u64);
        assert!(objs[0].modified.is_some());
    }

    #[test]
    fn threshold_rules() {
        // Below 512 bytes: raw even when compressible.
        let small = vec![b'a'; COMPRESS_MIN_SIZE - 1];
        assert!(matches!(encode_object(&small).unwrap(), Cow::Borrowed(_)));
        // At 512 bytes and compressible: compressed.
        let at = vec![b'a'; COMPRESS_MIN_SIZE];
        assert!(encode_object(&at).unwrap().starts_with(&COMPRESSED_MAGIC));
        // Incompressible: raw (would not save 10 %).
        let random = noise(4096);
        assert!(matches!(encode_object(&random).unwrap(), Cow::Borrowed(_)));
        // Content starting with the magic: always compressed, even tiny.
        let tricky = b"WTZ1 looks like a header".to_vec();
        let enc = encode_object(&tricky).unwrap();
        assert!(enc.starts_with(&COMPRESSED_MAGIC) && enc.len() > tricky.len());
        assert_eq!(decode_object(&enc).unwrap(), tricky);
        // The 10 % rule itself.
        assert!(saves_enough(900, 1000));
        assert!(!saves_enough(901, 1000));
    }

    #[test]
    fn magic_prefixed_content_round_trips_in_both_modes() {
        let dir = tempfile::tempdir().unwrap();
        for compress in [true, false] {
            let store =
                ObjectStore::open(dir.path().join(compress.to_string())).with_compression(compress);
            let mut content = b"WTZ1".to_vec();
            content.extend(noise(2000));
            let h = store.put(&content).unwrap();
            assert!(store.is_compressed(&h).unwrap(), "compress={compress}");
            assert_eq!(store.get(&h).unwrap(), content);
        }
    }

    #[test]
    fn legacy_raw_object_starting_with_magic_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let content = b"WTZ1 but stored raw by an old client".to_vec();
        let h = hash_bytes(&content);
        fs::create_dir_all(store.path_of(&h).parent().unwrap()).unwrap();
        fs::write(store.path_of(&h), &content).unwrap();
        assert_eq!(store.get(&h).unwrap(), content);
    }

    #[test]
    fn mixed_raw_and_compressed_store() {
        let dir = tempfile::tempdir().unwrap();
        let raw_writer = ObjectStore::open(dir.path()).with_compression(false);
        assert!(!raw_writer.compresses());
        let a = raw_writer.put(&vector_content()).unwrap();
        assert!(!raw_writer.is_compressed(&a).unwrap());

        let store = ObjectStore::open(dir.path());
        let mut other = vector_content();
        other.extend_from_slice(b"one more line\n");
        let b = store.put(&other).unwrap();
        let tiny = store.put(b"tiny").unwrap();
        assert!(store.is_compressed(&b).unwrap());
        assert!(!store.is_compressed(&tiny).unwrap());
        // Every reader sees every object regardless of its encoding.
        for s in [&store, &raw_writer] {
            assert_eq!(s.get(&a).unwrap(), vector_content());
            assert_eq!(s.get(&b).unwrap(), other);
            assert_eq!(s.get(&tiny).unwrap(), b"tiny");
        }
        let mut listed = store.list().unwrap();
        listed.sort();
        let mut want = vec![a, b, tiny];
        want.sort();
        assert_eq!(listed, want);
        let (count, before) = store.stats().unwrap();
        assert_eq!(count, 3);

        // Compaction rewrites the raw object; nothing else changes.
        assert_eq!(raw_writer.recompress(&a).unwrap(), None, "compression off");
        let (old, new) = store.recompress(&a).unwrap().expect("rewritten");
        assert_eq!(old, vector_content().len() as u64);
        assert!(new < old);
        assert!(store.is_compressed(&a).unwrap());
        assert_eq!(store.recompress(&a).unwrap(), None);
        assert_eq!(store.recompress(&tiny).unwrap(), None);
        assert_eq!(store.get(&a).unwrap(), vector_content());
        assert_eq!(store.stats().unwrap(), (3, before - old + new));
    }

    #[test]
    fn corrupt_compressed_objects_are_detected() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let h = store.put(&vector_content()).unwrap();
        let path = store.path_of(&h);
        let mut stored = fs::read(&path).unwrap();
        // Flip a byte inside the frame: the frame no longer decodes (or
        // decodes to other content).
        let mid = stored.len() / 2;
        stored[mid] ^= 0x55;
        fs::write(&path, &stored).unwrap();
        assert!(matches!(store.get(&h), Err(SdkError::CorruptObject(..))));
        // Truncated frame.
        fs::write(&path, &stored[..10]).unwrap();
        assert!(matches!(store.get(&h), Err(SdkError::CorruptObject(..))));
    }

    #[test]
    fn size_delete_and_temp_files() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let h = store.put(b"abc").unwrap();
        assert_eq!(store.size(&h).unwrap(), 3);
        assert!(store.delete(&h).unwrap());
        assert!(!store.delete(&h).unwrap());
        assert!(matches!(store.size(&h), Err(SdkError::ObjectNotFound(_))));
        assert!(matches!(
            store.is_compressed(&h),
            Err(SdkError::ObjectNotFound(_))
        ));

        let tmp = store
            .path_of(&h)
            .with_file_name(format!("{}left-over", fsutil::TEMP_PREFIX));
        fs::write(&tmp, b"partial").unwrap();
        let future = SystemTime::now() + std::time::Duration::from_secs(60);
        assert_eq!(store.stale_temp_files(future).unwrap(), vec![(tmp, 7)]);
        let past = SystemTime::now() - std::time::Duration::from_secs(3600);
        assert!(store.stale_temp_files(past).unwrap().is_empty());
    }

    // --- end storage ---

    #[test]
    fn stats_of_missing_root_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path().join("does-not-exist"));
        assert_eq!(store.stats().unwrap(), (0, 0));
    }
}
