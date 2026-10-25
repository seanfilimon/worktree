//! Sparse checkout and partial clones (ENGINEERING_PLAN §15.4) against the
//! real server: `wt clone --sparse` downloads only part of the objects
//! (checked through local object counts), `wt sparse add` hydrates on
//! demand (chunked file included), diff and merge of paths outside the set
//! download lazily, a snapshot + push from the sparse clone keeps every
//! other path intact on the server, gc/fsck accept the missing content, and
//! an unreachable server gives a clear error.

use std::path::Path;

use serde_json::Value;
use worktree_e2e::json::{self, pretty};
use worktree_e2e::{fsutil, require_e2e, TestEnv};

use crate::common::{add_member, publish_tree, server_tip, TREE};

/// Comfortably above the 8 MiB chunking threshold.
const BIG_SIZE: usize = 9 * 1024 * 1024 + 4321;

fn big_content(size: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(size + 64);
    let mut n: u64 = 0;
    while out.len() < size {
        out.extend_from_slice(format!("{n:010} sparse partial clone payload line\n").as_bytes());
        n = n
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407)
            >> 1;
    }
    out.truncate(size);
    out
}

/// Number of object files in a worktree's local store.
fn object_count(root: &Path) -> usize {
    let objects = root.join(".wt").join("objects");
    let mut n = 0;
    for dir in std::fs::read_dir(&objects)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", objects.display()))
        .flatten()
    {
        let name = dir.file_name();
        if name.len() != 2 {
            continue;
        }
        if let Ok(files) = std::fs::read_dir(dir.path()) {
            n += files
                .flatten()
                .filter(|f| f.file_name().len() == 62)
                .count();
        }
    }
    n
}

