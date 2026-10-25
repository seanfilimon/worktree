//! Local `gc` and `fsck` (ENGINEERING_PLAN §14.6).

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use common::Fixture;
use worktree_sdk::repo::storage::{FsckIssueKind, FsckOptions, GcOptions, GcReport};
use worktree_sdk::store::hash_bytes;
use worktree_sdk::{
    Manifest, ManifestEntry, MergeState, ObjectHash, ReflogEntry, Repository, Snapshot,
    SwitchOptions,
};

fn meta_dir(fx: &Fixture) -> PathBuf {
    fx.repo.wt_dir().join("trees").join(&fx.tree)
}

/// Every object reachable from `snap` (manifest + entry objects).
fn objects_of(repo: &Repository, snap: &Snapshot) -> Vec<ObjectHash> {
    let manifest = repo.manifest_of(snap).unwrap();
    let mut out = vec![snap.manifest];
    for e in &manifest.entries {
        out.extend(repo.store().entry_objects(e).unwrap());
    }
    out
}

/// Every file of `snap` reads back intact.
fn assert_readable(repo: &Repository, snap: &Snapshot) {
    let manifest = repo.manifest_of(snap).unwrap();
    for e in &manifest.entries {
        let data = repo.store().read_entry(e).unwrap();
        assert_eq!(data.len() as u64, e.size, "{}", e.path);
    }
}

/// Move `main` back to `to`, leaving the current tip reachable only from
/// the reflog (and whatever extra root a test adds).
fn reset_main(fx: &Fixture, to: &Snapshot) {
    fx.repo
        .set_branch_tip(&fx.tree, "main", Some(&to.id))
        .unwrap();
}

fn gc(fx: &Fixture, dry_run: bool, prune_days: u32) -> GcReport {
    fx.repo
        .gc(&GcOptions {
            dry_run,
            prune_days,
        })
        .unwrap()
}

struct History {
    a: Snapshot,
    b: Snapshot,
    c: Snapshot,
    d: Snapshot,
}

/// A (tag t1) <- B (main) <- C (feature) ; B <- D (reflog only).
fn history(fx: &Fixture) -> History {
    fx.write("a.txt", "v1\n");
    let a = fx.snap("A");
    fx.write("a.txt", "v2\n");
    fx.write("big.txt", &"compressible content line\n".repeat(200));
    let b = fx.snap("B");
    fx.repo
        .switch_branch(
            &fx.tree,
            "feature",
            SwitchOptions {
                create: true,
                force: false,
            },
        )
        .unwrap();
    fx.write("c.txt", "feature only\n");
    let c = fx.snap("C");
    fx.repo
        .switch_branch(&fx.tree, "main", SwitchOptions::default())
        .unwrap();
    fx.repo
        .create_tag(&fx.tree, "t1", Some(&a.id), None)
        .unwrap();
    fx.write("d.txt", "doomed unique content\n");
    let d = fx.snap("D");
    reset_main(fx, &b);
    History { a, b, c, d }
}

#[test]
fn gc_never_deletes_reachable_objects_and_dry_run_changes_nothing() {
    let fx = Fixture::new();
    let h = history(&fx);
    let store = fx.repo.store();
    let garbage = store.put(b"loose garbage nobody references").unwrap();
    let d_blob = hash_bytes(b"doomed unique content\n");
    let (count_before, bytes_before) = store.stats().unwrap();

    // Default window: D is protected by its reflog entry, the loose object
    // by the in-flight grace period.
    let r = gc(&fx, true, 30);
    assert!(r.dry_run);
    assert_eq!(r.objects_deleted, 0, "{r:?}");
    assert_eq!(r.snapshots_deleted, 0);
    assert!(r.objects_recent >= 1);
    let r = gc(&fx, false, 30);
    assert_eq!((r.objects_deleted, r.snapshots_deleted), (0, 0), "{r:?}");
    assert!(store.has(&garbage) && store.has(&d_blob));

    // --prune 0: the reflog no longer protects D; dry run first.
    let dry = gc(&fx, true, 0);
    assert_eq!(dry.snapshots_deleted, 1, "{dry:?}");
    // D's manifest, d.txt's blob and the loose object.
    assert_eq!(dry.objects_deleted, 3, "{dry:?}");
    assert!(dry.bytes_deleted > 0);
    assert_eq!(store.stats().unwrap().0, count_before, "dry run deleted");
    assert!(fx.repo.has_snapshot(&h.d.id));

    let real = gc(&fx, false, 0);
    assert_eq!(real.objects_deleted, dry.objects_deleted);
    assert_eq!(real.bytes_deleted, dry.bytes_deleted);
    assert_eq!(real.snapshots_deleted, 1);
    assert_eq!(real.snapshots_reachable, 3);
    assert_eq!(real.bytes_before, bytes_before);
    assert!(!store.has(&garbage) && !store.has(&d_blob));
    assert!(!store.has(&h.d.manifest));
    assert!(!fx.repo.has_snapshot(&h.d.id));
    for snap in [&h.a, &h.b, &h.c] {
        assert!(fx.repo.has_snapshot(&snap.id));
        for o in objects_of(&fx.repo, snap) {
            assert!(store.has(&o), "reachable object {o} deleted");
        }
        assert_readable(&fx.repo, snap);
    }
    let report = fx.repo.fsck(&FsckOptions { full: true }).unwrap();
    assert!(report.is_ok(), "{:?}", report.errors);

    // Idempotent.
    let again = gc(&fx, false, 0);
    assert_eq!((again.objects_deleted, again.snapshots_deleted), (0, 0));
}

