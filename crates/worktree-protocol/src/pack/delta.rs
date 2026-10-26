//! Copy/insert binary deltas.
//!
//! ```text
//! delta  = base_len:uvarint target_len:uvarint op*
//! op     = 0x00 len:uvarint bytes:len          INSERT (len >= 1)
//!        | 0x01 offset:uvarint len:uvarint      COPY base[offset .. offset+len] (len >= 1)
//! ```
//!
//! [`encode`] is deterministic and specified exactly (the Go server
//! implements the same algorithm, and the golden vectors check that both
//! produce identical bytes):
//!
//! 1. If the base or the target is shorter than [`BLOCK`] bytes, or the
//!    base is longer than [`super::MAX_DELTA_BASE`], the delta is one
//!    INSERT of the whole target (none for an empty target).
//! 2. Index: for every block-aligned offset `i = 0, 16, 32, ...` with
//!    `i + 16 <= base_len`, `fingerprint(base[i..i+16])` maps to `i`; the
//!    first offset wins.
//! 3. Scan the target with a rolling fingerprint from position `p = 0`
//!    (pending insert start `s = 0`) while `p + 16 <= target_len`: when the
//!    window's fingerprint is indexed at `o` and `base[o..o+16]` equals the
//!    window, extend the match backwards (not before `s`, not before base
//!    offset 0) and forwards (as far as both sides agree), emit the pending
//!    INSERT `target[s..start]` if non-empty and a COPY, and continue after
//!    the match with a fresh fingerprint; otherwise roll the window by one
//!    byte.
//! 4. Emit the remaining `target[s..]` as an INSERT.
//!
//! `fingerprint(w) = Σ w[j] · P^(15-j) mod 2^32` with `P = 16777619`.

use std::collections::HashMap;

use super::{put_uvarint, uvarint, PackError, MAX_DELTA_BASE};

/// Block (window) length of the delta index.
pub const BLOCK: usize = 16;
/// Multiplier of the rolling fingerprint (the 32-bit FNV prime).
pub const PRIME: u32 = 16_777_619;

const OP_INSERT: u8 = 0;
const OP_COPY: u8 = 1;

/// Fingerprint of one window (`w.len()` is [`BLOCK`] in the encoder).
pub fn fingerprint(w: &[u8]) -> u32 {
    w.iter().fold(0u32, |h, &b| {
        h.wrapping_mul(PRIME).wrapping_add(u32::from(b))
    })
}

fn insert(out: &mut Vec<u8>, data: &[u8]) {
    out.push(OP_INSERT);
    put_uvarint(out, data.len() as u64);
    out.extend_from_slice(data);
}

fn copy(out: &mut Vec<u8>, offset: usize, len: usize) {
    out.push(OP_COPY);
    put_uvarint(out, offset as u64);
    put_uvarint(out, len as u64);
}

/// Encode `target` as a delta against `base` (see the module docs).
pub fn encode(base: &[u8], target: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32);
    put_uvarint(&mut out, base.len() as u64);
    put_uvarint(&mut out, target.len() as u64);
    if base.len() < BLOCK || target.len() < BLOCK || base.len() > MAX_DELTA_BASE {
        if !target.is_empty() {
            insert(&mut out, target);
        }
        return out;
    }

    let mut index: HashMap<u32, usize> = HashMap::with_capacity(base.len() / BLOCK);
    let mut i = 0;
    while i + BLOCK <= base.len() {
        index.entry(fingerprint(&base[i..i + BLOCK])).or_insert(i);
        i += BLOCK;
    }
    // PRIME^(BLOCK-1): weight of the byte leaving the window.
    let out_weight = (1..BLOCK).fold(1u32, |w, _| w.wrapping_mul(PRIME));

    let (mut p, mut s) = (0usize, 0usize);
    let mut h = fingerprint(&target[..BLOCK]);
    while p + BLOCK <= target.len() {
        if let Some(&o) = index.get(&h) {
            if base[o..o + BLOCK] == target[p..p + BLOCK] {
                let (mut bp, mut bo) = (p, o);
                while bp > s && bo > 0 && base[bo - 1] == target[bp - 1] {
                    bp -= 1;
                    bo -= 1;
                }
                let (mut e, mut eo) = (p + BLOCK, o + BLOCK);
                while e < target.len() && eo < base.len() && target[e] == base[eo] {
                    e += 1;
                    eo += 1;
                }
                if bp > s {
                    insert(&mut out, &target[s..bp]);
                }
                copy(&mut out, bo, e - bp);
                p = e;
                s = e;
                if p + BLOCK <= target.len() {
                    h = fingerprint(&target[p..p + BLOCK]);
                }
                continue;
            }
        }
        if p + BLOCK < target.len() {
            h = h
                .wrapping_sub(u32::from(target[p]).wrapping_mul(out_weight))
                .wrapping_mul(PRIME)
                .wrapping_add(u32::from(target[p + BLOCK]));
        }
        p += 1;
    }
    if s < target.len() {
        insert(&mut out, &target[s..]);
    }
    out
}

/// The target size a delta declares (`None` if the header is malformed).
pub fn target_len(delta: &[u8]) -> Option<u64> {
    let (_, n) = uvarint(delta)?;
    uvarint(&delta[n..]).map(|(v, _)| v)
}

/// Apply `delta` to `base`. The result is at most `max_out` bytes; every
/// malformed delta (wrong base size, out-of-range copy, truncated op,
/// length mismatch) is an error.
pub fn apply(base: &[u8], delta: &[u8], max_out: u64) -> Result<Vec<u8>, PackError> {
    let bad = |m: &str| PackError::corrupt(format!("delta: {m}"));
    let (base_len, n) = uvarint(delta).ok_or_else(|| bad("bad base size"))?;
    let mut pos = n;
    if base_len != base.len() as u64 {
        return Err(bad("base size mismatch"));
    }
    let (target_len, n) = uvarint(&delta[pos..]).ok_or_else(|| bad("bad target size"))?;
    pos += n;
    if target_len > max_out {
        return Err(bad("target too large"));
    }
    let target_len = target_len as usize;
    let mut out = Vec::with_capacity(target_len);
    while pos < delta.len() {
        let op = delta[pos];
        pos += 1;
        match op {
            OP_INSERT => {
                let (len, n) = uvarint(&delta[pos..]).ok_or_else(|| bad("bad insert length"))?;
                pos += n;
                let len = usize::try_from(len).map_err(|_| bad("insert too long"))?;
                if len == 0 || len > delta.len() - pos || len > target_len - out.len() {
                    return Err(bad("insert out of range"));
                }
                out.extend_from_slice(&delta[pos..pos + len]);
                pos += len;
            }
            OP_COPY => {
                let (off, n) = uvarint(&delta[pos..]).ok_or_else(|| bad("bad copy offset"))?;
                pos += n;
                let (len, n) = uvarint(&delta[pos..]).ok_or_else(|| bad("bad copy length"))?;
                pos += n;
                let end = off
                    .checked_add(len)
                    .ok_or_else(|| bad("copy out of range"))?;
                if len == 0 || end > base.len() as u64 || len > (target_len - out.len()) as u64 {
                    return Err(bad("copy out of range"));
                }
                out.extend_from_slice(&base[off as usize..end as usize]);
            }
            _ => return Err(bad("unknown op")),
        }
    }
    if out.len() != target_len {
        return Err(bad("target size mismatch"));
    }
    Ok(out)
}
