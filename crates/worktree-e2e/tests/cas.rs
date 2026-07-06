//! WT-PHASE-2 acceptance: content-addressable storage through the real CLI.

mod common;

use common::{wt, wt_ok};
use std::path::{Path, PathBuf};

/// Locate this worktree's store under the shared test storage base.
///
/// The store dir name is the first 16 hex chars of BLAKE3 over the
/// canonicalized root path with forward slashes — same derivation as
/// `worktree-store::paths::worktree_hash`.
fn store_dir(root: &Path) -> PathBuf {
    let canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let normalized = canonical.to_string_lossy().replace('\\', "/");
    let hash = blake3::hash(normalized.as_bytes()).to_hex()[..16].to_string();
    let dir = common::storage_dir().join("stores").join(hash);
    assert!(dir.is_dir(), "store dir missing at {dir:?}");
    dir
}

fn count_files(dir: &Path) -> usize {
    if !dir.exists() {
        return 0;
    }
    walk(dir)
}

fn walk(dir: &Path) -> usize {
    let mut n = 0;
    for entry in std::fs::read_dir(dir).unwrap().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            n += walk(&path);
        } else {
            n += 1;
        }
    }
    n
}

#[test]
fn no_state_json_and_identical_content_dedups() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    wt_ok(root, &["init", "."]);
    // Two files, identical content.
    std::fs::write(root.join("one.txt"), "duplicate bytes\n").unwrap();
    std::fs::write(root.join("two.txt"), "duplicate bytes\n").unwrap();
    wt_ok(root, &["snapshot", "-m", "dedup test"]);

    // The legacy JSON state file must not exist.
    assert!(
        !root.join(".wt").join("state.json").exists(),
        "state.json must not be written by the CAS backend"
    );

    // Exactly one blob object for the duplicated content.
    let store = store_dir(root);
    assert_eq!(count_files(&store.join("objects").join("blobs")), 1);
    assert_eq!(count_files(&store.join("objects").join("snapshots")), 1);
    assert_eq!(count_files(&store.join("objects").join("manifests")), 1);
}

#[test]
fn doctor_detects_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    wt_ok(root, &["init", "."]);
    std::fs::write(root.join("data.txt"), "verify me\n").unwrap();
    wt_ok(root, &["snapshot", "-m", "healthy"]);

    let out = wt_ok(root, &["doctor"]);
    assert!(out.contains("no corruption"), "{out}");

    // Corrupt the single blob.
    let blobs = store_dir(root).join("objects").join("blobs");
    let blob_path = first_file(&blobs);
    let mut bytes = std::fs::read(&blob_path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    std::fs::write(&blob_path, bytes).unwrap();

    let out = wt(root, &["doctor"]);
    assert!(!out.status.success(), "doctor must fail on corruption");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("corrupt"), "{stdout}");
}

fn first_file(dir: &Path) -> PathBuf {
    let entry = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .next()
        .unwrap_or_else(|| panic!("no files under {dir:?}"));
    let path = entry.path();
    if path.is_dir() {
        first_file(&path)
    } else {
        path
    }
}

#[test]
fn reflog_records_cli_operations() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    wt_ok(root, &["init", "."]);
    std::fs::write(root.join("a.txt"), "x\n").unwrap();
    wt_ok(root, &["snapshot", "-m", "first"]);
    wt_ok(root, &["branch", "create", "feature"]);

    let out = wt_ok(root, &["reflog"]);
    assert!(out.contains("snapshot"), "{out}");

    // On-disk reflog files per DotWt.md.
    let reflog = root.join(".wt").join("reflog");
    assert!(reflog.join("main.log").exists());
    assert!(reflog.join("feature.log").exists());
    assert!(reflog.join("_global.log").exists());
    let global = std::fs::read_to_string(reflog.join("_global.log")).unwrap();
    assert!(global.contains("branch:create"), "{global}");
}

#[test]
fn legacy_state_json_migrates_on_first_read() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();

    // Fabricate a legacy (pre-CAS) worktree: .wt/ + state.json, no store.
    let wt_dir = root.join(".wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    std::fs::write(
        wt_dir.join("config.toml"),
        "[worktree]\nname = \"legacy\"\n",
    )
    .unwrap();
    let legacy_state = serde_json::json!({
        "name": "legacy",
        "created_at": "2026-01-01T00:00:00+00:00",
        "current_tree": "root",
        "trees": [{
            "name": "root",
            "path": ".",
            "current_branch": "main",
            "branches": [
                { "name": "main", "tip": "old-uuid-1", "created_at": "2026-01-01T00:00:00+00:00" }
            ],
            "snapshots": [
                {
                    "id": "old-uuid-0",
                    "message": "legacy first",
                    "author": "old-author",
                    "timestamp": "2026-01-01T01:00:00+00:00",
                    "parents": [],
                    "tree_name": "root",
                    "branch_name": "main",
                    "files": [ { "path": "a.txt", "hash": "deadbeef", "size": 4 } ],
                    "auto_generated": false
                },
                {
                    "id": "old-uuid-1",
                    "message": "legacy second",
                    "author": "old-author",
                    "timestamp": "2026-01-01T02:00:00+00:00",
                    "parents": ["old-uuid-0"],
                    "tree_name": "root",
                    "branch_name": "main",
                    "files": [ { "path": "a.txt", "hash": "cafebabe", "size": 5 } ],
                    "auto_generated": false
                }
            ],
            "tags": [
                { "name": "v-legacy", "target_snapshot": "old-uuid-1", "message": null,
                  "tagger": "old-author", "created_at": "2026-01-01T03:00:00+00:00" }
            ]
        }]
    });
    std::fs::write(
        wt_dir.join("state.json"),
        serde_json::to_string_pretty(&legacy_state).unwrap(),
    )
    .unwrap();

    // First read migrates: history visible, ids remapped to content hashes.
    let out = wt_ok(root, &["log"]);
    assert!(out.contains("legacy first"), "{out}");
    assert!(out.contains("legacy second"), "{out}");
    assert!(!out.contains("old-uuid"), "ids must be remapped: {out}");

    // Legacy file renamed, not deleted.
    assert!(!wt_dir.join("state.json").exists());
    assert!(wt_dir.join("state.json.migrated").exists());

    // Tags survive with remapped targets.
    let out = wt_ok(root, &["tag", "list"]);
    assert!(out.contains("v-legacy"), "{out}");
}
