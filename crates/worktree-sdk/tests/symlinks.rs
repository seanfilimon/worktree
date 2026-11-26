//! Symbolic links in the working directory: recorded as links, recreated on
//! checkout, never followed, and never written through.
//!
//! These tests create real symlinks, so they run on Unix only.
#![cfg(unix)]

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use chrono::Utc;
use common::Fixture;
use worktree_sdk::repo::StashOptions;
use worktree_sdk::store::hash_bytes;
use worktree_sdk::{
    ChangeKind, ConflictKind, ConflictResolution, DiffOptions, Manifest, ManifestEntry,
    MergeOptions, MergeOutcome, Snapshot, SwitchOptions,
};

/// Create a symlink at the tree-relative `rel` pointing at `target`.
fn link(f: &Fixture, rel: &str, target: &str) {
    let abs = f.path(rel);
    if let Some(parent) = abs.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    symlink(target, abs).unwrap();
}

/// The target of the symlink at `rel` (panics if it is not a link).
fn target(f: &Fixture, rel: &str) -> String {
    fs::read_link(f.path(rel))
        .unwrap_or_else(|e| panic!("{rel} is not a symlink: {e}"))
        .to_string_lossy()
        .into_owned()
}

fn is_link(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

fn head(f: &Fixture) -> Manifest {
    f.repo
        .manifest_of(&f.repo.resolve(&f.tree, "HEAD").unwrap())
        .unwrap()
}

fn changes(f: &Fixture) -> Vec<(String, ChangeKind)> {
    f.repo
        .status(&f.tree)
        .unwrap()
        .changes
        .into_iter()
        .map(|c| (c.path, c.kind))
        .collect()
}

fn switch(f: &Fixture, branch: &str, create: bool) {
    f.repo
        .switch_branch(
            &f.tree,
            branch,
            SwitchOptions {
                create,
                force: false,
            },
        )
        .unwrap();
}

/// Store `entries` as a snapshot on top of HEAD without touching the
/// working directory, as a fetch from a (possibly hostile) server would.
fn foreign_snapshot(f: &Fixture, entries: Vec<ManifestEntry>) -> Snapshot {
    let parent = f.repo.resolve(&f.tree, "HEAD").unwrap();
    let snapshot = Snapshot {
        id: uuid::Uuid::new_v4().to_string(),
        tree_id: parent.tree_id.clone(),
        branch: parent.branch.clone(),
        parents: vec![parent.id.clone()],
        manifest: f
            .repo
            .write_manifest(&Manifest::from_entries(entries))
            .unwrap(),
        author: "mallory".into(),
        message: "foreign".into(),
        created_at: Utc::now(),
        auto_generated: false,
        dependencies: Vec::new(),
        reverts: None,
        signature: None,
    };
    f.repo.write_snapshot(&snapshot).unwrap();
    snapshot
}

fn file_entry(f: &Fixture, path: &str, content: &[u8]) -> ManifestEntry {
    ManifestEntry {
        path: path.into(),
        hash: f.repo.store().put(content).unwrap(),
        size: content.len() as u64,
        executable: false,
        chunked: false,
        symlink: false,
    }
}

fn link_entry(f: &Fixture, path: &str, target: &str) -> ManifestEntry {
    ManifestEntry::symlink(
        path,
        f.repo.store().put(target.as_bytes()).unwrap(),
        target.len() as u64,
    )
}

#[test]
fn links_are_recorded_as_links_and_never_followed() {
    let f = Fixture::new();
    let outside = f.tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("secret.txt"), "top secret\n").unwrap();

    f.write("real/data.txt", "data\n");
    link(&f, "to-file", "real/data.txt");
    link(&f, "to-dir", "real");
    link(&f, "dangling", "does/not/exist");
    link(&f, "abs", outside.to_str().unwrap());
    link(&f, "escape", "../outside/secret.txt");
    f.snap("links");

    let m = head(&f);
    let paths: Vec<&str> = m.entries.iter().map(|e| e.path.as_str()).collect();
    assert_eq!(
        paths,
        [
            "abs",
            "dangling",
            "escape",
            "real/data.txt",
            "to-dir",
            "to-file"
        ],
        "nothing below a directory link is scanned"
    );
    for (path, want) in [
        ("to-file", "real/data.txt"),
        ("to-dir", "real"),
        ("dangling", "does/not/exist"),
        ("abs", outside.to_str().unwrap()),
        ("escape", "../outside/secret.txt"),
    ] {
        let e = m.get(path).unwrap();
        assert!(e.symlink, "{path} is a symlink entry");
        assert!(!e.executable && !e.chunked, "{path}");
        assert_eq!(e.size, want.len() as u64, "{path}");
        assert_eq!(e.hash, hash_bytes(want.as_bytes()), "{path}");
        // The blob is the target text, not the content behind the link.
        assert_eq!(f.repo.store().read_entry(e).unwrap(), want.as_bytes());
    }
    assert!(!m.get("real/data.txt").unwrap().symlink);
    assert!(f.repo.status(&f.tree).unwrap().is_clean());
    // A second scan (stat cache warm) agrees.
    assert!(f.repo.status(&f.tree).unwrap().is_clean());

    // Editing the file behind a link does not change the link.
    fs::write(outside.join("secret.txt"), "changed\n").unwrap();
    assert!(f.repo.status(&f.tree).unwrap().is_clean());

    // Retargeting is a modification.
    fs::remove_file(f.path("to-file")).unwrap();
    link(&f, "to-file", "real");
    assert_eq!(changes(&f), [("to-file".to_string(), ChangeKind::Modified)]);
    f.snap("retarget");

    // A regular file holding the same bytes as the old target is a
    // different thing: the type change shows.
    fs::remove_file(f.path("dangling")).unwrap();
    f.write("dangling", "does/not/exist");
    assert_eq!(
        changes(&f),
        [("dangling".to_string(), ChangeKind::Modified)]
    );
    f.snap("link became a file");
    assert!(!head(&f).get("dangling").unwrap().symlink);
}

