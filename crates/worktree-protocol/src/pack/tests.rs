use std::collections::HashMap;
use std::path::PathBuf;

use serde_json::{json, Value};

use super::*;

// ---- helpers ----

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.next() >> 24) as u8).collect()
    }
}

fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn from_hex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
        .collect()
}

/// Build a pack from `objects` = (kind, data, base data) where the base
/// is looked up among `haves` and earlier objects by content.
/// (kind, data, base data).
type TestObject = (u8, Vec<u8>, Option<Vec<u8>>);

fn build(objects: &[TestObject], opts: WriterOptions) -> (Vec<u8>, PackStats) {
    let mut w = PackWriter::new(Vec::new(), opts).unwrap();
    for (kind, data, base) in objects {
        let bh = base.as_ref().map(|b| hash(b));
        let base = bh.as_ref().zip(base.as_deref());
        w.add(hash(data), *kind, data, base).unwrap();
    }
    w.finish().unwrap()
}

/// Read every object of `pack`, resolving bases from `haves` and earlier
/// objects.
fn read_all(pack: &[u8], haves: &[Vec<u8>]) -> Result<Vec<PackObject>, PackError> {
    let mut store: HashMap<Hash, Vec<u8>> = haves.iter().map(|h| (hash(h), h.clone())).collect();
    let mut r = PackReader::new(pack)?;
    let mut out = Vec::new();
    while let Some(obj) = r.next_object(|h| store.get(h).cloned())? {
        if let Ok(d) = &obj.data {
            store.insert(obj.hash, d.clone());
        }
        out.push(obj);
    }
    Ok(out)
}

fn edit(base: &[u8]) -> Vec<u8> {
    let mut t = base.to_vec();
    t[base.len() / 2..base.len() / 2 + 10].copy_from_slice(b"0123456789");
    t.splice(base.len() / 4..base.len() / 4, b"inserted!".iter().copied());
    t.drain(base.len() * 3 / 4..base.len() * 3 / 4 + 40);
    t
}

// ---- varints ----

#[test]
fn uvarint_roundtrip_and_limits() {
    for v in [0u64, 1, 127, 128, 300, 16_384, u32::MAX as u64, u64::MAX] {
        let mut b = Vec::new();
        put_uvarint(&mut b, v);
        assert_eq!(uvarint(&b), Some((v, b.len())), "{v}");
    }
    assert_eq!(uvarint(&[]), None);
    assert_eq!(uvarint(&[0x80]), None);
    // 11 bytes / overflow of the 10th byte.
    assert_eq!(uvarint(&[0xff; 11]), None);
    let mut over = vec![0xff; 9];
    over.push(0x02);
    assert_eq!(uvarint(&over), None);
    // Non-minimal encodings decode (like Go's binary.Uvarint).
    assert_eq!(uvarint(&[0x80, 0x00]), Some((0, 2)));
}

// ---- deltas ----

#[test]
fn delta_roundtrips() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    let base = rng.bytes(8192);
    let cases: Vec<Vec<u8>> = vec![
        Vec::new(),
        b"tiny".to_vec(),
        base.clone(),
        edit(&base),
        [b"head".as_slice(), &base, b"tail"].concat(),
        base[4096..].to_vec(),
        rng.bytes(5000),
        vec![b'a'; 3000],
    ];
    for target in cases {
        let d = delta::encode(&base, &target);
        assert_eq!(delta::target_len(&d), Some(target.len() as u64));
        let out = delta::apply(&base, &d, MAX_OBJECT_SIZE).unwrap();
        assert_eq!(out, target);
    }
    // A small edit compresses to a tiny delta.
    let d = delta::encode(&base, &edit(&base));
    assert!(d.len() < 100, "edit delta is {} bytes", d.len());
    assert!(delta_pays_off(d.len(), base.len()));
    // Unrelated content does not pay off.
    let other = rng.bytes(8192);
    assert!(!delta_pays_off(
        delta::encode(&base, &other).len(),
        other.len()
    ));
}

