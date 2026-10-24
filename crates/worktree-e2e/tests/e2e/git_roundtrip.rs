//! Git interop: a real `git` repository (branches, merge, tags) imported
//! with `wt git import` and exported with `wt git export` into a fresh bare
//! repository; trees, history shape and authorship must survive.

use std::collections::BTreeSet;
use std::path::Path;

use worktree_e2e::fsutil::{self, assert_same_files};
use worktree_e2e::json::{self, parents, str_field};
use worktree_e2e::{require_e2e, Git, TestEnv};

const ADA: (&str, &str) = ("Ada Lovelace", "ada@example.com");
const GRACE: (&str, &str) = ("Grace Hopper", "grace@example.com");
const BRANCHES: &[&str] = &["main", "feature", "topic"];
const TAGS: &[&str] = &["v0.1", "v1.0"];

/// Sorted "subject\x1fauthor name\x1fauthor email\x1fauthor time" records.
fn history(git: &Git, repo: &Path, rev: &str) -> Vec<String> {
    let out = git.out(repo, &["log", "--format=%s%x1f%an%x1f%ae%x1f%at", rev]);
    let mut lines: Vec<String> = out.lines().map(String::from).collect();
    lines.sort();
    lines
}

fn tree_of(git: &Git, repo: &Path, rev: &str) -> String {
    git.out(repo, &["rev-parse", &format!("{rev}^{{tree}}")])
}

fn count(git: &Git, repo: &Path, rev: &str, merges_only: bool) -> usize {
    let mut args = vec!["rev-list", "--count"];
    if merges_only {
        args.push("--merges");
    }
    args.push(rev);
    git.out(repo, &args).parse().expect("rev-list --count")
}