#[test]
fn checkout_recreates_and_removes_links_not_their_targets() {
    let f = Fixture::new();
    let outside = f.tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("keep.txt"), "keep me\n").unwrap();

    f.write("a.txt", "a\n");
    f.snap("base");
    switch(&f, "links", true);
    f.write("real/data.txt", "data\n");
    link(&f, "to-file", "real/data.txt");
    link(&f, "to-dir", "real");
    link(&f, "out", "../outside");
    link(&f, "out-file", "../outside/keep.txt");
    f.snap("links");

    // Leaving the branch removes the links themselves.
    switch(&f, "main", false);
    for rel in ["to-file", "to-dir", "out", "out-file", "real/data.txt"] {
        assert!(fs::symlink_metadata(f.path(rel)).is_err(), "{rel} is gone");
    }
    assert_eq!(
        fs::read_to_string(outside.join("keep.txt")).unwrap(),
        "keep me\n",
        "what the links pointed at is untouched"
    );

    // Coming back recreates them with the stored targets.
    switch(&f, "links", false);
    assert_eq!(target(&f, "to-file"), "real/data.txt");
    assert_eq!(target(&f, "to-dir"), "real");
    assert_eq!(target(&f, "out"), "../outside");
    assert_eq!(target(&f, "out-file"), "../outside/keep.txt");
    assert_eq!(f.read("to-file"), "data\n", "the link resolves");
    assert!(f.repo.status(&f.tree).unwrap().is_clean());

    // restore puts a deleted or damaged link back.
    fs::remove_file(f.path("to-dir")).unwrap();
    fs::remove_file(f.path("to-file")).unwrap();
    f.write("to-file", "now a file\n");
    let restored = f.repo.restore(&f.tree, &[], None).unwrap();
    assert_eq!(restored, ["to-dir", "to-file"]);
    assert_eq!(target(&f, "to-dir"), "real");
    assert_eq!(target(&f, "to-file"), "real/data.txt");
    assert_eq!(f.read("real/data.txt"), "data\n");
}

