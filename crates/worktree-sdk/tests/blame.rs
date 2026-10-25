//! `Repository::blame` (ENGINEERING_PLAN §14.4): edits, insertions,
//! deletions, merges, renames (not followed), ranges, binary and chunked
//! files.

mod common;

use common::Fixture;
use worktree_sdk::{BlameLine, MergeOptions, MergeOutcome, SdkError, Snapshot, SwitchOptions};

fn switch(fx: &Fixture, name: &str, create: bool) {
    fx.repo
        .switch_branch(
            &fx.tree,
            name,
            SwitchOptions {
                create,
                force: false,
            },
        )
        .unwrap();
}

/// (text, snapshot id) per line.
fn owners(lines: &[BlameLine]) -> Vec<(&str, &str)> {
    lines
        .iter()
        .map(|l| (l.text.as_str(), l.snapshot_id.as_str()))
        .collect()
}

#[test]
fn linear_edits_insertions_and_deletions() {
    let fx = Fixture::new();
    fx.write("f.txt", "a\nb\nc\nd\n");
    let s1 = fx.snap("first\nbody");
    fx.write("f.txt", "a\nB\nc\nd\n");
    let s2 = fx.snap("edit b");
    fx.write("f.txt", "a\nB\nnew1\nnew2\nc\nd\n");
    let s3 = fx.snap("insert");
    fx.write("f.txt", "a\nB\nnew1\nnew2\nd\n");
    let s4 = fx.snap("delete c");

    let lines = fx.repo.blame(&fx.tree, "f.txt", None).unwrap();
    assert_eq!(
        owners(&lines),
        [
            ("a", s1.id.as_str()),
            ("B", s2.id.as_str()),
            ("new1", s3.id.as_str()),
            ("new2", s3.id.as_str()),
            ("d", s1.id.as_str()),
        ]
    );
    assert_eq!(
        lines.iter().map(|l| l.line).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    // Metadata comes from the attributed snapshot; message is its subject.
    assert_eq!(lines[0].message, "first");
    assert_eq!(lines[0].author, s1.author);
    assert_eq!(lines[0].created_at, s1.created_at);
    // The deletion snapshot owns no line, but older revisions still blame.
    assert!(lines.iter().all(|l| l.snapshot_id != s4.id));
    let at_s2 = fx.repo.blame(&fx.tree, "f.txt", Some(&s2.id)).unwrap();
    assert_eq!(
        owners(&at_s2),
        [
            ("a", s1.id.as_str()),
            ("B", s2.id.as_str()),
            ("c", s1.id.as_str()),
            ("d", s1.id.as_str()),
        ]
    );
    let prev = fx.repo.blame(&fx.tree, "f.txt", Some("HEAD~1")).unwrap();
    assert_eq!(prev.len(), 6);
}

#[test]
fn snapshots_not_touching_the_file_are_skipped() {
    let fx = Fixture::new();
    fx.write("f.txt", "x\ny\n");
    let s1 = fx.snap("one");
    for i in 0..5 {
        fx.write("other.txt", &format!("{i}\n"));
        fx.snap("other");
    }
    fx.write("f.txt", "x\nY\n");
    let s2 = fx.snap("two");
    fx.write("other.txt", "last\n");
    fx.snap("other again");
    let lines = fx.repo.blame(&fx.tree, "f.txt", None).unwrap();
    assert_eq!(
        owners(&lines),
        [("x", s1.id.as_str()), ("Y", s2.id.as_str())]
    );
}

/// The shared Go/Rust scenario (keep in sync with
/// services/server-go/internal/blame/blame_test.go `TestSharedScenario`):
///
/// ```text
/// S1 base     a.txt = one two three
/// S2 feature  (branch from S1) one two three feature
/// S3 top      (main) zero one two three
/// S4 merge    feature into main: zero one two three feature
/// S5 edit     zero one TWO feature
/// blame a.txt at S5 = zero:S3 one:S1 TWO:S5 feature:S2
/// ```
#[test]
fn shared_scenario_merge_follows_the_merged_branch() {
    let fx = Fixture::new();
    fx.write("a.txt", "one\ntwo\nthree\n");
    let s1 = fx.snap("base");
    switch(&fx, "feature", true);
    fx.write("a.txt", "one\ntwo\nthree\nfeature\n");
    let s2 = fx.snap("feature");
    switch(&fx, "main", false);
    fx.write("a.txt", "zero\none\ntwo\nthree\n");
    let s3 = fx.snap("top");
    let s4: Snapshot = match fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap()
    {
        MergeOutcome::Merged { snapshot } => snapshot,
        other => panic!("expected a merge snapshot, got {other:?}"),
    };
    assert_eq!(s4.parents, [s3.id.clone(), s2.id.clone()]);
    assert_eq!(fx.read("a.txt"), "zero\none\ntwo\nthree\nfeature\n");
    fx.write("a.txt", "zero\none\nTWO\nfeature\n");
    let s5 = fx.snap("edit");

    let lines = fx.repo.blame(&fx.tree, "a.txt", None).unwrap();
    assert_eq!(
        owners(&lines),
        [
            ("zero", s3.id.as_str()),
            ("one", s1.id.as_str()),
            ("TWO", s5.id.as_str()),
            ("feature", s2.id.as_str()),
        ]
    );
    // At the merge itself every line still belongs to a parent's history.
    let at_merge = fx.repo.blame(&fx.tree, "a.txt", Some(&s4.id)).unwrap();
    assert!(at_merge.iter().all(|l| l.snapshot_id != s4.id));
}

#[test]
fn merge_resolution_lines_belong_to_the_merge() {
    let fx = Fixture::new();
    fx.write("m.txt", "1\n2\n3\n");
    let s1 = fx.snap("base");
    switch(&fx, "feature", true);
    fx.write("m.txt", "1\nfeature\n3\n");
    fx.snap("feature edit");
    switch(&fx, "main", false);
    fx.write("m.txt", "1\nmain\n3\n");
    fx.snap("main edit");
    let outcome = fx
        .repo
        .merge(&fx.tree, "feature", MergeOptions::default())
        .unwrap();
    assert!(matches!(outcome, MergeOutcome::Conflicts { .. }));
    fx.write("m.txt", "1\nresolved\n3\n");
    let merged = fx.repo.continue_merge(&fx.tree, None).unwrap();
    let lines = fx.repo.blame(&fx.tree, "m.txt", None).unwrap();
    assert_eq!(
        owners(&lines),
        [
            ("1", s1.id.as_str()),
            ("resolved", merged.id.as_str()),
            ("3", s1.id.as_str()),
        ]
    );
}

#[test]
fn renames_are_not_followed() {
    let fx = Fixture::new();
    fx.write("old.txt", "p\nq\n");
    let s1 = fx.snap("create");
    fx.remove("old.txt");
    fx.write("new.txt", "p\nq\n");
    let s2 = fx.snap("rename");
    let lines = fx.repo.blame(&fx.tree, "new.txt", None).unwrap();
    assert_eq!(
        owners(&lines),
        [("p", s2.id.as_str()), ("q", s2.id.as_str())]
    );
    // The old path is still blamable at its own revision.
    let old = fx.repo.blame(&fx.tree, "old.txt", Some(&s1.id)).unwrap();
    assert_eq!(owners(&old), [("p", s1.id.as_str()), ("q", s1.id.as_str())]);
    // ...but not at HEAD.
    let err = fx.repo.blame(&fx.tree, "old.txt", None).unwrap_err();
    assert!(matches!(err, SdkError::InvalidArgument(_)), "{err}");
}

#[test]
fn ranges_select_and_validate_lines() {
    let fx = Fixture::new();
    fx.write("r.txt", "1\n2\n3\n4\n5\n");
    let s1 = fx.snap("one");
    fx.write("r.txt", "1\n2\nthree\n4\n5\n");
    let s2 = fx.snap("two");
    let lines = fx
        .repo
        .blame_lines(&fx.tree, "r.txt", None, Some(2..=3))
        .unwrap();
    assert_eq!(
        lines
            .iter()
            .map(|l| (l.line, l.text.as_str(), l.snapshot_id.as_str()))
            .collect::<Vec<_>>(),
        [(2, "2", s1.id.as_str()), (3, "three", s2.id.as_str())]
    );
    // The end is clamped to the file length.
    let tail = fx
        .repo
        .blame_lines(&fx.tree, "r.txt", None, Some(4..=99))
        .unwrap();
    assert_eq!(tail.iter().map(|l| l.line).collect::<Vec<_>>(), [4, 5]);
    for (lo, hi) in [(0, 2), (3, 2), (6, 7)] {
        let bad = lo..=hi;
        let err = fx
            .repo
            .blame_lines(&fx.tree, "r.txt", None, Some(bad.clone()))
            .unwrap_err();
        assert!(
            matches!(err, SdkError::InvalidArgument(_)),
            "{bad:?}: {err}"
        );
    }
}

#[test]
fn binary_missing_and_empty_files() {
    let fx = Fixture::new();
    std::fs::write(fx.path("bin.dat"), [0u8, 1, 2, 3]).unwrap();
    fx.write("empty.txt", "");
    fx.write("crlf.txt", "a\r\nb");
    let s1 = fx.snap("files");
    let err = fx.repo.blame(&fx.tree, "bin.dat", None).unwrap_err();
    assert!(err.to_string().contains("binary"), "{err}");
    let err = fx.repo.blame(&fx.tree, "nope.txt", None).unwrap_err();
    assert!(err.to_string().contains("does not exist"), "{err}");
    assert!(fx
        .repo
        .blame(&fx.tree, "empty.txt", None)
        .unwrap()
        .is_empty());
    let crlf = fx.repo.blame(&fx.tree, "crlf.txt", None).unwrap();
    assert_eq!(
        owners(&crlf),
        [("a", s1.id.as_str()), ("b", s1.id.as_str())]
    );
}

#[test]
fn text_that_was_binary_before_is_attributed_to_the_change() {
    let fx = Fixture::new();
    std::fs::write(fx.path("f"), [0u8, b'a', b'\n']).unwrap();
    fx.snap("binary");
    fx.write("f", "a\n");
    let s2 = fx.snap("text");
    let lines = fx.repo.blame(&fx.tree, "f", None).unwrap();
    assert_eq!(owners(&lines), [("a", s2.id.as_str())]);
}

#[test]
fn empty_branch_is_an_error() {
    let fx = Fixture::new();
    let err = fx.repo.blame(&fx.tree, "x", None).unwrap_err();
    assert!(matches!(err, SdkError::InvalidRevision(_)), "{err}");
}

#[test]
fn chunked_files_are_read_through_read_entry() {
    let fx = Fixture::new();
    // > 8 MiB: stored as content-defined chunks.
    let mut text = String::with_capacity(10 * 1024 * 1024);
    let mut i = 0;
    while text.len() <= 9 * 1024 * 1024 {
        text.push_str(&format!("line number {i:08}\n"));
        i += 1;
    }
    fx.write("big.txt", &text);
    let s1 = fx.snap("big");
    let head = fx.repo.resolve(&fx.tree, "HEAD").unwrap();
    let manifest = fx.repo.manifest_of(&head).unwrap();
    assert!(manifest.get("big.txt").unwrap().chunked);
    let edited = text.replacen("line number 00000010\n", "changed\n", 1);
    fx.write("big.txt", &edited);
    let s2 = fx.snap("edit");
    let lines = fx
        .repo
        .blame_lines(&fx.tree, "big.txt", None, Some(10..=12))
        .unwrap();
    assert_eq!(
        owners(&lines),
        [
            ("line number 00000009", s1.id.as_str()),
            ("changed", s2.id.as_str()),
            ("line number 00000011", s1.id.as_str()),
        ]
    );
}