#[test]
fn delta_apply_rejects_malformed_input() {
    let base = b"0123456789abcdef0123456789abcdef".to_vec();
    let good = delta::encode(&base, b"0123456789abcdef!!");
    assert!(delta::apply(&base, &good, 1 << 20).is_ok());
    // Wrong base.
    assert!(delta::apply(b"short", &good, 1 << 20).is_err());
    // Too large for the caller.
    assert!(delta::apply(&base, &good, 4).is_err());
    let mk = |ops: &[u8], target: u64| {
        let mut d = Vec::new();
        put_uvarint(&mut d, base.len() as u64);
        put_uvarint(&mut d, target);
        d.extend_from_slice(ops);
        d
    };
    // Copy out of range, zero-length copy/insert, truncated insert, unknown op,
    // size mismatch.
    for (ops, target) in [
        (vec![1, 30, 5], 5),
        (vec![1, 0, 0], 0),
        (vec![0, 0], 0),
        (vec![0, 5, b'a'], 5),
        (vec![7], 1),
        (vec![0, 1, b'a'], 2),
        (vec![1, 0, 4, 1, 0, 4], 4),
    ] {
        assert!(
            delta::apply(&base, &mk(&ops, target), 1 << 20).is_err(),
            "{ops:?}"
        );
    }
}

#[test]
fn fingerprint_rolls() {
    let data: Vec<u8> = (0..64u8).collect();
    let w = (1..delta::BLOCK).fold(1u32, |w, _| w.wrapping_mul(delta::PRIME));
    let mut h = delta::fingerprint(&data[..16]);
    for p in 0..data.len() - 16 {
        h = h
            .wrapping_sub(u32::from(data[p]).wrapping_mul(w))
            .wrapping_mul(delta::PRIME)
            .wrapping_add(u32::from(data[p + 16]));
        assert_eq!(h, delta::fingerprint(&data[p + 1..p + 17]));
    }
}

// ---- packs ----

#[test]
fn pack_roundtrip_with_deltas_and_compression() {
    let mut rng = Rng(42);
    let have = rng.bytes(6000);
    let v1 = edit(&have);
    let v2 = edit(&v1);
    let text = "line\n".repeat(400).into_bytes();
    let objects = vec![
        (1, b"hello".to_vec(), None),
        (1, v1.clone(), Some(have.clone())),
        (1, v2.clone(), Some(v1.clone())),
        (2, text.clone(), None),
        (1, rng.bytes(100), Some(have.clone())), // unrelated: whole
        (1, v1.clone(), Some(have.clone())),     // duplicate: skipped
    ];
    for opts in [
        WriterOptions::default(),
        WriterOptions {
            compress: false,
            ..WriterOptions::default()
        },
    ] {
        let (pack, stats) = build(&objects, opts);
        assert_eq!(stats.entries, 5);
        assert_eq!(stats.deltas, 2);
        assert_eq!(stats.pack_bytes, pack.len() as u64);
        assert!(stats.bytes_saved() > 10_000, "{stats:?}");
        let got = read_all(&pack, std::slice::from_ref(&have)).unwrap();
        assert_eq!(got.len(), 5);
        assert_eq!(got[1].base, Some(hash(&have)));
        assert_eq!(got[2].base, Some(hash(&v1)));
        assert_eq!(got[2].data.as_ref().unwrap(), &v2);
        assert_eq!(got[3].kind, 2);
        assert!(got.iter().all(|o| o.data.is_ok()));
    }
    // Deltas off: every entry whole.
    let (pack, stats) = build(
        &objects,
        WriterOptions {
            deltas: false,
            ..WriterOptions::default()
        },
    );
    assert_eq!(stats.deltas, 0);
    assert!(read_all(&pack, &[])
        .unwrap()
        .iter()
        .all(|o| o.base.is_none()));
}