#[test]
fn a_file_replacing_a_link_never_writes_through_it() {
    let f = Fixture::new();
    let outside = f.tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("victim.txt"), "original\n").unwrap();

    link(&f, "f", "../outside/victim.txt");
    f.snap("f is a link");
    switch(&f, "file", true);
    fs::remove_file(f.path("f")).unwrap();
    f.write("f", "regular content\n");
    f.snap("f is a file");

    switch(&f, "main", false);
    assert_eq!(target(&f, "f"), "../outside/victim.txt");
    // Back to the branch where `f` is a file: the link is replaced.
    switch(&f, "file", false);
    assert!(!is_link(&f.path("f")));
    assert_eq!(f.read("f"), "regular content\n");
    assert_eq!(
        fs::read_to_string(outside.join("victim.txt")).unwrap(),
        "original\n"
    );
}

#[test]
fn checkout_refuses_to_write_or_delete_through_a_linked_directory() {
    let f = Fixture::new();
    let outside = f.tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("victim.txt"), "original\n").unwrap();
    f.write("a.txt", "a\n");
    f.snap("base");

    // A hostile manifest: `dir` is a link out of the tree *and* a
    // directory holding files. Entries are applied in path order, so the
    // link exists by the time `dir/evil.txt` is written.
    let hostile = foreign_snapshot(
        &f,
        vec![
            file_entry(&f, "a.txt", b"a\n"),
            link_entry(&f, "dir", "../outside"),
            file_entry(&f, "dir/evil.txt", b"pwned\n"),
            file_entry(&f, "dir/victim.txt", b"overwritten\n"),
        ],
    );
    let err = f
        .repo
        .checkout(&f.tree, &hostile.id, true)
        .expect_err("writing through the link must fail");
    assert!(err.to_string().contains("symbolic link"), "{err}");
    assert!(!outside.join("evil.txt").exists());
    assert_eq!(
        fs::read_to_string(outside.join("victim.txt")).unwrap(),
        "original\n"
    );

    // The same with a link the user made themselves: a tracked file below
    // a path that is now a link is neither written nor deleted through it.
    let f = Fixture::new();
    let outside = f.tmp.path().join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("victim.txt"), "original\n").unwrap();
    f.write("dir/victim.txt", "tracked\n");
    f.snap("base");
    fs::remove_dir_all(f.path("dir")).unwrap();
    link(&f, "dir", "../outside");
    // `dir/victim.txt` now reads as deleted and `dir` as a new link.
    assert_eq!(
        changes(&f),
        [
            ("dir".to_string(), ChangeKind::Added),
            ("dir/victim.txt".to_string(), ChangeKind::Deleted),
        ]
    );
    let err = f
        .repo
        .restore(&f.tree, &["dir/victim.txt".to_string()], None)
        .expect_err("restoring below a link must fail");
    assert!(err.to_string().contains("symbolic link"), "{err}");
    assert_eq!(
        fs::read_to_string(outside.join("victim.txt")).unwrap(),
        "original\n"
    );
    // Restoring everything removes the stray link first, then succeeds.
    f.repo.restore(&f.tree, &[], None).unwrap();
    assert!(!is_link(&f.path("dir")));
    assert_eq!(f.read("dir/victim.txt"), "tracked\n");
    assert_eq!(
        fs::read_to_string(outside.join("victim.txt")).unwrap(),
        "original\n"
    );
}

#[test]
fn malformed_link_entries_are_rejected() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.snap("base");

    // A target with a NUL byte is refused at checkout.
    let bad = foreign_snapshot(
        &f,
        vec![ManifestEntry::symlink(
            "bad",
            f.repo.store().put(b"a\0b").unwrap(),
            3,
        )],
    );
    assert!(f.repo.checkout(&f.tree, &bad.id, true).is_err());
    assert!(fs::symlink_metadata(f.path("bad")).is_err());

    // Executable / chunked symlinks and links in `.wt-tree/` do not decode.
    let mut exec = link_entry(&f, "x", "a.txt");
    exec.executable = true;
    let mut chunked = link_entry(&f, "x", "a.txt");
    chunked.chunked = true;
    let marker = link_entry(&f, ".wt-tree/hooks/pre-snapshot", "/bin/sh");
    for entry in [exec, chunked, marker] {
        let bytes = Manifest::from_entries(vec![entry]).to_bytes();
        assert!(Manifest::from_bytes(&bytes).is_err());
    }
}

