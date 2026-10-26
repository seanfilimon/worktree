//! Pack reader.

use std::collections::HashMap;
use std::io::{self, BufReader, Read};

use super::{
    delta, Hash, PackError, CODEC_RAW, CODEC_ZSTD, HEADER_LEN, MAGIC, MAX_DELTA_DEPTH,
    MAX_OBJECT_SIZE, MAX_PAYLOAD, TYPE_DELTA, TYPE_END, TYPE_WHOLE, VERSION,
};

/// One object read from a pack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackObject {
    pub hash: Hash,
    /// `ObjectKind` wire value.
    pub kind: u8,
    /// The delta base, for DELTA entries.
    pub base: Option<Hash>,
    /// The verified object content, or why this entry cannot be used (a
    /// missing base, a delta that does not apply, a hash mismatch). Such
    /// entries do not make the rest of the pack unusable.
    pub data: Result<Vec<u8>, String>,
}

/// Reads a pack entry by entry, verifying every object hash, the delta
/// depth limit and (at the end) the trailer checksum.
pub struct PackReader<R: Read> {
    r: BufReader<R>,
    hasher: blake3::Hasher,
    consumed: u64,
    depths: HashMap<Hash, u8>,
    done: bool,
}

fn read_err(e: io::Error) -> PackError {
    if e.kind() == io::ErrorKind::UnexpectedEof {
        PackError::Truncated
    } else {
        PackError::Io(e)
    }
}

impl<R: Read> PackReader<R> {
    /// Read and check the header.
    pub fn new(r: R) -> Result<Self, PackError> {
        let mut pr = Self {
            r: BufReader::with_capacity(256 * 1024, r),
            hasher: blake3::Hasher::new(),
            consumed: 0,
            depths: HashMap::new(),
            done: false,
        };
        let mut header = [0u8; HEADER_LEN];
        pr.fill(&mut header)?;
        if header[..4] != MAGIC {
            return Err(PackError::corrupt("not a pack (bad magic)"));
        }
        if header[4] != VERSION {
            return Err(PackError::corrupt(format!(
                "unsupported pack version {}",
                header[4]
            )));
        }
        if header[5..] != [0, 0, 0] {
            return Err(PackError::corrupt("reserved header bytes are not zero"));
        }
        Ok(pr)
    }

    /// Bytes consumed so far.
    pub fn consumed(&self) -> u64 {
        self.consumed
    }

    fn fill(&mut self, buf: &mut [u8]) -> Result<(), PackError> {
        self.r.read_exact(buf).map_err(read_err)?;
        self.hasher.update(buf);
        self.consumed += buf.len() as u64;
        Ok(())
    }

    fn byte(&mut self) -> Result<u8, PackError> {
        let mut b = [0u8; 1];
        self.fill(&mut b)?;
        Ok(b[0])
    }

    fn uvarint(&mut self) -> Result<u64, PackError> {
        let mut x = 0u64;
        let mut s = 0u32;
        for i in 0..10 {
            let b = self.byte()?;
            if b < 0x80 {
                if i == 9 && b > 1 {
                    break;
                }
                return Ok(x | (u64::from(b) << s));
            }
            x |= u64::from(b & 0x7f) << s;
            s += 7;
        }
        Err(PackError::corrupt("varint overflows 64 bits"))
    }

    /// The next object, or `None` after the end marker (whose trailer
    /// is then verified, and nothing may follow it). `lookup` returns the
    /// content of a delta base: an earlier object of this pack or one the
    /// receiver has.
    pub fn next_object(
        &mut self,
        mut lookup: impl FnMut(&Hash) -> Option<Vec<u8>>,
    ) -> Result<Option<PackObject>, PackError> {
        if self.done {
            return Ok(None);
        }
        let typ = self.byte()?;
        if typ == TYPE_END {
            self.finish_trailer()?;
            return Ok(None);
        }
        if typ != TYPE_WHOLE && typ != TYPE_DELTA {
            return Err(PackError::corrupt(format!("unknown entry type {typ}")));
        }
        let kind = self.byte()?;
        if kind > 3 {
            return Err(PackError::corrupt(format!("unknown object kind {kind}")));
        }
        let codec = self.byte()?;
        if codec != CODEC_RAW && codec != CODEC_ZSTD {
            return Err(PackError::corrupt(format!("unknown codec {codec}")));
        }
        let mut hash = [0u8; 32];
        self.fill(&mut hash)?;
        let base = if typ == TYPE_DELTA {
            let mut b = [0u8; 32];
            self.fill(&mut b)?;
            Some(b)
        } else {
            None
        };
        let size = self.uvarint()?;
        let raw_len = self.uvarint()?;
        let pay_len = self.uvarint()?;
        if size > MAX_OBJECT_SIZE {
            return Err(PackError::corrupt("object larger than 64 MiB"));
        }
        if raw_len > MAX_PAYLOAD || pay_len > MAX_PAYLOAD {
            return Err(PackError::corrupt("payload too large"));
        }
        if typ == TYPE_WHOLE && raw_len != size {
            return Err(PackError::corrupt("whole entry size mismatch"));
        }
        if codec == CODEC_RAW && pay_len != raw_len {
            return Err(PackError::corrupt("raw payload length mismatch"));
        }
        let mut payload = vec![0u8; pay_len as usize];
        self.fill(&mut payload)?;
        let raw = if codec == CODEC_ZSTD {
            let out = zstd::bulk::decompress(&payload, raw_len as usize)
                .map_err(|e| PackError::corrupt(format!("zstd: {e}")))?;
            if out.len() as u64 != raw_len {
                return Err(PackError::corrupt("zstd payload length mismatch"));
            }
            out
        } else {
            payload
        };

        let depth = match &base {
            None => 0,
            Some(b) => self.depths.get(b).copied().unwrap_or(0) + 1,
        };
        if depth > MAX_DELTA_DEPTH {
            return Err(PackError::corrupt(format!(
                "delta chain deeper than {MAX_DELTA_DEPTH}"
            )));
        }
        self.depths.insert(hash, depth);

        let data = match &base {
            None => Ok(raw),
            Some(b) => match lookup(b) {
                None => Err(format!("delta base {} is not available", hex(b))),
                Some(base_data) => delta::apply(&base_data, &raw, size).map_err(|e| e.to_string()),
            },
        };
        let data = data.and_then(|d| {
            if d.len() as u64 != size {
                Err("object size mismatch".to_string())
            } else if super::hash(&d) != hash {
                Err("content does not match its hash".to_string())
            } else {
                Ok(d)
            }
        });
        Ok(Some(PackObject {
            hash,
            kind,
            base,
            data,
        }))
    }

    fn finish_trailer(&mut self) -> Result<(), PackError> {
        let digest = *self.hasher.finalize().as_bytes();
        let mut trailer = [0u8; 32];
        self.r.read_exact(&mut trailer).map_err(read_err)?;
        self.consumed += trailer.len() as u64;
        if trailer != digest {
            return Err(PackError::Checksum);
        }
        let mut extra = [0u8; 1];
        match self.r.read(&mut extra) {
            Ok(0) => {}
            Ok(_) => return Err(PackError::corrupt("data after the trailer")),
            Err(e) => return Err(PackError::Io(e)),
        }
        self.done = true;
        Ok(())
    }
}

fn hex(h: &Hash) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}