#[test]
fn gc_roots_remote_refs_merge_state_stash_and_state_files() {
    let fx = Fixture::new();
    fx.write("base.txt", "base\n");
    let base = fx.snap("base");
    let unique = |name: &str| {
        fx.write(&format!("{name}.txt"), &format!("only in {name}\n"));
        let s = fx.snap(name);
        reset_main(&fx, &base);
        fx.remove(&format!("{name}.txt"));
        s
    };
    let remote = unique("remote");
    let merge = unique("merge");
    let rebase = unique("rebase");
    let doomed = unique("doomed");

    // Remote-tracking ref.
    fx.repo
        .set_remote_tip(&fx.tree, "origin", "main", Some(&remote.id))
        .unwrap();
    // A pending merge.
    let state = MergeState {
        source_branch: "topic".into(),
        source_tip: merge.id.clone(),
        target_tip: Some(base.id.clone()),
        base: None,
        conflicts: vec![],
        message: "merge topic".into(),
        resolved: vec![],
    };
    fs::write(
        meta_dir(&fx).join("MERGE_STATE.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    // State of an in-progress rebase (format unknown to gc).
    fs::write(
        meta_dir(&fx).join("REBASE_STATE.json"),
        serde_json::to_vec(&serde_json::json!({"onto": rebase.id, "todo": [] })).unwrap(),
    )
    .unwrap();
    // A stash entry referencing a manifest that no snapshot uses.
    let store = fx.repo.store();
    let (stash_blob, chunked) = store.put_file(b"stashed work in progress\n").unwrap();
    let stash_manifest = fx
        .repo
        .write_manifest(&Manifest::from_entries(vec![ManifestEntry {
            path: "wip.txt".into(),
            hash: stash_blob,
            size: 25,
            executable: false,
            chunked,
        }]))
        .unwrap();
    let line = serde_json::json!({"message": "wip", "manifest": stash_manifest.to_hex()});
    fs::write(meta_dir(&fx).join("stash.jsonl"), format!("{line}\n")).unwrap();

    let r = gc(&fx, false, 0);
    assert_eq!(r.snapshots_deleted, 1, "{r:?}");
    assert!(!fx.repo.has_snapshot(&doomed.id));
    assert!(!store.has(&hash_bytes(b"only in doomed\n")));
    for (snap, file) in [
        (&remote, "only in remote\n"),
        (&merge, "only in merge\n"),
        (&rebase, "only in rebase\n"),
    ] {
        assert!(fx.repo.has_snapshot(&snap.id), "{}", snap.message);
        assert!(store.has(&hash_bytes(file.as_bytes())), "{file}");
        assert_readable(&fx.repo, snap);
    }
    assert!(store.has(&stash_manifest) && store.has(&stash_blob));
    assert_eq!(
        store.get(&stash_blob).unwrap(),
        b"stashed work in progress\n"
    );
}

#[test]
fn gc_keeps_reflog_entries_inside_the_window() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let a = fx.snap("a");
    fx.write("b.txt", "in the window\n");
    let b = fx.snap("b");
    reset_main(&fx, &a);
    // Rewrite b's reflog entries as 10 days old: inside a 30-day window,
    // outside a 5-day one.
    let reflog = meta_dir(&fx).join("reflog.jsonl");
    let old = chrono::Utc::now() - chrono::Duration::days(10);
    let mut lines = Vec::new();
    for line in fs::read_to_string(&reflog).unwrap().lines() {
        let mut e: ReflogEntry = serde_json::from_str(line).unwrap();
        e.at = old;
        lines.push(serde_json::to_string(&e).unwrap());
    }
    fs::write(&reflog, lines.join("\n") + "\n").unwrap();
    // Age the record past the in-flight grace too.
    let rec = fx
        .repo
        .wt_dir()
        .join("snapshots")
        .join(format!("{}.json", b.id));
    let past = std::time::SystemTime::now() - std::time::Duration::from_secs(7200);
    fs::File::options()
        .write(true)
        .open(&rec)
        .unwrap()
        .set_modified(past)
        .unwrap();
    for o in objects_of(&fx.repo, &b) {
        if let Ok(f) = fs::File::options()
            .write(true)
            .open(fx.repo.store().path_of(&o))
        {
            f.set_modified(past).unwrap();
        }
    }

    assert_eq!(gc(&fx, false, 30).snapshots_deleted, 0);
    assert!(fx.repo.has_snapshot(&b.id));
    let r = gc(&fx, false, 5);
    assert_eq!(r.snapshots_deleted, 1, "{r:?}");
    assert!(!fx.repo.has_snapshot(&b.id));
    assert!(!fx.repo.store().has(&hash_bytes(b"in the window\n")));
}

#[test]
fn gc_refuses_when_a_reachable_manifest_is_corrupt() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let a = fx.snap("a");
    let garbage = fx.repo.store().put(b"garbage").unwrap();
    fs::write(fx.repo.store().path_of(&a.manifest), b"tampered").unwrap();
    let err = fx
        .repo
        .gc(&GcOptions {
            dry_run: false,
            prune_days: 0,
        })
        .unwrap_err();
    assert!(err.to_string().contains("wt fsck"), "{err}");
    assert!(fx.repo.store().has(&garbage), "nothing may be deleted");
}

