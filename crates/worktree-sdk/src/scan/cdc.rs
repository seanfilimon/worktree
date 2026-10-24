//! FastCDC content-defined chunking.
//!
//! Implements the FastCDC algorithm (Xia et al., USENIX ATC '16) with gear
//! hashing, cut-point skipping (the first `min` bytes of every chunk are not
//! hashed) and normalized chunking: before the target average size a
//! *stricter* mask (`mask_s`, `log2(avg) + 1` bits) is used, after it a
//! *looser* one (`mask_l`, `log2(avg) - 1` bits). This concentrates chunk
//! sizes around the average while keeping boundaries content-defined, so an
//! edit only disturbs the chunks near it.
//!
//! The gear table is derived from a fixed seed and is part of the on-disk
//! format: changing it (or the masks) changes every chunk boundary and
//! defeats deduplication against previously stored chunks.

/// Minimum chunk size used by [`super::chunk_boundaries`] (256 KiB).
pub const MIN_CHUNK_SIZE: usize = 256 * 1024;
/// Target average chunk size used by [`super::chunk_boundaries`] (1 MiB).
pub const AVG_CHUNK_SIZE: usize = 1024 * 1024;
/// Maximum chunk size used by [`super::chunk_boundaries`] (4 MiB).
pub const MAX_CHUNK_SIZE: usize = 4 * 1024 * 1024;

/// Seed for the gear table ("WT_GEAR1" in ASCII). Frozen: see module docs.
const GEAR_SEED: u64 = 0x5754_5f47_4541_5231;

/// 256 pseudo-random 64-bit values, one per byte value, generated at compile
/// time with SplitMix64.
static GEAR: [u64; 256] = gear_table(GEAR_SEED);

const fn gear_table(seed: u64) -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut state = seed;
    let mut i = 0;
    while i < 256 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        table[i] = z ^ (z >> 31);
        i += 1;
    }
    table
}

/// A mask selecting the `n` most significant bits of the gear hash.
///
/// The gear hash is updated as `h = (h << 1) + GEAR[b]`, so its high bits
/// depend on the longest window of recent bytes (bit 63 on the last 64
/// bytes). Testing high bits therefore gives the widest effective window.
const fn high_bits(n: u32) -> u64 {
    if n == 0 {
        0
    } else if n >= 64 {
        u64::MAX
    } else {
        u64::MAX << (64 - n)
    }
}

/// Content-defined chunk boundaries with explicit size parameters.
///
/// Returns `(offset, len)` pairs that cover `data` exactly, in order. Every
/// chunk is at most `max` bytes, and every chunk except the last is at
/// least `min` bytes. Inputs of at most `min` bytes yield a single chunk;
/// empty input yields no chunks. Output is a pure function of the input
/// bytes and parameters.
///
/// `avg` is the target average size; it is rounded down to a power of two
/// for mask selection.
///
/// # Panics
///
/// Panics unless `0 < min <= avg <= max`.
pub fn chunk_boundaries_with(
    data: &[u8],
    min: usize,
    avg: usize,
    max: usize,
) -> Vec<(usize, usize)> {
    assert!(
        min > 0 && min <= avg && avg <= max,
        "chunk sizes must satisfy 0 < min <= avg <= max (got min={min}, avg={avg}, max={max})"
    );
    if data.is_empty() {
        return Vec::new();
    }

    let bits = avg.ilog2();
    let mask_s = high_bits(bits + 1);
    let mask_l = high_bits(bits.saturating_sub(1));

    let mut out = Vec::with_capacity(data.len() / avg + 1);
    let mut offset = 0;
    while offset < data.len() {
        let len = cut_point(&data[offset..], min, avg, max, mask_s, mask_l);
        out.push((offset, len));
        offset += len;
    }
    out
}