#[test]
fn links_in_the_tree_marker_are_not_shared() {
    let f = Fixture::new();
    f.write("a.txt", "a\n");
    f.write("elsewhere.sh", "#!/bin/sh\n");
    fs::create_dir_all(f.path(".wt-tree/hooks")).unwrap();
    symlink("../../elsewhere.sh", f.path(".wt-tree/hooks/pre-push")).unwrap();
    f.snap("base");
    assert!(head(&f).get(".wt-tree/hooks/pre-push").is_none());
}

#[test]
fn diff_shows_the_target_text() {
    let f = Fixture::new();
    f.write("v1.txt", "one\n");
    f.write("v2.txt", "two\n");
    link(&f, "current", "v1.txt");
    let first = f.snap("v1");
    fs::remove_file(f.path("current")).unwrap();
    link(&f, "current", "v2.txt");

    let diffs = f
        .repo
        .diff_working(&f.tree, None, &DiffOptions::default())
        .unwrap();
    assert_eq!(diffs.len(), 1);
    let d = &diffs[0];
    assert_eq!((d.path.as_str(), d.kind), ("current", ChangeKind::Modified));
    assert!(!d.binary);
    let patch = d.patch.as_deref().unwrap();
    assert!(patch.contains("-v1.txt"), "{patch}");
    assert!(patch.contains("+v2.txt"), "{patch}");
    assert!(!patch.contains("two"), "the link is not followed: {patch}");

    let second = f.snap("v2");
    let diffs = f
        .repo
        .diff_revs(&f.tree, &first.id, &second.id, &DiffOptions::default())
        .unwrap();
    assert_eq!(diffs.len(), 1);
    assert!(diffs[0].patch.as_deref().unwrap().contains("+v2.txt"));
}

#[test]
fn merge_takes_a_one_sided_retarget_and_conflicts_on_two() {
    let f = Fixture::new();
    f.write("v1.txt", "one\n");
    f.write("v2.txt", "two\n");
    f.write("v3.txt", "three\n");
    link(&f, "current", "v1.txt");
    link(&f, "stable", "v1.txt");
    f.snap("base");

    switch(&f, "feature", true);
    fs::remove_file(f.path("current")).unwrap();
    link(&f, "current", "v2.txt");
    fs::remove_file(f.path("stable")).unwrap();
    link(&f, "stable", "v2.txt");
    f.snap("feature retargets both");

    switch(&f, "main", false);
    fs::remove_file(f.path("current")).unwrap();
    link(&f, "current", "v3.txt");
    f.write("other.txt", "unrelated\n");
    f.snap("main retargets current");

    let outcome = f
        .repo
        .merge(&f.tree, "feature", MergeOptions::default())
        .unwrap();
    let MergeOutcome::Conflicts { conflicts } = outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].path, "current");
    assert_eq!(conflicts[0].kind, ConflictKind::Symlink);
    assert!(conflicts[0].hunks.is_empty());
    // Ours stays in the working tree, untouched by markers; the one-sided
    // retarget was taken.
    assert_eq!(target(&f, "current"), "v3.txt");
    assert_eq!(target(&f, "stable"), "v2.txt");
    assert_eq!(f.read("v3.txt"), "three\n", "no markers written through");

    let status = f.repo.merge_status(&f.tree).unwrap().unwrap();
    assert!(!status.conflicts[0].resolved);
    f.repo
        .resolve_conflict(&f.tree, "current", &ConflictResolution::Theirs)
        .unwrap();
    assert_eq!(target(&f, "current"), "v2.txt");
    let merged = f.repo.continue_merge(&f.tree, None).unwrap();
    let m = f.repo.manifest_of(&merged).unwrap();
    let current = m.get("current").unwrap();
    assert!(current.symlink);
    assert_eq!(current.hash, hash_bytes(b"v2.txt"));
    assert!(f.repo.status(&f.tree).unwrap().is_clean());
}