#[test]
fn gc_compacts_raw_objects_and_removes_stale_temp_files() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    let a = fx.snap("a");
    // An object written raw by a pre-compression client.
    let content = "legacy raw object line\n".repeat(100);
    let raw_store = worktree_sdk::ObjectStore::open(fx.repo.store().root()).with_compression(false);
    let blob = raw_store.put(content.as_bytes()).unwrap();
    let manifest = fx
        .repo
        .write_manifest(&Manifest::from_entries(vec![ManifestEntry {
            path: "legacy.txt".into(),
            hash: blob,
            size: content.len() as u64,
            executable: false,
            chunked: false,
        }]))
        .unwrap();
    let snap = Snapshot {
        id: "legacy-snapshot".into(),
        manifest,
        parents: vec![a.id.clone()],
        ..a.clone()
    };
    fx.repo.write_snapshot(&snap).unwrap();
    fx.repo
        .set_branch_tip(&fx.tree, "main", Some(&snap.id))
        .unwrap();
    assert!(!fx.repo.store().is_compressed(&blob).unwrap());
    let tmp = fx
        .repo
        .store()
        .path_of(&blob)
        .with_file_name(".wt-tmp-interrupted");
    fs::write(&tmp, b"partial write").unwrap();
    fs::File::options()
        .write(true)
        .open(&tmp)
        .unwrap()
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(7200))
        .unwrap();

    let dry = gc(&fx, true, 30);
    assert_eq!((dry.objects_compacted, dry.temp_files_deleted), (0, 1));
    assert!(tmp.exists());
    let r = gc(&fx, false, 30);
    assert!(r.objects_compacted >= 1, "{r:?}");
    assert!(r.bytes_compacted > 0);
    assert_eq!(r.temp_files_deleted, 1);
    assert!(r.bytes_after < r.bytes_before);
    assert!(!tmp.exists());
    assert!(fx.repo.store().is_compressed(&blob).unwrap());
    assert_eq!(fx.repo.store().get(&blob).unwrap(), content.as_bytes());
}

