//! Pack writer.

use std::collections::HashMap;
use std::io::{self, Write};

use super::{
    delta, delta_pays_off, put_uvarint, Hash, CODEC_RAW, CODEC_ZSTD, MAGIC, MAX_DELTA_BASE,
    MAX_DELTA_DEPTH, MIN_COMPRESS, TYPE_DELTA, TYPE_END, TYPE_WHOLE, VERSION,
};

/// How a [`PackWriter`] encodes payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriterOptions {
    /// zstd-compress payloads of at least [`MIN_COMPRESS`] bytes when that
    /// makes them smaller. Off produces raw (fully deterministic) packs.
    pub compress: bool,
    /// zstd level.
    pub level: i32,
    /// Try deltas at all (off = every entry is WHOLE).
    pub deltas: bool,
}

impl Default for WriterOptions {
    fn default() -> Self {
        Self {
            compress: true,
            level: 3,
            deltas: true,
        }
    }
}

/// What a writer produced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PackStats {
    /// Entries written (duplicates are skipped).
    pub entries: u64,
    /// Entries written as deltas.
    pub deltas: u64,
    /// Total size of the objects carried (what a whole-object transfer
    /// would send).
    pub object_bytes: u64,
    /// Bytes of the pack, header and trailer included.
    pub pack_bytes: u64,
}

impl PackStats {
    /// Bytes the pack saves against sending every object whole and raw.
    pub fn bytes_saved(&self) -> u64 {
        self.object_bytes.saturating_sub(self.pack_bytes)
    }
}

/// Writes a pack to `W` entry by entry.
pub struct PackWriter<W: Write> {
    out: W,
    hasher: blake3::Hasher,
    opts: WriterOptions,
    /// Delta depth of every entry written so far (0 = whole).
    depths: HashMap<Hash, u8>,
    stats: PackStats,
}

impl<W: Write> PackWriter<W> {
    /// Start a pack (writes the header).
    pub fn new(out: W, opts: WriterOptions) -> io::Result<Self> {
        let mut w = Self {
            out,
            hasher: blake3::Hasher::new(),
            opts,
            depths: HashMap::new(),
            stats: PackStats::default(),
        };
        let mut header = Vec::with_capacity(super::HEADER_LEN);
        header.extend_from_slice(&MAGIC);
        header.extend_from_slice(&[VERSION, 0, 0, 0]);
        w.emit(&header)?;
        Ok(w)
    }

    fn emit(&mut self, b: &[u8]) -> io::Result<()> {
        self.out.write_all(b)?;
        self.hasher.update(b);
        self.stats.pack_bytes += b.len() as u64;
        Ok(())
    }

    /// Whether `hash` was already written to this pack.
    pub fn contains(&self, hash: &Hash) -> bool {
        self.depths.contains_key(hash)
    }

    /// Add an object (`kind` = `ObjectKind` wire value). With a `base`
    /// (an object the receiver has, or an earlier entry of this pack) the
    /// object is sent as a delta when that saves at least 25 % and the
    /// chain stays within [`MAX_DELTA_DEPTH`]; otherwise whole. Returns
    /// whether a delta was written. Objects already in the pack are
    /// skipped.
    pub fn add(
        &mut self,
        hash: Hash,
        kind: u8,
        data: &[u8],
        base: Option<(&Hash, &[u8])>,
    ) -> io::Result<bool> {
        if self.depths.contains_key(&hash) {
            return Ok(false);
        }
        let opts = self.opts;
        let delta = match base {
            Some((base_hash, base_data)) if self.depth_allows(base_hash) => {
                delta_candidate(&opts, &hash, data, base_hash, base_data)?.map(|p| (base_hash, p))
            }
            _ => None,
        };
        self.add_encoded(hash, kind, data.len(), delta, || {
            Payload::encode(&opts, data)
        })
    }

    /// Whether a delta against `base` keeps the chain within
    /// [`MAX_DELTA_DEPTH`].
    fn depth_allows(&self, base: &Hash) -> bool {
        self.depths.get(base).copied().unwrap_or(0) < MAX_DELTA_DEPTH
    }