#[test]
fn merge_of_a_link_against_an_edited_file_is_a_conflict() {
    let f = Fixture::new();
    f.write("notes.txt", "line one\nline two\n");
    f.write("real.txt", "real\n");
    f.snap("base");

    switch(&f, "linked", true);
    fs::remove_file(f.path("notes.txt")).unwrap();
    link(&f, "notes.txt", "real.txt");
    f.snap("notes becomes a link");

    switch(&f, "main", false);
    f.write("notes.txt", "line one\nline two\nline three\n");
    f.snap("notes edited");

    let outcome = f
        .repo
        .merge(&f.tree, "linked", MergeOptions::default())
        .unwrap();
    let MergeOutcome::Conflicts { conflicts } = outcome else {
        panic!("expected a conflict, got {outcome:?}");
    };
    assert_eq!(conflicts[0].kind, ConflictKind::Symlink);
    assert!(!is_link(&f.path("notes.txt")));
    assert_eq!(f.read("notes.txt"), "line one\nline two\nline three\n");
    assert_eq!(f.read("real.txt"), "real\n");

    // Resolving with explicit content replaces whatever is there with a
    // regular file.
    f.repo
        .resolve_conflict(&f.tree, "notes.txt", &ConflictResolution::Theirs)
        .unwrap();
    assert_eq!(target(&f, "notes.txt"), "real.txt");
    f.repo
        .resolve_conflict(
            &f.tree,
            "notes.txt",
            &ConflictResolution::Content(b"by hand\n".to_vec()),
        )
        .unwrap();
    assert!(!is_link(&f.path("notes.txt")));
    assert_eq!(f.read("notes.txt"), "by hand\n");
    assert_eq!(f.read("real.txt"), "real\n", "not written through the link");
    f.repo.continue_merge(&f.tree, None).unwrap();
}

#[test]
fn stash_keeps_links() {
    let f = Fixture::new();
    f.write("v1.txt", "one\n");
    f.write("v2.txt", "two\n");
    link(&f, "current", "v1.txt");
    f.snap("base");

    fs::remove_file(f.path("current")).unwrap();
    link(&f, "current", "v2.txt");
    link(&f, "extra", "v1.txt");
    f.repo
        .stash_push(
            &f.tree,
            &StashOptions {
                include_untracked: true,
                ..StashOptions::default()
            },
        )
        .unwrap();
    assert_eq!(target(&f, "current"), "v1.txt");
    assert!(fs::symlink_metadata(f.path("extra")).is_err());

    f.repo.stash_pop(&f.tree, 0).unwrap();
    assert_eq!(target(&f, "current"), "v2.txt");
    assert_eq!(target(&f, "extra"), "v1.txt");
}

#[test]
fn sparse_checkout_handles_links() {
    let f = Fixture::new();
    f.write("src/a.rs", "fn a() {}\n");
    f.write("docs/guide.md", "# guide\n");
    link(&f, "docs/latest", "guide.md");
    f.snap("base");

    let report = f.repo.sparse_set(&f.tree, &["src".to_string()]).unwrap();
    assert_eq!(report.removed, 2);
    assert!(fs::symlink_metadata(f.path("docs/latest")).is_err());
    assert!(f.repo.status(&f.tree).unwrap().is_clean());

    let report = f.repo.sparse_disable(&f.tree).unwrap();
    assert_eq!(report.written, 2);
    assert_eq!(target(&f, "docs/latest"), "guide.md");
    assert!(f.repo.status(&f.tree).unwrap().is_clean());

    // A link replaced by a file with the very same bytes is a local
    // modification: it is kept when its path leaves the sparse set.
    fs::remove_file(f.path("docs/latest")).unwrap();
    f.write("docs/latest", "guide.md");
    let report = f.repo.sparse_set(&f.tree, &["src".to_string()]).unwrap();
    assert_eq!(report.kept_modified, ["docs/latest"]);
    assert_eq!(f.read("docs/latest"), "guide.md");
}