#[test]
fn git_roundtrip() {
    require_e2e!();
    assert!(
        Git::available(),
        "the git_roundtrip scenario needs `git` on PATH"
    );
    let env = TestEnv::new("git_roundtrip");
    let git = env.git();

    // ------------------------------------------------ build a git repo
    let src = env.path("src-git");
    std::fs::create_dir_all(&src).unwrap();
    git.run(&src, &["init", "-q"]);
    git.run(&src, &["symbolic-ref", "HEAD", "refs/heads/main"]);
    fsutil::write(&src.join("README.md"), "# imported\n");
    fsutil::write(&src.join("src/main.txt"), "fn main v1\n");
    fsutil::write(&src.join("empty.txt"), "");
    fsutil::write(
        &src.join("assets/bin.dat"),
        (0u8..=255).collect::<Vec<u8>>(),
    );
    git.commit_all(&src, "initial commit", ADA);
    let initial_sha = git.out(&src, &["rev-parse", "HEAD"]);
    fsutil::write(&src.join("src/main.txt"), "fn main v2, improved\n");
    git.commit_all(&src, "second commit", GRACE);

    git.run(&src, &["checkout", "-q", "-b", "feature"]);
    fsutil::write(&src.join("feature.txt"), "feature\n");
    git.commit_all(&src, "feature: add file", GRACE);
    fsutil::write(&src.join("feature.txt"), "feature, refined\n");
    git.commit_all(&src, "feature: refine", ADA);

    git.run(&src, &["checkout", "-q", "main"]);
    fsutil::write(&src.join("README.md"), "# imported\n\nUpdated on main.\n");
    git.commit_all(&src, "main: update readme", ADA);
    git.committing(
        &src,
        &[
            "merge",
            "--no-ff",
            "-q",
            "-m",
            "Merge branch 'feature'",
            "feature",
        ],
        ADA,
    );
    git.run(&src, &["tag", "v1.0"]);
    git.committing(
        &src,
        &["tag", "-a", "v0.1", "-m", "first release", &initial_sha],
        ADA,
    );

    git.run(&src, &["checkout", "-q", "-b", "topic"]);
    fsutil::write(&src.join("deep/nested/dir/file.txt"), "deep\n");
    git.commit_all(&src, "topic: deep file", GRACE);
    git.run(&src, &["checkout", "-q", "main"]);
    assert_eq!(
        count(&git, &src, "main", true),
        1,
        "fixture: one merge on main"
    );

    // ------------------------------------------------------- wt git import
    let alice = env.user("alice");
    let dest = alice.dir.join("imported");
    alice.wt().args(["git", "import"]).arg(&src).arg(&dest).ok();
    let repo = alice.repo(&dest, "imported");

    let branches = repo.wt().args(["branch", "list"]).json();
    let names: BTreeSet<String> = json::names(&branches, &["is_current"])
        .into_iter()
        .collect();
    for b in BRANCHES {
        assert!(names.contains(*b), "imported branches {names:?} lack {b}");
    }
    assert_eq!(
        repo.current_branch(),
        "main",
        "the git HEAD branch is checked out"
    );
    assert_same_files(&src, &dest);
    repo.assert_clean();

    for b in BRANCHES {
        let log = repo.log(&[b]);
        assert_eq!(
            log.len(),
            count(&git, &src, b, false),
            "snapshot count on {b}"
        );
        let merges = log.iter().filter(|s| parents(s).len() == 2).count();
        assert_eq!(merges, count(&git, &src, b, true), "merge snapshots on {b}");
        let mut wt_subjects: Vec<String> = log
            .iter()
            .map(|s| {
                str_field(s, "message")
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .collect();
        wt_subjects.sort();
        let mut git_subjects: Vec<String> = git
            .out(&src, &["log", "--format=%s", b])
            .lines()
            .map(String::from)
            .collect();
        git_subjects.sort();
        assert_eq!(wt_subjects, git_subjects, "messages preserved on {b}");
    }
    let main_log = repo.log(&["main"]);
    let initial = main_log
        .iter()
        .find(|s| str_field(s, "message").trim() == "initial commit")
        .expect("initial commit imported");
    let author = str_field(initial, "author");
    assert!(
        author.contains(ADA.0) && author.contains(ADA.1),
        "author preserved: {author:?}"
    );
    assert!(parents(initial).is_empty());
    assert!(
        str_field(initial, "created_at").starts_with("2023-11-14T23:13:20"),
        "author timestamp preserved: {}",
        json::pretty(initial)
    );

    let tags = repo.wt().args(["tag", "list"]).json();
    let tag_names: BTreeSet<String> = json::names(&tags, &["target"]).into_iter().collect();
    for t in TAGS {
        assert!(
            tag_names.contains(*t),
            "imported tags {tag_names:?} lack {t}"
        );
    }

    // ------------------------------------------------------- wt git export
    let out = env.path("exported.git");
    std::fs::create_dir_all(&out).unwrap();
    git.run(&out, &["init", "-q", "--bare"]);
    repo.wt().args(["git", "export"]).arg(&out).ok();

    for b in BRANCHES {
        assert_eq!(
            tree_of(&git, &out, b),
            tree_of(&git, &src, b),
            "{b}^{{tree}} equal after export"
        );
        assert_eq!(
            count(&git, &out, b, false),
            count(&git, &src, b, false),
            "commit count on {b}"
        );
        assert_eq!(
            count(&git, &out, b, true),
            count(&git, &src, b, true),
            "merge count on {b}"
        );
        assert_eq!(
            history(&git, &out, b),
            history(&git, &src, b),
            "subjects/authors/dates on {b}"
        );
    }
    for t in TAGS {
        assert_eq!(
            tree_of(&git, &out, t),
            tree_of(&git, &src, t),
            "tag {t} exported"
        );
    }
    git.run(&out, &["fsck", "--no-progress"]);

    // Re-export is incremental and deterministic.
    let heads_before: Vec<String> = BRANCHES
        .iter()
        .map(|b| git.out(&out, &["rev-parse", b]))
        .collect();
    repo.wt().args(["git", "export"]).arg(&out).ok();
    let heads_after: Vec<String> = BRANCHES
        .iter()
        .map(|b| git.out(&out, &["rev-parse", b]))
        .collect();
    assert_eq!(
        heads_before, heads_after,
        "re-export must not rewrite commits"
    );
}
