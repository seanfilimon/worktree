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
        self.stats.entries += 1;
        self.stats.object_bytes += data.len() as u64;
        if let Some((base_hash, base_data)) = base.filter(|(b, _)| self.opts.deltas && **b != hash)
        {
            let depth = self.depths.get(base_hash).copied().unwrap_or(0);
            if depth < MAX_DELTA_DEPTH
                && data.len() >= delta::BLOCK
                && base_data.len() <= MAX_DELTA_BASE
            {
                let d = delta::encode(base_data, data);
                if delta_pays_off(d.len(), data.len()) {
                    self.entry(TYPE_DELTA, kind, &hash, Some(base_hash), data.len(), &d)?;
                    self.depths.insert(hash, depth + 1);
                    self.stats.deltas += 1;
                    return Ok(true);
                }
            }
        }
        self.entry(TYPE_WHOLE, kind, &hash, None, data.len(), data)?;
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
        raw: &[u8],
    ) -> io::Result<()> {
        let compressed = if self.opts.compress && raw.len() >= MIN_COMPRESS {
            let z = zstd::bulk::compress(raw, self.opts.level)?;
            (z.len() < raw.len()).then_some(z)
        } else {
            None
        };
        let (codec, payload) = match &compressed {
            Some(z) => (CODEC_ZSTD, z.as_slice()),
            None => (CODEC_RAW, raw),
        };
        let mut head = Vec::with_capacity(3 + 64 + 30);
        head.extend_from_slice(&[typ, kind, codec]);
        head.extend_from_slice(hash);
        if let Some(b) = base {
            head.extend_from_slice(b);
        }
        put_uvarint(&mut head, size as u64);
        put_uvarint(&mut head, raw.len() as u64);
        put_uvarint(&mut head, payload.len() as u64);
        self.emit(&head)?;
        self.emit(payload)
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
