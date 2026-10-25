//! Storage compression and integrity (ENGINEERING_PLAN §14.6): after a push
//! the local `.wt/objects` and the server's object store hold
//! `WTZ1`-compressed files for compressible content (small objects stay
//! raw); `wt fsck --full` is clean, a flipped byte in a blob is caught by
//! `wt fsck --full` and one in a manifest by plain `wt fsck`; `wt gc
//! --dry-run` reports a dropped stash's objects without touching them and
//! `wt gc --prune 0` deletes them; `wt admin fsck` is clean and reports a
//! corrupted server object.

use std::path::{Path, PathBuf};

use serde_json::Value;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{fsutil, require_e2e, Repo, TestEnv};

use crate::common::publish_tree;

const MAGIC: &[u8] = b"WTZ1";

/// Object files below `root`: `.../objects/<2 hex>/<rest>` -> (path, hash).
fn object_files(root: &Path) -> Vec<(PathBuf, String)> {
    let mut out = Vec::new();
    walk(root, &mut out);
    out.sort();
    out
}

fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
            continue;
        }
        let fan = p
            .parent()
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str());
        let top = p
            .parent()
            .and_then(Path::parent)
            .and_then(|d| d.file_name())
            .and_then(|n| n.to_str());
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        if let (Some(fan), Some("objects")) = (fan, top) {
            let hash = format!("{fan}{name}");
            if fan.len() == 2 && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()) {
                out.push((p.clone(), hash));
            }
        }
    }
}

fn is_compressed(p: &Path) -> bool {
    fsutil::read(p).starts_with(MAGIC)
}

/// A highly compressible text of about 40 KiB.
fn compressible() -> String {
    (0..800)
        .map(|i| format!("line {i:04}: the quick brown fox jumps over the lazy dog\n"))
        .collect()
}

#[track_caller]
fn fsck(repo: &Repo, full: bool) -> (bool, Value) {
    let mut cmd = repo.wt().args(["--json", "fsck"]);
    if full {
        cmd = cmd.arg("--full");
    }
    let out = cmd.run();
    (out.success(), out.json())
}

/// Flip the last byte of `path`; returns the original contents.
fn corrupt(path: &Path) -> Vec<u8> {
    let original = fsutil::read(path);
    let mut bad = original.clone();
    let last = bad.len() - 1;
    bad[last] ^= 0x5a;
    fsutil::write(path, bad);
    original
}

fn num(v: &Value, key: &str) -> u64 {
    find_objects(v, &[key])
        .first()
        .and_then(|o| o[key].as_u64())
        .unwrap_or_else(|| panic!("number `{key}` in\n{}", pretty(v)))
}