    /// [`PackWriter::add`] with payloads prepared ahead of time (for
    /// example on worker threads, see [`delta_candidate`] and
    /// [`Payload::encode`]). `size` is the object's size; `delta` is the
    /// candidate delta against a base (written when the chain depth allows
    /// it), `whole` produces the whole payload otherwise. The output is
    /// byte-identical to [`PackWriter::add`] given the same inputs.
    pub fn add_encoded(
        &mut self,
        hash: Hash,
        kind: u8,
        size: usize,
        delta: Option<(&Hash, Payload)>,
        whole: impl FnOnce() -> io::Result<Payload>,
    ) -> io::Result<bool> {
        if self.depths.contains_key(&hash) {
            return Ok(false);
        }
        self.stats.entries += 1;
        self.stats.object_bytes += size as u64;
        if let Some((base_hash, payload)) = delta.filter(|(b, _)| self.depth_allows(b)) {
            let depth = self.depths.get(base_hash).copied().unwrap_or(0);
            self.entry(TYPE_DELTA, kind, &hash, Some(base_hash), size, &payload)?;
            self.depths.insert(hash, depth + 1);
            self.stats.deltas += 1;
            return Ok(true);
        }
        let payload = whole()?;
        self.entry(TYPE_WHOLE, kind, &hash, None, size, &payload)?;
        self.depths.insert(hash, 0);
        Ok(false)
    }

    fn entry(
        &mut self,
        typ: u8,
        kind: u8,
        hash: &Hash,
        base: Option<&Hash>,
        size: usize,
        payload: &Payload,
    ) -> io::Result<()> {
        let mut head = Vec::with_capacity(3 + 64 + 30);
        head.extend_from_slice(&[typ, kind, payload.codec]);
        head.extend_from_slice(hash);
        if let Some(b) = base {
            head.extend_from_slice(b);
        }
        put_uvarint(&mut head, size as u64);
        put_uvarint(&mut head, payload.raw_len as u64);
        put_uvarint(&mut head, payload.bytes.len() as u64);
        self.emit(&head)?;
        self.emit(&payload.bytes)
    }

    /// Write the end marker and trailer; returns the sink and the stats.
    pub fn finish(mut self) -> io::Result<(W, PackStats)> {
        self.emit(&[TYPE_END])?;
        let digest = *self.hasher.finalize().as_bytes();
        self.out.write_all(&digest)?;
        self.stats.pack_bytes += digest.len() as u64;
        self.out.flush()?;
        Ok((self.out, self.stats))
    }
}

/// An entry payload as a [`PackWriter`] writes it: zstd-compressed when the
/// options ask for it and that makes it smaller, raw otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payload {
    codec: u8,
    raw_len: usize,
    bytes: Vec<u8>,
}

impl Payload {
    /// Encode `raw` (an object, or a delta) under `opts`.
    pub fn encode(opts: &WriterOptions, raw: &[u8]) -> io::Result<Self> {
        if opts.compress && raw.len() >= MIN_COMPRESS {
            let z = zstd::bulk::compress(raw, opts.level)?;
            if z.len() < raw.len() {
                return Ok(Self {
                    codec: CODEC_ZSTD,
                    raw_len: raw.len(),
                    bytes: z,
                });
            }
        }
        Ok(Self {
            codec: CODEC_RAW,
            raw_len: raw.len(),
            bytes: raw.to_vec(),
        })
    }
}

/// The encoded delta of `data` against `base` if the writer would send it
/// as a delta, chain depth aside (that is checked when the entry is added):
/// deltas enabled, a distinct base within [`MAX_DELTA_BASE`], and a delta
/// saving at least 25 %. Pure, so it can run on worker threads.
pub fn delta_candidate(
    opts: &WriterOptions,
    hash: &Hash,
    data: &[u8],
    base_hash: &Hash,
    base: &[u8],
) -> io::Result<Option<Payload>> {
    if !opts.deltas || base_hash == hash || data.len() < delta::BLOCK || base.len() > MAX_DELTA_BASE
    {
        return Ok(None);
    }
    let d = delta::encode(base, data);
    if !delta_pays_off(d.len(), data.len()) {
        return Ok(None);
    }
    Payload::encode(opts, &d).map(Some)
}
