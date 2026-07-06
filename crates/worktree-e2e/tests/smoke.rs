//! Smoke test: the basic local VCS loop works through the real CLI binary.

mod common;

use common::wt_ok;

#[test]
fn init_snapshot_log_branch_merge_roundtrip() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    // init
    let out = wt_ok(root, &["init", "."]);
    assert!(out.contains("initialized"), "unexpected init output: {out}");

    // first snapshot
    std::fs::write(root.join("a.txt"), "hello\n").unwrap();
    let out = wt_ok(root, &["snapshot", "-m", "first"]);
    assert!(out.contains("Snapshot created"), "{out}");

    // log shows it
    let out = wt_ok(root, &["log"]);
    assert!(out.contains("first"), "{out}");

    // status is clean right after a snapshot
    let out = wt_ok(root, &["status"]);
    assert!(out.contains("clean"), "{out}");

    // branch, change, snapshot, merge back
    wt_ok(root, &["branch", "create", "feature"]);
    wt_ok(root, &["branch", "switch", "feature"]);
    std::fs::write(root.join("b.txt"), "feature work\n").unwrap();
    wt_ok(root, &["snapshot", "-m", "feature work"]);
    wt_ok(root, &["branch", "switch", "main"]);
    let out = wt_ok(root, &["merge", "feature"]);
    assert!(out.contains("Merged branch 'feature'"), "{out}");

    // merged file list includes both files
    let out = wt_ok(root, &["log", "-n", "1"]);
    assert!(out.contains("Merge branch 'feature'"), "{out}");

    // tags work
    wt_ok(root, &["tag", "create", "v0.1.0", "-m", "first tag"]);
    let out = wt_ok(root, &["tag", "list"]);
    assert!(out.contains("v0.1.0"), "{out}");
}

#[test]
fn init_refuses_double_init() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();

    wt_ok(root, &["init", "."]);
    let out = common::wt(root, &["init", "."]);
    assert!(
        !out.status.success(),
        "second init should fail: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