#[test]
fn delta_depth_is_bounded() {
    let mut rng = Rng(7);
    let mut versions = vec![rng.bytes(4000)];
    for _ in 0..12 {
        let next = edit(versions.last().unwrap());
        versions.push(next);
    }
    let objects: Vec<_> = versions
        .windows(2)
        .map(|w| (1u8, w[1].clone(), Some(w[0].clone())))
        .collect();
    let (pack, _) = build(&objects, WriterOptions::default());
    let got = read_all(&pack, &versions[..1]).unwrap();
    let whole: Vec<usize> = got
        .iter()
        .enumerate()
        .filter(|(_, o)| o.base.is_none())
        .map(|(i, _)| i)
        .collect();
    // The have is depth 0: entries 0..8 are deltas (depth 1..8), entry 8
    // restarts the chain.
    assert_eq!(whole, vec![8]);
    assert!(got.iter().all(|o| o.data.is_ok()));

    // A reader refuses chains deeper than the limit.
    // The writer never produces such a chain, so craft one by hand.
    let mut chain = Vec::new();
    chain.extend_from_slice(&MAGIC);
    chain.extend_from_slice(&[VERSION, 0, 0, 0]);
    let mut prev = versions[0].clone();
    for v in versions.iter().skip(1).take(9) {
        let d = delta::encode(&prev, v);
        chain.extend_from_slice(&[TYPE_DELTA, 1, CODEC_RAW]);
        chain.extend_from_slice(&hash(v));
        chain.extend_from_slice(&hash(&prev));
        put_uvarint(&mut chain, v.len() as u64);
        put_uvarint(&mut chain, d.len() as u64);
        put_uvarint(&mut chain, d.len() as u64);
        chain.extend_from_slice(&d);
        prev = v.clone();
    }
    chain.push(TYPE_END);
    let digest = hash(&chain);
    chain.extend_from_slice(&digest);
    let err = read_all(&chain, &versions[..1]).unwrap_err();
    assert!(err.to_string().contains("deeper"), "{err}");
}

#[test]
fn reader_reports_truncation_checksum_and_framing_errors() {
    let objects = vec![(1u8, b"some object content".to_vec(), None)];
    let (pack, _) = build(&objects, WriterOptions::default());
    // Every strict prefix is truncated.
    for cut in [0, 4, 8, 9, 20, pack.len() - 33, pack.len() - 1] {
        let err = read_all(&pack[..cut], &[]).unwrap_err();
        assert!(matches!(err, PackError::Truncated), "cut {cut}: {err}");
    }
    let mut bad = pack.clone();
    *bad.last_mut().unwrap() ^= 1;
    assert!(matches!(read_all(&bad, &[]), Err(PackError::Checksum)));
    let mut bad = pack.clone();
    bad[0] = b'X';
    assert!(matches!(read_all(&bad, &[]), Err(PackError::Corrupt(_))));
    let mut bad = pack.clone();
    bad[4] = 9;
    assert!(read_all(&bad, &[])
        .unwrap_err()
        .to_string()
        .contains("version"));
    let mut bad = pack.clone();
    bad[8] = 7; // entry type
    assert!(matches!(read_all(&bad, &[]), Err(PackError::Corrupt(_))));
    let mut extra = pack.clone();
    extra.push(0);
    assert!(read_all(&extra, &[])
        .unwrap_err()
        .to_string()
        .contains("after"));
    // A flipped content byte keeps the framing but fails the trailer.
    let mut flipped = pack.clone();
    flipped[pack.len() - 34] ^= 0xff;
    assert!(read_all(&flipped, &[]).is_err());
}

#[test]
fn bad_entries_are_reported_per_object() {
    let base = Rng(3).bytes(3000);
    let target = edit(&base);
    let (pack, _) = build(
        &[
            (1, target.clone(), Some(base.clone())),
            (1, b"fine".to_vec(), None),
        ],
        WriterOptions::default(),
    );
    // Without the base the delta entry is unusable, the next one is fine.
    let got = read_all(&pack, &[]).unwrap();
    assert!(got[0].data.as_ref().unwrap_err().contains("not available"));
    assert_eq!(got[1].data.as_ref().unwrap(), b"fine");

    // A whole entry whose hash lies.
    let mut lie = Vec::new();
    lie.extend_from_slice(&MAGIC);
    lie.extend_from_slice(&[VERSION, 0, 0, 0, TYPE_WHOLE, 1, CODEC_RAW]);
    lie.extend_from_slice(&hash(b"other"));
    lie.extend_from_slice(&[4, 4, 4]);
    lie.extend_from_slice(b"data");
    lie.push(TYPE_END);
    let d = hash(&lie);
    lie.extend_from_slice(&d);
    let got = read_all(&lie, &[]).unwrap();
    assert!(got[0].data.as_ref().unwrap_err().contains("hash"));
}