/// Length of the next chunk at the start of `data` (always `>= 1` for
/// non-empty input).
fn cut_point(data: &[u8], min: usize, avg: usize, max: usize, mask_s: u64, mask_l: u64) -> usize {
    let len = data.len();
    if len <= min {
        return len;
    }
    let end = len.min(max);
    let normal = avg.clamp(min, end);

    let mut hash = 0u64;
    for (i, &b) in data.iter().enumerate().take(normal).skip(min) {
        hash = (hash << 1).wrapping_add(GEAR[b as usize]);
        if hash & mask_s == 0 {
            return i + 1;
        }
    }
    for (i, &b) in data.iter().enumerate().take(end).skip(normal) {
        hash = (hash << 1).wrapping_add(GEAR[b as usize]);
        if hash & mask_l == 0 {
            return i + 1;
        }
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// Deterministic pseudo-random bytes (xorshift64*).
    fn random_bytes(seed: u64, len: usize) -> Vec<u8> {
        let mut state = seed | 1;
        let mut out = Vec::with_capacity(len + 8);
        while out.len() < len {
            state ^= state >> 12;
            state ^= state << 25;
            state ^= state >> 27;
            out.extend_from_slice(&state.wrapping_mul(0x2545_F491_4F6C_DD1D).to_le_bytes());
        }
        out.truncate(len);
        out
    }

    fn assert_covers(data: &[u8], chunks: &[(usize, usize)]) {
        let mut expected = 0;
        for &(off, len) in chunks {
            assert_eq!(off, expected, "chunks must be contiguous");
            assert!(len > 0, "chunks must be non-empty");
            expected += len;
        }
        assert_eq!(expected, data.len(), "chunks must cover the input exactly");
    }

    fn assert_bounds(chunks: &[(usize, usize)], min: usize, max: usize) {
        for (i, &(_, len)) in chunks.iter().enumerate() {
            assert!(len <= max, "chunk {i} exceeds max: {len}");
            if i + 1 < chunks.len() {
                assert!(len >= min, "non-final chunk {i} below min: {len}");
            }
        }
    }

    #[test]
    fn empty_input_has_no_chunks() {
        assert!(chunk_boundaries_with(&[], 64, 256, 1024).is_empty());
    }

    #[test]
    fn small_inputs_are_a_single_chunk() {
        for len in [1usize, 10, 63, 64] {
            let data = random_bytes(len as u64, len);
            assert_eq!(chunk_boundaries_with(&data, 64, 256, 1024), vec![(0, len)]);
        }
    }

    #[test]
    fn default_sizes_cover_and_respect_bounds() {
        let data = random_bytes(42, 12 * 1024 * 1024 + 12345);
        let chunks = super::super::chunk_boundaries(&data);
        assert_covers(&data, &chunks);
        assert_bounds(&chunks, MIN_CHUNK_SIZE, MAX_CHUNK_SIZE);
        assert!(
            chunks.len() >= 3,
            "12 MiB of random data should split, got {chunks:?}"
        );
    }

    #[test]
    fn small_sizes_cover_and_respect_bounds() {
        let (min, avg, max) = (512, 2048, 8192);
        for seed in 1..=5 {
            let data = random_bytes(seed, 300_000 + seed as usize * 777);
            let chunks = chunk_boundaries_with(&data, min, avg, max);
            assert_covers(&data, &chunks);
            assert_bounds(&chunks, min, max);
        }
    }

    #[test]
    fn deterministic() {
        let data = random_bytes(7, 500_000);
        let a = chunk_boundaries_with(&data, 1024, 4096, 16384);
        let b = chunk_boundaries_with(&data.clone(), 1024, 4096, 16384);
        assert_eq!(a, b);
    }

    #[test]
    fn average_chunk_size_is_near_target() {
        let (min, avg, max) = (1024, 4096, 16384);
        let data = random_bytes(99, 4 * 1024 * 1024);
        let chunks = chunk_boundaries_with(&data, min, avg, max);
        let mean = data.len() / chunks.len();
        assert!(
            (avg / 2..=avg * 2).contains(&mean),
            "mean chunk size {mean} far from target {avg}"
        );
    }

    #[test]
    fn low_entropy_input_is_cut_at_max() {
        let data = vec![0u8; 100_000];
        let (min, max) = (1000, 8000);
        let chunks = chunk_boundaries_with(&data, min, 4000, max);
        assert_covers(&data, &chunks);
        assert_bounds(&chunks, min, max);
    }

    #[test]
    fn shift_resistant_to_prefix_insertion() {
        let (min, avg, max) = (1024, 4096, 16384);
        let original = random_bytes(1234, 1024 * 1024);
        let mut shifted = random_bytes(5678, 37);
        shifted.extend_from_slice(&original);

        let a = chunk_boundaries_with(&original, min, avg, max);
        let b = chunk_boundaries_with(&shifted, min, avg, max);
        let a_chunks: HashSet<&[u8]> = a.iter().map(|&(o, l)| &original[o..o + l]).collect();
        let b_chunks: HashSet<&[u8]> = b.iter().map(|&(o, l)| &shifted[o..o + l]).collect();
        let changed = a_chunks.difference(&b_chunks).count();
        assert!(
            changed <= 3,
            "prefix insertion changed {changed} of {} chunks",
            a.len()
        );
        // Everything after the resynchronization point is identical, just
        // shifted by the inserted length.
        let tail_a: Vec<usize> = a.iter().rev().take(a.len() - 3).map(|c| c.1).collect();
        let tail_b: Vec<usize> = b.iter().rev().take(a.len() - 3).map(|c| c.1).collect();
        assert_eq!(tail_a, tail_b);
    }

    #[test]
    fn local_edit_only_changes_nearby_chunks() {
        let (min, avg, max) = (1024, 4096, 16384);
        let original = random_bytes(31337, 1024 * 1024);
        let mut edited = original.clone();
        for b in &mut edited[500_000..500_010] {
            *b ^= 0xFF;
        }
        let a = chunk_boundaries_with(&original, min, avg, max);
        let b = chunk_boundaries_with(&edited, min, avg, max);
        let a_chunks: HashSet<&[u8]> = a.iter().map(|&(o, l)| &original[o..o + l]).collect();
        let b_chunks: HashSet<&[u8]> = b.iter().map(|&(o, l)| &edited[o..o + l]).collect();
        assert!(a_chunks.difference(&b_chunks).count() <= 3);
    }

    #[test]
    fn min_equal_avg_equal_max_gives_fixed_size_chunks() {
        let data = random_bytes(3, 10_000);
        let chunks = chunk_boundaries_with(&data, 1000, 1000, 1000);
        assert_covers(&data, &chunks);
        assert!(chunks.iter().all(|&(_, l)| l == 1000));
    }

    #[test]
    #[should_panic(expected = "chunk sizes must satisfy")]
    fn invalid_sizes_panic() {
        chunk_boundaries_with(b"abc", 4096, 1024, 8192);
    }

    #[test]
    fn gear_table_is_frozen() {
        // Guards the on-disk chunking format: these values must never change.
        assert_eq!(GEAR.len(), 256);
        let distinct: HashSet<u64> = GEAR.iter().copied().collect();
        assert_eq!(distinct.len(), 256);
        assert_eq!(
            [GEAR[0], GEAR[1], GEAR[255]],
            [
                11_528_016_949_132_445_775,
                3_666_635_998_152_463_967,
                6_271_896_609_402_433_185
            ]
        );
        let data = random_bytes(2024, 100_000);
        let lens: Vec<usize> = chunk_boundaries_with(&data, 1024, 4096, 16384)
            .iter()
            .take(4)
            .map(|c| c.1)
            .collect();
        assert_eq!(lens, [1280, 2832, 3324, 3602]);
        assert_eq!(high_bits(0), 0);
        assert_eq!(high_bits(1), 1 << 63);
        assert_eq!(high_bits(64), u64::MAX);
    }
}
