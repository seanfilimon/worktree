//! Blame (ENGINEERING_PLAN §14.4): alice and bob edit one file in several
//! snapshots (including concurrent edits joined by a merge on pull); `wt
//! blame` attributes every line to the snapshot and author that last changed
//! it (never to the merge snapshot), honours `--rev`, `-L` and
//! `--porcelain`, and the server's `GET {tree}/blame/{rev}/{path}` agrees
//! line for line.

use serde_json::Value;
use worktree_e2e::json::{self, find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, TREE};

/// (line, text, snapshot id, author) of every blame line in `v`.
fn rows(v: &Value) -> Vec<(u64, String, String, String)> {
    find_objects(v, &["line", "text", "snapshot_id"])
        .into_iter()
        .map(|l| {
            (
                l["line"].as_u64().unwrap_or_default(),
                str_field(l, "text"),
                str_field(l, "snapshot_id"),
                str_field(l, "author"),
            )
        })
        .collect()
}

fn row(line: u64, text: &str, id: &str, author: &str) -> (u64, String, String, String) {
    (line, text.to_string(), id.to_string(), author.to_string())
}

#[test]
fn blame() {
    require_e2e!();
    let env = TestEnv::with_server("blame");
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[("file.txt", "one\ntwo\nthree\nfour\nfive\n")],
    );
    let s1 = a.head_id();
    let alice_api = alice.api();
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);

    // bob changes line 2 and pushes.
    let s2 = b.commit(
        &[("file.txt", "one\nTWO by bob\nthree\nfour\nfive\n")],
        "bob: line two",
    );
    b.push();
    // alice concurrently changes line 5, pulls (merge snapshot) and pushes.
    let s3 = a.commit(
        &[("file.txt", "one\ntwo\nthree\nfour\nFIVE by alice\n")],
        "alice: line five",
    );
    a.pull();
    let merge = a.head();
    assert_eq!(
        json::parents(&merge).len(),
        2,
        "pull of a diverged branch merges:\n{}",
        pretty(&merge)
    );
    let m = str_field(&merge, "id");
    assert_eq!(
        a.read("file.txt"),
        "one\nTWO by bob\nthree\nfour\nFIVE by alice\n"
    );
    a.push();
    // alice appends a line.
    let s4 = a.commit(
        &[(
            "file.txt",
            "one\nTWO by bob\nthree\nfour\nFIVE by alice\nsix\n",
        )],
        "alice: line six",
    );
    a.push();

    // --- local blame
    let local = a.wt().args(["blame", "file.txt"]).json();
    let expected = vec![
        row(1, "one", &s1, "Alice"),
        row(2, "TWO by bob", &s2, "Bob"),
        row(3, "three", &s1, "Alice"),
        row(4, "four", &s1, "Alice"),
        row(5, "FIVE by alice", &s3, "Alice"),
        row(6, "six", &s4, "Alice"),
    ];
    let got = rows(&local);
    // Authors are recorded as "Name <email>" or just the name: compare the
    // name part.
    let normalize = |r: &[(u64, String, String, String)]| -> Vec<(u64, String, String, String)> {
        r.iter()
            .map(|(l, t, id, au)| {
                (
                    *l,
                    t.clone(),
                    id.clone(),
                    au.split('<').next().unwrap_or_default().trim().to_string(),
                )
            })
            .collect()
    };
    assert_eq!(
        normalize(&got),
        expected,
        "local blame:\n{}",
        pretty(&local)
    );
    assert!(
        got.iter().all(|r| r.2 != m),
        "no line is attributed to the merge snapshot"
    );
    let line2 = find_objects(&local, &["line", "text", "snapshot_id"])[1];
    assert_eq!(str_field(line2, "message"), "bob: line two");
    assert_eq!(
        str_field(&local["snapshot"], "id"),
        s4,
        "{}",
        pretty(&local)
    );

    // --rev: the file as of the merge (5 lines)
    let at_merge = a.wt().args(["blame", "file.txt", "--rev", &m]).json();
    assert_eq!(
        normalize(&rows(&at_merge)),
        expected[..5].to_vec(),
        "{}",
        pretty(&at_merge)
    );
    // -L
    let range = a.wt().args(["blame", "file.txt", "-L", "2,+2"]).json();
    assert_eq!(
        normalize(&rows(&range)),
        expected[1..3].to_vec(),
        "{}",
        pretty(&range)
    );
    // --porcelain and the default text output
    let porcelain = a.wt().args(["blame", "file.txt", "--porcelain"]).ok();
    assert!(porcelain.stdout.contains(&format!("{s2} 2")), "{porcelain}");
    assert!(porcelain.stdout.contains("author Bob"), "{porcelain}");
    assert!(porcelain.stdout.contains("\tTWO by bob"), "{porcelain}");
    let text = a.wt().args(["blame", "file.txt"]).ok();
    assert!(
        text.lines()
            .iter()
            .any(|l| l.starts_with(&s2[..8]) && l.contains("Bob") && l.ends_with("TWO by bob")),
        "{text}"
    );

    // --- the server agrees, for the branch and for a snapshot id
    let tree = format!("/api/v1/trees/alice/{TREE}");
    let remote = alice_api
        .get(&format!("{tree}/blame/main/file.txt"))
        .ok_json();
    assert_eq!(
        normalize(&rows(&remote)),
        normalize(&got),
        "server blame:\n{}\nlocal blame:\n{}",
        pretty(&remote),
        pretty(&local)
    );
    assert_eq!(str_field(&remote["snapshot"], "id"), s4);
    let remote_at_merge = bob
        .api()
        .get(&format!("{tree}/blame/{m}/file.txt"))
        .ok_json();
    assert_eq!(
        normalize(&rows(&remote_at_merge)),
        normalize(&rows(&at_merge))
    );
    let remote_range = alice_api
        .get(&format!("{tree}/blame/main/file.txt?start=2&end=3"))
        .ok_json();
    assert_eq!(normalize(&rows(&remote_range)), expected[1..3].to_vec());
    // bob's local blame (after pulling) matches too
    b.pull();
    let bob_local = b.wt().args(["blame", "file.txt"]).json();
    assert_eq!(normalize(&rows(&bob_local)), expected);

    // --- errors: missing file, binary file, outsider
    alice_api
        .get(&format!("{tree}/blame/main/nope.txt"))
        .assert_status(404);
    a.commit(&[("bin.dat", "\u{0}\u{1}\u{2}binary\u{0}")], "binary");
    a.push();
    a.wt().args(["blame", "bin.dat"]).fails();
    alice_api
        .get(&format!("{tree}/blame/main/bin.dat"))
        .assert_error(422, "binary_file");
    let carol = env.user("carol");
    carol.register();
    let status = carol
        .api()
        .get(&format!("{tree}/blame/main/file.txt"))
        .status;
    assert!(
        matches!(status, 403 | 404),
        "an outsider cannot blame a private tree (got {status})"
    );
}
