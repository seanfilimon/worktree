//! pack-delta (ENGINEERING_PLAN §16.1): push, clone and fetch over packs
//! with deltas (observed through the server's `worktree_pack_*`
//! counters), an interrupted pack upload resumed by the client, `wt push
//! --no-pack` / `WT_NO_PACK=1`, and the fallback to whole objects against a
//! server started with `--disable-packs`.

use std::collections::BTreeMap;

use worktree_e2e::fsutil::assert_same_files;
use worktree_e2e::{require_e2e, Api, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

/// Every `worktree_pack_*` sample of GET /metrics (`name{labels}` -> value).
fn pack_counters(env: &TestEnv) -> BTreeMap<String, u64> {
    let text = Api::new(&env.url()).get("/metrics").assert_ok().text();
    assert!(
        text.contains("# TYPE worktree_pack_bytes_total counter"),
        "pack counters are rendered:\n{text}"
    );
    text.lines()
        .filter(|l| l.starts_with("worktree_pack_"))
        .filter_map(|l| {
            let (k, v) = l.rsplit_once(' ')?;
            Some((k.to_string(), v.trim().parse().ok()?))
        })
        .collect()
}

fn get(m: &BTreeMap<String, u64>, key: &str) -> u64 {
    m.get(key).copied().unwrap_or(0)
}

fn grew(before: &BTreeMap<String, u64>, after: &BTreeMap<String, u64>, key: &str) -> u64 {
    get(after, key) - get(before, key)
}

const UP_BYTES: &str = "worktree_pack_bytes_total{direction=\"upload\"}";
const DOWN_BYTES: &str = "worktree_pack_bytes_total{direction=\"download\"}";
const UP_DELTAS: &str = "worktree_pack_deltas_total{direction=\"upload\"}";
const DOWN_DELTAS: &str = "worktree_pack_deltas_total{direction=\"download\"}";
const UP_SAVED: &str = "worktree_pack_bytes_saved_total{direction=\"upload\"}";
const DOWN_SAVED: &str = "worktree_pack_bytes_saved_total{direction=\"download\"}";
const RESUMES: &str = "worktree_pack_upload_resumes_total";

/// A ~40 KB text file unique to `seed`.
fn text(seed: usize) -> String {
    (0..800)
        .map(|i| {
            format!(
                "{seed:03}-{i:05}: lorem ipsum dolor sit amet {}\n",
                (i * 7919 + seed) % 1000
            )
        })
        .collect()
}

/// `n` bytes of incompressible content.
fn noise(seed: u64, n: usize) -> Vec<u8> {
    let mut s = seed.wrapping_mul(2).wrapping_add(1);
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            (s >> 24) as u8
        })
        .collect()
}

fn small_edit(s: &str) -> String {
    s.replacen("lorem ipsum", "LOREM IPSUM (edited)", 2)
}

#[test]
fn packs() {
    require_e2e!();
    let env = TestEnv::with_server("packs");
    let alice = env.user("alice");

    // --- initial push: one pack upload
    let contents: Vec<(String, String)> = (0..10)
        .map(|i| (format!("src/file{i}.txt"), text(i)))
        .collect();
    let files: Vec<(&str, &str)> = contents
        .iter()
        .map(|(p, c)| (p.as_str(), c.as_str()))
        .collect();
    let m0 = pack_counters(&env);
    let a = publish_tree(&env, &alice, &files);
    let m1 = pack_counters(&env);
    assert!(
        grew(&m0, &m1, UP_BYTES) > 0,
        "initial push used a pack: {m1:?}"
    );
    let alice_api = alice.api();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(a.head_id())
    );

    // --- a small edit travels as a delta
    a.commit(
        &[("src/file3.txt", &small_edit(&contents[3].1))],
        "edit file3",
    );
    a.push();
    let m2 = pack_counters(&env);
    assert!(grew(&m1, &m2, UP_DELTAS) >= 1, "delta upload: {m2:?}");
    assert!(
        grew(&m1, &m2, UP_SAVED) > 30_000,
        "the delta saves most of the 40 KB file: {m2:?}"
    );
    assert!(grew(&m1, &m2, UP_BYTES) < 10_000, "{m2:?}");

    // --- bob clones over a pack and later fetches a delta
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_same_files(&a.dir, &b.dir);
    let m3 = pack_counters(&env);
    assert!(grew(&m2, &m3, DOWN_BYTES) > 0, "clone used a pack: {m3:?}");

    a.commit(
        &[("src/file5.txt", &small_edit(&contents[5].1))],
        "edit file5",
    );
    a.push();
    let m4 = pack_counters(&env);
    b.pull();
    let m5 = pack_counters(&env);
    assert_eq!(b.head_id(), a.head_id());
    assert_eq!(b.read("src/file5.txt"), small_edit(&contents[5].1));
    assert!(grew(&m4, &m5, DOWN_DELTAS) >= 1, "delta download: {m5:?}");
    assert!(grew(&m4, &m5, DOWN_SAVED) > 30_000, "{m5:?}");

    // --- an interrupted upload is resumed from what the server kept
    a.write("assets/blob.bin", noise(42, 1_200_000));
    a.snapshot("add a large binary");
    let out = a
        .wt()
        .arg("push")
        .env("WT_PACK_TEST_INTERRUPT_AFTER", "262144")
        .ok();
    let m6 = pack_counters(&env);
    assert_eq!(grew(&m5, &m6, RESUMES), 1, "resumed once: {m6:?}\n{out}");
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(a.head_id())
    );
    b.pull();
    assert_eq!(
        std::fs::read(b.path("assets/blob.bin")).unwrap(),
        noise(42, 1_200_000)
    );

    // --- `wt push --no-pack` and WT_NO_PACK=1 transfer whole objects
    let m7 = pack_counters(&env);
    a.commit(
        &[("src/file7.txt", &small_edit(&contents[7].1))],
        "edit file7",
    );
    a.wt().args(["push", "--no-pack"]).ok();
    b.wt().arg("pull").env("WT_NO_PACK", "1").ok();
    let m8 = pack_counters(&env);
    assert_eq!(grew(&m7, &m8, UP_BYTES), 0, "--no-pack: {m8:?}");
    assert_eq!(grew(&m7, &m8, DOWN_BYTES), 0, "WT_NO_PACK: {m8:?}");
    assert_eq!(b.read("src/file7.txt"), small_edit(&contents[7].1));
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(a.head_id())
    );
}

#[test]
fn packs_fallback_when_the_server_disables_them() {
    require_e2e!();
    let env = TestEnv::with_server_args("packs_fallback", &["--disable-packs"]);
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("a.txt", &text(1)), ("b.txt", &text(2))]);
    a.commit(&[("a.txt", &small_edit(&text(1)))], "edit");
    a.push();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice.api(), "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_same_files(&a.dir, &b.dir);
    a.commit(&[("b.txt", &small_edit(&text(2)))], "edit b");
    a.push();
    b.pull();
    assert_eq!(b.read("b.txt"), small_edit(&text(2)));
    let m = pack_counters(&env);
    assert!(
        m.values().all(|v| *v == 0),
        "no pack transfers on a server with --disable-packs: {m:?}"
    );
}
