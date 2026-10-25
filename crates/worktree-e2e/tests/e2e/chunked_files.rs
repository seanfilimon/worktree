//! Chunked large files and shallow sync (ENGINEERING_PLAN §13.2): a file
//! larger than 8 MiB is stored as content-defined chunks, survives push →
//! clone → edit → push → pull byte for byte (also through the REST blob
//! endpoint), and `wt clone --depth 1` + `wt fetch --deepen 1` grow a
//! shallow history one snapshot at a time.

use worktree_e2e::fsutil;
use worktree_e2e::json::{self, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

/// Size of the generated file: comfortably above the 8 MiB chunking
/// threshold (several FastCDC chunks).
const BIG_SIZE: usize = 9 * 1024 * 1024 + 12_345;

/// Deterministic, mildly repetitive text of `size` bytes: numbered lines
/// with a rotating vocabulary, so chunk boundaries are content-defined
/// rather than one degenerate run.
fn big_content(size: usize) -> Vec<u8> {
    const WORDS: &[&str] = &[
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel", "india",
        "juliet", "kilo", "lima", "mike", "november", "oscar", "papa",
    ];
    let mut out = Vec::with_capacity(size + 128);
    let mut n: u64 = 0;
    while out.len() < size {
        let a = WORDS[(n % 16) as usize];
        let b = WORDS[((n / 16) % 16) as usize];
        let c = WORDS[((n * 7 + 3) % 16) as usize];
        out.extend_from_slice(format!("{n:08} {a} {b} {c} the quick brown fox\n").as_bytes());
        n += 1;
    }
    out.truncate(size);
    out
}

#[track_caller]
fn assert_same_bytes(what: &str, got: &[u8], want: &[u8]) {
    if got != want {
        let first = got
            .iter()
            .zip(want)
            .position(|(a, b)| a != b)
            .unwrap_or(got.len().min(want.len()));
        panic!(
            "{what}: content differs (got {} bytes, want {} bytes, first difference at byte {first})",
            got.len(),
            want.len()
        );
    }
}

/// Size of the largest file in a worktree's local object store
/// (`.wt/objects/<hh>/<rest>`, stored raw).
fn largest_object(root: &std::path::Path) -> u64 {
    let mut max = 0;
    let objects = root.join(".wt").join("objects");
    for dir in std::fs::read_dir(&objects)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", objects.display()))
        .flatten()
    {
        if let Ok(files) = std::fs::read_dir(dir.path()) {
            for f in files.flatten() {
                if let Ok(md) = f.metadata() {
                    max = max.max(md.len());
                }
            }
        }
    }
    max
}

/// FastCDC maximum chunk size (§13.2).
const MAX_CHUNK: u64 = 4 * 1024 * 1024;

#[test]
fn chunked_files() {
    require_e2e!();
    let env = TestEnv::with_server("chunked_files");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# big files\n")]);
    let alice_api = alice.api();

    // --- a >8 MiB file: snapshot, push
    let big = big_content(BIG_SIZE);
    a.write("assets/big.dat", &big);
    a.snapshot("add big.dat");
    let big_tip = a.head_id();
    a.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(big_tip.clone()),
        "server main after pushing the chunked file"
    );

    // The REST blob endpoint reassembles the chunks.
    let blob = alice_api.get(&format!(
        "/api/v1/trees/alice/{TREE}/blob/{big_tip}/assets/big.dat"
    ));
    blob.assert_ok();
    assert_same_bytes("REST blob of big.dat", &blob.body, &big);

    // --- bob clones elsewhere: identical bytes
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_eq!(b.head_id(), big_tip);
    assert_same_bytes(
        "bob's clone of big.dat",
        &fsutil::read(&b.path("assets/big.dat")),
        &big,
    );
    b.assert_clean();
    // Stored as chunks, not as one 9 MiB blob, on both sides.
    for (who, dir) in [("alice", &a.dir), ("bob", &b.dir)] {
        let largest = largest_object(dir);
        assert!(
            largest <= MAX_CHUNK,
            "{who}'s object store holds a {largest}-byte object: big.dat was not chunked"
        );
    }

    // --- bob edits a small part in the middle, pushes; alice pulls
    let mut edited = big.clone();
    let mid = BIG_SIZE / 2;
    let patch = b"<<< bob edited this small region in the middle of the file >>>\n";
    edited[mid..mid + patch.len()].copy_from_slice(patch);
    b.write("assets/big.dat", &edited);
    assert_eq!(
        b.changes(),
        vec![("assets/big.dat".to_string(), "modified".to_string())],
        "the edit is detected"
    );
    b.snapshot("bob: edit the middle of big.dat");
    let edit_tip = b.head_id();
    b.push();
    a.pull();
    assert_eq!(a.head_id(), edit_tip, "alice fast-forwards to bob's edit");
    assert_same_bytes(
        "alice's pulled big.dat",
        &fsutil::read(&a.path("assets/big.dat")),
        &edited,
    );
    a.assert_clean();
    let show = a.wt().args(["show", &edit_tip, "--stat"]).ok();
    assert!(
        show.stdout.contains("big.dat"),
        "the edit snapshot touches big.dat\n{show}"
    );
    let blob = alice_api.get(&format!(
        "/api/v1/trees/alice/{TREE}/blob/{edit_tip}/assets/big.dat"
    ));
    blob.assert_ok();
    assert_same_bytes("REST blob of the edited big.dat", &blob.body, &edited);

    // A small fourth snapshot on top.
    let last = a.commit(&[("notes.txt", "small change\n")], "notes");
    a.push();
    let full: Vec<String> = json::ids(&a.log(&[]));
    assert_eq!(full.len(), 4, "full history: {full:?}");
    assert_eq!(full[0], last);

    // --- shallow clone: only the newest snapshot
    let shallow_dir = alice.dir.join("shallow");
    alice
        .wt()
        .args([
            "clone",
            "--depth",
            "1",
            &env.url(),
            &format!("alice/{TREE}"),
        ])
        .arg(&shallow_dir)
        .ok();
    let s = alice.repo(&shallow_dir, TREE);
    s.set_identity();
    let log = s.log(&[]);
    assert_eq!(
        json::ids(&log),
        vec![last.clone()],
        "a --depth 1 clone has exactly one snapshot:\n{}",
        json::pretty(&serde_json::Value::Array(log.clone()))
    );
    let status = s.status_json();
    assert_eq!(
        json::find_bool(&status, "shallow"),
        Some(true),
        "status reports a shallow history:\n{}",
        json::pretty(&status)
    );
    let text = s.wt().arg("log").ok();
    assert!(
        text.mentions("shallow boundary"),
        "`wt log` marks the shallow boundary\n{text}"
    );
    // The working copy is complete (the big file is checked out).
    assert_same_bytes(
        "shallow clone's big.dat",
        &fsutil::read(&s.path("assets/big.dat")),
        &edited,
    );
    s.assert_clean();

    // --- deepen by one snapshot
    s.wt().args(["fetch", "--deepen", "1"]).ok();
    let log = s.log(&[]);
    assert_eq!(
        json::ids(&log),
        vec![last.clone(), edit_tip.clone()],
        "--deepen 1 adds exactly one older snapshot"
    );
    assert_eq!(
        str_field(&log[1], "message"),
        "bob: edit the middle of big.dat"
    );
    // Deepen to the full history: the boundary disappears.
    s.wt().args(["fetch", "--deepen", "10"]).ok();
    assert_eq!(json::ids(&s.log(&[])), full, "deepened to the full history");
    let status = s.status_json();
    assert_eq!(
        json::find_bool(&status, "shallow"),
        Some(false),
        "no longer shallow once the root is reached:\n{}",
        json::pretty(&status)
    );
}