#[test]
fn storage() {
    require_e2e!();
    let env = TestEnv::with_server("storage");
    let alice = env.user("alice");
    let big = compressible();
    let a = publish_tree(
        &env,
        &alice,
        &[("big.txt", big.as_str()), ("small.txt", "tiny\n")],
    );

    // --- local store: the big blob is compressed, objects stay readable
    let local = object_files(&a.dir.join(".wt").join("objects"));
    assert!(!local.is_empty(), "no local objects found");
    let compressed: Vec<_> = local.iter().filter(|(p, _)| is_compressed(p)).collect();
    assert!(
        !compressed.is_empty(),
        "expected WTZ1 files in .wt/objects: {local:?}"
    );
    let largest = local
        .iter()
        .map(|(p, _)| std::fs::metadata(p).unwrap().len())
        .max()
        .unwrap();
    assert!(
        (largest as usize) < big.len() / 4,
        "the {} byte text is stored compressed (largest object file {largest} bytes)",
        big.len()
    );
    assert!(
        local.iter().any(|(p, _)| fsutil::read(p) == b"tiny\n"),
        "small objects are stored raw"
    );

    // --- server store: compressed too
    let server = object_files(&env.server().data_dir);
    assert!(!server.is_empty(), "no server objects found");
    let server_compressed: Vec<_> = server.iter().filter(|(p, _)| is_compressed(p)).collect();
    assert!(
        !server_compressed.is_empty(),
        "expected WTZ1 files in the server's object store: {server:?}"
    );
    // The same object ids on both sides (hashes are over the content).
    for (_, h) in &compressed {
        assert!(
            server.iter().any(|(_, s)| s == h),
            "server has local object {h}"
        );
    }
    // A fresh clone reads compressed objects back.
    let bob = env.user("bob");
    bob.register();
    alice
        .wt()
        .args(["tenant", "add-member", "alice", "bob", "--role", "viewer"])
        .ok();
    let b = bob.clone_tree("alice", "proj", "proj");
    assert_eq!(b.read("big.txt"), big);

    // --- fsck --full is clean
    let (ok, report) = fsck(&a, true);
    assert!(ok, "fsck --full on a fresh store:\n{}", pretty(&report));
    assert!(num(&report, "objects_checked") >= local.len() as u64);
    assert!(num(&report, "snapshots_checked") >= 1);
    a.wt().arg("fsck").ok();

    // --- a corrupted blob is caught by --full
    let (blob_path, blob_hash) = compressed[0].clone();
    let original = corrupt(&blob_path);
    let (ok, report) = fsck(&a, true);
    assert!(!ok, "fsck --full must fail:\n{}", pretty(&report));
    assert!(
        report.to_string().contains(&blob_hash),
        "the corrupt object is named:\n{}",
        pretty(&report)
    );
    let out = a.wt().args(["fsck", "--full"]).code(1);
    assert!(out.mentions("corrupt object"), "{out}");
    fsutil::write(&blob_path, &original);
    assert!(fsck(&a, true).0, "clean again after restoring the file");

    // --- a corrupted manifest is caught by plain fsck
    let manifest = str_field(&a.head(), "manifest");
    let (manifest_path, _) = local
        .iter()
        .find(|(_, h)| *h == manifest)
        .cloned()
        .unwrap_or_else(|| panic!("manifest {manifest} not in {local:?}"));
    let original = corrupt(&manifest_path);
    let (ok, report) = fsck(&a, false);
    assert!(!ok, "plain fsck reads manifests:\n{}", pretty(&report));
    assert!(
        report.to_string().contains(&manifest),
        "{}",
        pretty(&report)
    );
    fsutil::write(&manifest_path, &original);
    assert!(fsck(&a, false).0);

    // --- gc: a dropped stash leaves unreachable objects
    let junk: String = (0..300)
        .map(|i| format!("stashed junk {i} {}\n", worktree_e2e::random_hex(8)))
        .collect();
    a.write("junk.txt", &junk);
    a.wt()
        .args(["stash", "push", "--include-untracked", "-m", "junk"])
        .ok();
    let with_stash = object_files(&a.dir.join(".wt").join("objects")).len();
    assert!(with_stash > local.len(), "the stash stored new objects");
    // While the stash exists its objects are GC roots.
    let kept = a.wt().args(["gc", "--prune", "0"]).json();
    assert_eq!(num(&kept, "objects_deleted"), 0, "{}", pretty(&kept));
    a.wt().args(["stash", "drop"]).ok();
    let dry = a.wt().args(["gc", "--dry-run", "--prune", "0"]).json();
    assert_eq!(dry["dry_run"], Value::Bool(true), "{}", pretty(&dry));
    assert!(num(&dry, "objects_deleted") >= 2, "{}", pretty(&dry));
    assert!(num(&dry, "bytes_deleted") > 0, "{}", pretty(&dry));
    assert_eq!(
        object_files(&a.dir.join(".wt").join("objects")).len(),
        with_stash,
        "--dry-run deletes nothing"
    );
    let text = a.wt().args(["gc", "--dry-run", "--prune", "0"]).ok();
    assert!(text.mentions("would"), "{text}");
    let real = a.wt().args(["gc", "--prune", "0"]).json();
    assert_eq!(real["dry_run"], Value::Bool(false));
    assert_eq!(
        num(&real, "objects_deleted"),
        num(&dry, "objects_deleted"),
        "{}",
        pretty(&real)
    );
    let after = object_files(&a.dir.join(".wt").join("objects")).len();
    assert_eq!(
        after as u64,
        with_stash as u64 - num(&real, "objects_deleted")
    );
    let again = a.wt().args(["gc", "--prune", "0"]).json();
    assert_eq!(num(&again, "objects_deleted"), 0, "{}", pretty(&again));
    // Default gc (30 day prune window) keeps everything reachable.
    a.wt().arg("gc").ok();
    assert!(fsck(&a, true).0, "fsck --full after gc");
    assert_eq!(a.read("big.txt"), big);
    a.assert_clean();
    a.commit(&[("after-gc.txt", "still works\n")], "after gc");
    a.push();

    // --- server fsck
    let admin = env.admin_user();
    admin.login();
    alice.wt().args(["admin", "fsck"]).fails(); // server admins only
    let report = admin.wt().args(["admin", "fsck"]).json();
    assert!(
        report["objects_checked"].as_u64().unwrap_or(0) >= server.len() as u64,
        "{}",
        pretty(&report)
    );
    assert!(
        report["corrupt"].as_array().is_none_or(|c| c.is_empty()),
        "{}",
        pretty(&report)
    );
    assert!(
        report["missing"].as_array().is_none_or(|m| m.is_empty()),
        "{}",
        pretty(&report)
    );
    let (spath, shash) = server_compressed[0].clone();
    let original = corrupt(&spath);
    let out = admin.wt().args(["--json", "admin", "fsck"]).run();
    assert_eq!(out.code, Some(1), "admin fsck reports corruption\n{out}");
    assert!(out.stdout.contains(&shash), "{out}");
    fsutil::write(&spath, &original);
    admin.wt().args(["admin", "fsck"]).ok();
}