#[test]
fn sparse_checkout() {
    require_e2e!();
    let env = TestEnv::with_server("sparse_checkout");
    let alice = env.user("alice");
    let a = publish_tree(&env, &alice, &[("README.md", "# sparse\n")]);
    let big = big_content(BIG_SIZE);

    // --- alice: a tree with several areas, two snapshots, a feature branch
    a.write("src/main.rs", "fn main() {}\n");
    a.write("docs/guide.md", "line 1\nline 2\nline 3\nline 4\n");
    a.write("docs/notes.md", "notes v1\n");
    a.write("docs/untouched.md", "untouched v1\n");
    a.write("assets/big.dat", &big);
    let first = a.commit(&[], "add areas");
    let second = a.commit(
        &[
            ("docs/notes.md", "notes v2\n"),
            ("docs/untouched.md", "untouched v2\n"),
        ],
        "notes and untouched v2",
    );
    a.push();
    a.wt().args(["switch", "-c", "feature"]).ok();
    a.commit(
        &[(
            "docs/guide.md",
            "line 1\nline 2\nline 3\nline 4 (feature)\n",
        )],
        "feature: guide",
    );
    a.push();
    a.wt().args(["switch", "main"]).ok();
    let main_tip = a.commit(
        &[("docs/guide.md", "line 1 (main)\nline 2\nline 3\nline 4\n")],
        "main: guide",
    );
    a.push();
    let alice_api = alice.api();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(main_tip.clone())
    );
    let full_count = object_count(&a.dir);

    // --- bob: sparse clone of src/ only
    let bob = env.user("bob");
    bob.register();
    add_member(&alice_api, "alice", "bob", "developer");
    let b_dir = bob.dir.join("sparse");
    let clone = bob
        .wt()
        .args(["clone", &env.url(), &format!("alice/{TREE}")])
        .arg(&b_dir)
        .args(["--sparse", "src"])
        .ok();
    assert!(clone.mentions("sparse checkout"), "{clone}");
    let b = bob.repo(&b_dir, TREE);
    b.set_identity();
    let files: Vec<String> = b.files().into_keys().collect();
    assert_eq!(
        files,
        vec!["src/main.rs".to_string()],
        "only src/ is checked out"
    );
    b.assert_clean();
    let sparse_count = object_count(&b.dir);
    assert!(
        sparse_count * 2 < full_count,
        "a sparse clone downloads part of the objects: {sparse_count} of {full_count}"
    );

    let status = b.status_json();
    let sparse = &status["sparse"];
    assert_eq!(
        sparse["patterns"],
        serde_json::json!(["/src"]),
        "status reports the sparse state:\n{}",
        pretty(&status)
    );
    assert_eq!(sparse["partial_remote"], "origin", "{}", pretty(&status));
    assert_eq!(sparse["files_checked_out"], 1, "{}", pretty(&status));
    let text = b.wt().arg("status").ok();
    assert!(text.mentions("sparse checkout (/src)"), "{text}");
    assert!(text.mentions("1 of 6 files checked out"), "{text}");

    // gc and fsck accept the content a partial clone has not downloaded.
    let fsck = b.wt().arg("fsck").json();
    assert!(
        json::find_objects(&fsck, &["errors"])
            .iter()
            .all(|o| o["errors"].as_array().is_some_and(Vec::is_empty)),
        "fsck reports no errors in a partial clone:\n{}",
        pretty(&fsck)
    );
    b.wt().args(["gc", "--prune", "0"]).ok();
    assert_eq!(
        object_count(&b.dir),
        sparse_count,
        "gc neither deletes nor downloads"
    );

    // --- diff outside the set downloads lazily
    let diff = b
        .wt()
        .args(["diff", &first, &second, "--", "docs/notes.md"])
        .ok();
    assert!(
        diff.stdout.contains("-notes v1") && diff.stdout.contains("+notes v2"),
        "lazy diff of a path outside the sparse set\n{diff}"
    );
    let after_diff = object_count(&b.dir);
    assert!(
        after_diff > sparse_count,
        "the diff downloaded the two blobs"
    );

    // --- sparse add hydrates (chunk list + chunks)
    let add = b.wt().args(["sparse", "add", "assets"]).json();
    assert_eq!(add["written"], 1, "{}", pretty(&add));
    assert!(
        add["fetched"].as_u64().unwrap_or(0) > 2,
        "chunk list and chunks are downloaded:\n{}",
        pretty(&add)
    );
    assert_eq!(
        fsutil::read(&b.path("assets/big.dat")),
        big,
        "hydrated big.dat"
    );
    b.assert_clean();
    let list = b.wt().args(["sparse", "list"]).json();
    assert_eq!(
        list["patterns"],
        serde_json::json!(["/src", "/assets"]),
        "{}",
        pretty(&list)
    );

    // --- merge touching only paths outside the set fetches lazily
    let merge = b.wt().args(["merge", "feature"]).ok();
    assert!(!merge.mentions("conflict"), "{merge}");
    assert!(
        !b.exists("docs/guide.md"),
        "merged files outside the set stay unwritten"
    );
    b.assert_clean();

    // --- snapshot + push from the sparse clone
    b.commit(
        &[("src/main.rs", "fn main() { println!(\"sparse\"); }\n")],
        "edit from the sparse clone",
    );
    let bob_tip = b.head_id();
    b.push();
    assert_eq!(
        server_tip(&alice_api, "alice", TREE, "main"),
        Some(bob_tip.clone())
    );

    // The server tree is intact: alice pulls everything.
    a.pull();
    assert_eq!(a.head_id(), bob_tip);
    assert_eq!(
        a.read("docs/guide.md"),
        "line 1 (main)\nline 2\nline 3\nline 4 (feature)\n"
    );
    assert_eq!(a.read("docs/notes.md"), "notes v2\n");
    assert_eq!(a.read("docs/untouched.md"), "untouched v2\n");
    assert_eq!(a.read("README.md"), "# sparse\n");
    assert_eq!(fsutil::read(&a.path("assets/big.dat")), big);
    a.assert_clean();
    let blob = alice_api.get(&format!(
        "/api/v1/trees/alice/{TREE}/blob/{bob_tip}/docs/untouched.md"
    ));
    blob.assert_ok();
    assert_eq!(blob.text(), "untouched v2\n");

    // --- offline: a clear error, local work still fine
    b.wt().args(["remote", "remove", "origin"]).ok();
    b.add_remote("origin", "http://127.0.0.1:1", "alice");
    let offline = b
        .wt()
        .args(["diff", &first, &second, "--", "docs/untouched.md"])
        .fails();
    assert!(offline.mentions("partial clone"), "{offline}");
    assert!(offline.mentions("downloaded on demand"), "{offline}");
    b.assert_clean();
    b.wt().args(["remote", "remove", "origin"]).ok();
    b.add_remote("origin", &env.url(), "alice");

    // --- disable: everything is checked out (downloaded on demand)
    let disable = b.wt().args(["sparse", "disable"]).json();
    assert_eq!(
        disable["enabled"],
        Value::Bool(false),
        "{}",
        pretty(&disable)
    );
    assert_eq!(b.read("docs/untouched.md"), "untouched v2\n");
    assert_eq!(b.read("README.md"), "# sparse\n");
    b.assert_clean();
    fsutil::assert_same_files(&a.dir, &b.dir);
    let status = b.status_json();
    assert!(status.get("sparse").is_none(), "{}", pretty(&status));
}