// ---- golden vectors shared with the Go server ----

fn vectors_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/pack/vectors.json")
}

fn delta_inputs() -> Vec<(&'static str, Vec<u8>, Vec<u8>)> {
    let mut rng = Rng(0x005e_ed0f_7ac4);
    let random = rng.bytes(2048);
    let text: Vec<u8> = (0..120)
        .map(|i| format!("line {i}: the quick brown fox jumps over the lazy dog\n"))
        .collect::<String>()
        .into_bytes();
    let mut text2 = text.clone();
    let at = text2.len() / 3;
    text2.splice(at..at, b"line inserted by an edit\n".iter().copied());
    text2.truncate(text2.len() - 200);
    let blocks: Vec<&[u8]> = random.chunks(256).collect();
    let shuffled: Vec<u8> = [7, 3, 0, 5, 1, 6, 2, 4]
        .iter()
        .flat_map(|&i| blocks[i].iter().copied())
        .collect();
    vec![
        ("empty-target", random[..64].to_vec(), Vec::new()),
        ("short", b"0123456789".to_vec(), b"9876543210".to_vec()),
        ("identical", random.clone(), random.clone()),
        ("edit", random.clone(), edit(&random)),
        (
            "prepend-append",
            random.clone(),
            [b"HEADER".as_slice(), &random, b"FOOTER"].concat(),
        ),
        ("text-lines", text.clone(), text2),
        (
            "repetitive",
            vec![b'a'; 1000],
            [vec![b'a'; 1500], b"b".to_vec()].concat(),
        ),
        ("unrelated", random.clone(), rng.bytes(1024)),
        ("shuffled-blocks", random.clone(), shuffled),
    ]
}

/// Pack vectors: (name, haves, objects as (kind, data, base index)) where
/// a base index < haves.len() names a have and larger ones an earlier
/// object (index - haves.len()).
#[allow(clippy::type_complexity)]
fn pack_inputs() -> Vec<(
    &'static str,
    Vec<Vec<u8>>,
    Vec<(u8, Vec<u8>, Option<usize>)>,
)> {
    let mut rng = Rng(0xfeed_beef);
    let have = rng.bytes(1500);
    let v1 = edit(&have);
    let v2 = edit(&v1);
    let manifest = "entry\n".repeat(50).into_bytes();
    let mut versions = vec![rng.bytes(700)];
    for _ in 0..10 {
        let next = edit(versions.last().unwrap());
        versions.push(next);
    }
    vec![
        ("empty", Vec::new(), Vec::new()),
        (
            "mixed",
            vec![have.clone()],
            vec![
                (1, b"hello pack".to_vec(), None),
                (1, v1.clone(), Some(0)),
                (1, v2, Some(2)),
                (2, manifest, None),
                (3, rng.bytes(96), Some(0)),
                (1, v1, Some(0)),
            ],
        ),
        (
            "depth-limit",
            vec![versions[0].clone()],
            versions[1..]
                .iter()
                .enumerate()
                .map(|(i, v)| (1u8, v.clone(), Some(i)))
                .collect(),
        ),
    ]
}

fn build_vector(
    haves: &[Vec<u8>],
    objects: &[(u8, Vec<u8>, Option<usize>)],
    opts: WriterOptions,
) -> Vec<u8> {
    let resolved: Vec<_> = objects
        .iter()
        .map(|(k, d, b)| {
            let base = b.map(|i| {
                if i < haves.len() {
                    haves[i].clone()
                } else {
                    objects[i - haves.len()].1.clone()
                }
            });
            (*k, d.clone(), base)
        })
        .collect();
    build(&resolved, opts).0
}

const RAW: WriterOptions = WriterOptions {
    compress: false,
    level: 3,
    deltas: true,
};

