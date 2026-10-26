//! Pack format with binary deltas (docs/ENGINEERING_PLAN.md §16.1,
//! `specs/sync/Packs.md`).
//!
//! A pack is one byte stream carrying content-addressed objects:
//!
//! ```text
//! pack    = header entry* end trailer
//! header  = "WTPK" version:u8(=1) reserved:3×u8(=0)
//! entry   = type:u8 kind:u8 codec:u8 hash:32 [base:32 if type=DELTA]
//!           size:uvarint raw_len:uvarint pay_len:uvarint payload:pay_len
//! end     = 0x00
//! trailer = BLAKE3(header .. end):32
//! ```
//!
//! `type` is 1 (WHOLE: the payload decodes to the object) or 2 (DELTA: the
//! payload decodes to a copy/insert delta against the object `base`, see
//! [`delta`]). `kind` is the `ObjectKind` wire value (0-3). `codec` is 0
//! (raw) or 1 (one zstd frame of `raw_len` bytes). `size` is the object
//! size; `hash` is BLAKE3 of the object content. Integers are unsigned
//! LEB128 varints (protobuf encoding).
//!
//! Bases are objects the receiver already has or earlier entries of the
//! same pack; delta chains are at most [`MAX_DELTA_DEPTH`] deep (an object
//! the receiver has counts as depth 0). The Go server implements the same
//! format (`services/server-go/internal/syncsvc/pack_format.go`); the
//! golden vectors in `testdata/pack/vectors.json` keep both byte for byte
//! identical for raw (uncompressed) packs and the delta encoder, and each
//! side decodes the other's zstd packs.

pub mod delta;
mod reader;
mod writer;

#[cfg(test)]
mod tests;

pub use reader::{PackObject, PackReader};
pub use writer::{PackStats, PackWriter, WriterOptions};

/// First four bytes of every pack.
pub const MAGIC: [u8; 4] = *b"WTPK";
/// Format version written in the header.
pub const VERSION: u8 = 1;
/// Header length (magic, version, three reserved zero bytes).
pub const HEADER_LEN: usize = 8;
/// Trailer length (BLAKE3 digest of everything before it).
pub const TRAILER_LEN: usize = 32;

/// Entry type byte: end of entries.
pub const TYPE_END: u8 = 0;
/// Entry type byte: the payload is the whole object.
pub const TYPE_WHOLE: u8 = 1;
/// Entry type byte: the payload is a delta against `base`.
pub const TYPE_DELTA: u8 = 2;

/// Payload codec: stored as is.
pub const CODEC_RAW: u8 = 0;
/// Payload codec: one zstd frame.
pub const CODEC_ZSTD: u8 = 1;

/// Largest object a pack may carry (the 64 MiB object limit of the sync
/// protocol).
pub const MAX_OBJECT_SIZE: u64 = 64 << 20;
/// Largest decoded or encoded payload a reader accepts (an object plus
/// slack for incompressible payloads and delta headers).
pub const MAX_PAYLOAD: u64 = MAX_OBJECT_SIZE + (1 << 20);
/// Longest delta chain (an object the receiver has is depth 0).
pub const MAX_DELTA_DEPTH: u8 = 8;
/// Largest base a delta is computed against (bounded window); larger
/// objects are sent whole.
pub const MAX_DELTA_BASE: usize = 16 << 20;
/// Largest `PackChunk.data` on the wire (both directions).
pub const MAX_CHUNK: usize = 4 << 20;
/// Payloads shorter than this are never compressed.
pub const MIN_COMPRESS: usize = 64;

/// A 32-byte BLAKE3 object hash.
pub type Hash = [u8; 32];

/// BLAKE3 of `data`.
pub fn hash(data: &[u8]) -> Hash {
    *blake3::hash(data).as_bytes()
}

/// Whether a delta of `delta_len` bytes is worth sending instead of the
/// whole `target_len`-byte object: it must save at least 25 %.
pub fn delta_pays_off(delta_len: usize, target_len: usize) -> bool {
    (delta_len as u128) * 4 <= (target_len as u128) * 3
}

/// Errors that make a pack (not just one entry) unusable.
#[derive(Debug, thiserror::Error)]
pub enum PackError {
    /// The stream ended before the trailer (an interrupted upload).
    #[error("pack is truncated")]
    Truncated,
    /// The trailer does not match the content.
    #[error("pack checksum mismatch")]
    Checksum,
    /// Malformed framing.
    #[error("corrupt pack: {0}")]
    Corrupt(String),
    /// An underlying read or write failed.
    #[error("pack I/O: {0}")]
    Io(#[from] std::io::Error),
}

impl PackError {
    pub(crate) fn corrupt(msg: impl Into<String>) -> Self {
        PackError::Corrupt(msg.into())
    }
}

/// Append `v` as an unsigned LEB128 varint.
pub fn put_uvarint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Decode an unsigned LEB128 varint from the front of `b`: `(value, bytes
/// read)`, `None` when `b` ends early or the value overflows 64 bits (the
/// semantics of Go's `binary.Uvarint`).
pub fn uvarint(b: &[u8]) -> Option<(u64, usize)> {
    let mut x = 0u64;
    let mut s = 0u32;
    for (i, &byte) in b.iter().enumerate() {
        if i == 10 {
            return None;
        }
        if byte < 0x80 {
            if i == 9 && byte > 1 {
                return None;
            }
            return Some((x | (u64::from(byte) << s), i + 1));
        }
        x |= u64::from(byte & 0x7f) << s;
        s += 7;
    }
    None
}