#[test]
fn fsck_detects_corruption_missing_objects_and_dangling_refs() {
    let fx = Fixture::new();
    let h = history(&fx);
    let clean = fx.repo.fsck(&FsckOptions::default()).unwrap();
    assert!(clean.is_ok(), "{:?}", clean.errors);
    assert_eq!(clean.snapshots_checked, 4);
    assert!(clean.manifests_checked >= 4 && clean.refs_checked >= 3);
    assert_eq!(
        clean.objects_checked, 0,
        "only --full re-hashes every object"
    );
    let full = fx.repo.fsck(&FsckOptions { full: true }).unwrap();
    assert!(full.is_ok() && full.objects_checked > 0);

    let store = fx.repo.store();
    // A tampered blob is found by --full only (the default pass only checks
    // presence of blobs).
    let v1 = hash_bytes(b"v1\n");
    fs::write(store.path_of(&v1), b"tampered").unwrap();
    assert!(fx.repo.fsck(&FsckOptions::default()).unwrap().is_ok());
    let r = fx.repo.fsck(&FsckOptions { full: true }).unwrap();
    let kinds: Vec<_> = r.errors.iter().map(|e| (e.kind, e.id.clone())).collect();
    assert_eq!(kinds, vec![(FsckIssueKind::CorruptObject, v1.to_hex())]);

    // A lost blob, a lost parent record (A) and therefore a dangling tag.
    let c_blob = hash_bytes(b"feature only\n");
    store.delete(&c_blob).unwrap();
    fs::remove_file(
        fx.repo
            .wt_dir()
            .join("snapshots")
            .join(format!("{}.json", h.a.id)),
    )
    .unwrap();
    // A snapshot whose manifest does not decode, and one whose manifest is gone.
    let junk = store.put(b"\xff\xff\xff not a manifest").unwrap();
    let bad = Snapshot {
        id: "bad-manifest".into(),
        manifest: junk,
        parents: vec![h.b.id.clone()],
        ..h.b.clone()
    };
    fx.repo.write_snapshot(&bad).unwrap();
    let gone = Snapshot {
        id: "gone-manifest".into(),
        manifest: hash_bytes(b"never stored"),
        parents: vec![],
        ..h.b.clone()
    };
    fx.repo.write_snapshot(&gone).unwrap();
    // An unreadable snapshot record.
    fs::write(
        fx.repo.wt_dir().join("snapshots").join("broken.json"),
        b"{not json",
    )
    .unwrap();

    let r = fx.repo.fsck(&FsckOptions { full: true }).unwrap();
    assert!(!r.is_ok());
    let got: HashSet<_> = r.errors.iter().map(|e| (e.kind, e.id.clone())).collect();
    let want: HashSet<_> = [
        (FsckIssueKind::CorruptObject, v1.to_hex()),
        (FsckIssueKind::MissingObject, c_blob.to_hex()),
        (FsckIssueKind::MissingParent, h.b.id.clone()),
        (FsckIssueKind::BadManifest, junk.to_hex()),
        (FsckIssueKind::MissingManifest, "gone-manifest".to_string()),
        (FsckIssueKind::BadSnapshot, "broken".to_string()),
        (FsckIssueKind::DanglingRef, format!("{}: tag t1", fx.tree)),
    ]
    .into_iter()
    .collect();
    assert_eq!(got, want, "{:#?}", r.errors);
    // The report serializes for `wt fsck --json`.
    let json = serde_json::to_value(&r).unwrap();
    assert!(json["errors"][0]["kind"].is_string());
}

#[test]
fn fsck_accepts_shallow_boundaries_and_restricted_paths() {
    let fx = Fixture::new();
    fx.write("a.txt", "a\n");
    fx.write("secret/plan.txt", "restricted\n");
    let a = fx.snap("a");
    // Pretend the history was fetched shallowly with secret/ withheld:
    // the parent record and the restricted blob are absent.
    let child = Snapshot {
        id: "shallow-child".into(),
        parents: vec!["not-fetched".into()],
        ..a.clone()
    };
    fx.repo.write_snapshot(&child).unwrap();
    fs::write(meta_dir(&fx).join("shallow"), "shallow-child\n").unwrap();
    fs::write(meta_dir(&fx).join("stubbed"), "secret/plan.txt\n").unwrap();
    fx.repo
        .store()
        .delete(&hash_bytes(b"restricted\n"))
        .unwrap();

    let r = fx.repo.fsck(&FsckOptions { full: true }).unwrap();
    assert!(r.is_ok(), "{:?}", r.errors);
    assert_eq!(r.restricted_missing, 1);
}