/// Regenerate with `WT_UPDATE_GOLDEN=1 cargo test -p worktree-protocol
/// pack::tests::golden`; the Go side adds `zstd_pack_go` with
/// `WT_UPDATE_GOLDEN=1 go test ./internal/syncsvc -run PackGolden`.
#[test]
fn golden_vectors() {
    let path = vectors_path();
    let existing: Value = std::fs::read(&path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null);
    let update = std::env::var("WT_UPDATE_GOLDEN").is_ok_and(|v| v == "1");

    let deltas: Vec<Value> = delta_inputs()
        .into_iter()
        .map(|(name, base, target)| {
            json!({
                "name": name,
                "base": to_hex(&base),
                "target": to_hex(&target),
                "delta": to_hex(&delta::encode(&base, &target)),
            })
        })
        .collect();
    let packs: Vec<Value> = pack_inputs()
        .into_iter()
        .map(|(name, haves, objects)| {
            let old = existing["packs"]
                .as_array()
                .and_then(|a| a.iter().find(|p| p["name"] == name))
                .cloned()
                .unwrap_or(Value::Null);
            json!({
                "name": name,
                "haves": haves.iter().map(|h| to_hex(h)).collect::<Vec<_>>(),
                "objects": objects.iter().map(|(k, d, b)| json!({
                    "kind": k, "data": to_hex(d), "base": b,
                })).collect::<Vec<_>>(),
                "raw_pack": to_hex(&build_vector(&haves, &objects, RAW)),
                "zstd_pack_rust": if update || old["zstd_pack_rust"].is_null() {
                    Value::String(to_hex(&build_vector(&haves, &objects, WriterOptions::default())))
                } else {
                    old["zstd_pack_rust"].clone()
                },
                "zstd_pack_go": old["zstd_pack_go"].clone(),
            })
        })
        .collect();
    let generated = json!({
        "comment": "Golden vectors of the W0rkTree pack format (specs/sync/Packs.md). Shared by crates/worktree-protocol/src/pack/tests.rs and services/server-go/internal/syncsvc/pack_format_test.go.",
        "deltas": deltas,
        "packs": packs,
    });
    if update {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut text = serde_json::to_string_pretty(&generated).unwrap();
        text.push('\n');
        std::fs::write(&path, text).unwrap();
        return;
    }
    assert!(
        existing.is_object(),
        "missing {}; run with WT_UPDATE_GOLDEN=1",
        path.display()
    );
    // Byte-for-byte: delta encoder and raw packs.
    for (g, e) in generated["deltas"]
        .as_array()
        .unwrap()
        .iter()
        .zip(existing["deltas"].as_array().unwrap())
    {
        assert_eq!(g, e, "delta vector {}", g["name"]);
    }
    for (g, e) in generated["packs"]
        .as_array()
        .unwrap()
        .iter()
        .zip(existing["packs"].as_array().unwrap())
    {
        assert_eq!(g["raw_pack"], e["raw_pack"], "raw pack {}", g["name"]);
        assert_eq!(g["objects"], e["objects"], "pack inputs {}", g["name"]);
    }
    // Decoding: every stored pack (raw, Rust zstd, Go zstd) yields the
    // objects.
    for p in existing["packs"].as_array().unwrap() {
        let haves: Vec<Vec<u8>> = p["haves"]
            .as_array()
            .unwrap()
            .iter()
            .map(|h| from_hex(h.as_str().unwrap()))
            .collect();
        let mut want: Vec<Vec<u8>> = Vec::new();
        for o in p["objects"].as_array().unwrap() {
            let d = from_hex(o["data"].as_str().unwrap());
            if !want.contains(&d) {
                want.push(d);
            }
        }
        for field in ["raw_pack", "zstd_pack_rust", "zstd_pack_go"] {
            let Some(hex) = p[field].as_str() else {
                assert_ne!(field, "raw_pack");
                continue;
            };
            let got: Vec<Vec<u8>> = read_all(&from_hex(hex), &haves)
                .unwrap_or_else(|e| panic!("{} {field}: {e}", p["name"]))
                .into_iter()
                .map(|o| o.data.unwrap())
                .collect();
            assert_eq!(got, want, "{} {field}", p["name"]);
        }
    }
}
