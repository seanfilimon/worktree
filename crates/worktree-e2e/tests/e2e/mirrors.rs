//! Git mirrors (ENGINEERING_PLAN §15.6): alice pushes a tree, adds a mirror
//! to a local bare Git repository (`--mirror-allow-local` lets the server
//! push to a file:// remote) and syncs it; a plain `git clone` of the
//! remote has her files, history and authorship, and a proprietary path is
//! left out with a LICENSES/NOTICE. A second push reaches the remote
//! through the automatic (debounced) sync as one new commit on top of the
//! unchanged first one, and a manual sync then has nothing to do.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;
use worktree_e2e::git::Git;
use worktree_e2e::json::{find_objects, pretty, str_field};
use worktree_e2e::{require_e2e, TestEnv};

use crate::common::{publish_tree, TREE};

/// `file://` URL of a local path (file:///C:/... on Windows).
fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

#[test]
fn mirrors() {
    require_e2e!();
    if !Git::available() {
        eprintln!("skipping mirrors e2e: git is not installed");
        return;
    }
    let env = TestEnv::with_server_args("mirrors", &["--mirror-allow-local"]);
    let alice = env.user("alice");
    let a = publish_tree(
        &env,
        &alice,
        &[
            ("README.md", "# mirrored\n"),
            ("src/lib.rs", "pub fn f() {}\n"),
            ("vendor/blob.c", "/* proprietary */\n"),
        ],
    );
    let first_snapshot = a.head_id();
    a.wt()
        .args(["license", "set", "vendor/**", "LicenseRef-Proprietary"])
        .ok();

    let git = env.git();
    let root = env.path("");
    let remote = env.path("remote.git");
    git.run(
        &root,
        &[
            "init",
            "--bare",
            "--quiet",
            "--initial-branch=main",
            &remote.to_string_lossy(),
        ],
    );
    let url = file_url(&remote);

    // --- add the mirror (main only) and see it listed
    let created = a
        .wt()
        .args(["mirror", "add", &url, "--branch", "main"])
        .json();
    let id = str_field(&created, "id");
    assert_eq!(str_field(&created, "url"), url, "{}", pretty(&created));
    assert_eq!(created["active"], Value::Bool(true));
    let list = a.wt().args(["mirror", "list"]).json();
    assert_eq!(
        list["git"]["available"],
        Value::Bool(true),
        "the server finds git:\n{}",
        pretty(&list)
    );
    assert!(
        find_objects(&list, &["id", "url"])
            .iter()
            .any(|m| m["id"] == id.as_str()),
        "{}",
        pretty(&list)
    );
    let text = a.wt().args(["mirror", "list"]).ok();
    assert!(text.mentions(&id) && text.mentions("main"), "{text}");

    // --- sync now
    let synced = a.wt().args(["mirror", "sync", &id]).json();
    let result = &synced["result"];
    assert_eq!(result["pushed"], Value::Bool(true), "{}", pretty(&synced));
    assert_eq!(result["commits"].as_u64(), Some(1), "{}", pretty(&synced));
    assert!(
        result["excluded"].to_string().contains("vendor/blob.c"),
        "{}",
        pretty(&synced)
    );
    assert_eq!(
        str_field(&synced["mirror"], "last_status"),
        "ok",
        "{}",
        pretty(&synced)
    );

    // --- a plain git clone of the remote has the files and history
    let clone = env.path("clone");
    git.run(&root, &["clone", "--quiet", &url, &clone.to_string_lossy()]);
    let read = |rel: &str| std::fs::read_to_string(clone.join(rel)).unwrap_or_default();
    assert_eq!(read("README.md"), "# mirrored\n");
    assert_eq!(read("src/lib.rs"), "pub fn f() {}\n");
    assert!(
        !clone.join("vendor").exists(),
        "proprietary paths are not redistributed"
    );
    let notice = read("LICENSES/NOTICE");
    assert!(notice.contains("1 path was excluded"), "NOTICE:\n{notice}");
    let log = git.out(&clone, &["log", "--format=%s|%an|%ae", "main"]);
    assert_eq!(log, "initial|Alice|alice@example.com", "{log}");
    let first_commit = git.out(&clone, &["rev-parse", "HEAD"]);
    assert_eq!(
        git.out(&remote, &["for-each-ref", "--format=%(refname)"]),
        "refs/heads/main"
    );

    // --- a second push is mirrored automatically, incrementally
    let second = a.commit(&[("CHANGELOG.md", "## v2\n")], "second");
    assert_ne!(second, first_snapshot);
    a.push();
    let count = worktree_e2e::poll_until(
        "the automatic sync to push the second snapshot",
        Duration::from_secs(60),
        Duration::from_millis(250),
        || {
            let n = git.out(&remote, &["rev-list", "--count", "main"]);
            if n == "2" {
                Ok(n)
            } else {
                Err(format!("remote main has {n} commits"))
            }
        },
    );
    assert_eq!(count, "2");
    assert_eq!(
        git.out(&remote, &["rev-parse", "main~1"]),
        first_commit,
        "the first commit is unchanged (incremental export)"
    );
    git.run(&clone, &["pull", "--quiet", "--ff-only"]);
    assert_eq!(read("CHANGELOG.md"), "## v2\n");
    assert_eq!(git.out(&clone, &["log", "-1", "--format=%s"]), "second");

    // --- nothing new: a manual sync writes no commits
    let again = worktree_e2e::poll_until(
        "a manual sync with nothing to do",
        Duration::from_secs(30),
        Duration::from_millis(250),
        || {
            let v = a.wt().args(["mirror", "sync", &id]).json();
            if v["result"]["commits"].as_u64() == Some(0) {
                Ok(v)
            } else {
                Err(pretty(&v))
            }
        },
    );
    assert!(
        again["result"]["refs"].to_string().contains("up_to_date"),
        "{}",
        pretty(&again)
    );
    let shown = a.wt().args(["mirror", "show", &id]).json();
    assert_eq!(str_field(&shown, "last_status"), "ok", "{}", pretty(&shown));
    assert!(shown.get("last_sync_at").is_some());

    // --- pause, remove
    a.wt().args(["mirror", "update", &id, "--inactive"]).ok();
    let paused = a.wt().args(["mirror", "sync", &id]).fails();
    assert!(paused.mentions("inactive"), "{paused}");
    a.wt()
        .args([
            "mirror",
            "remove",
            &id,
            "--yes",
            "--tree",
            &format!("alice/{TREE}"),
        ])
        .ok();
    let list = a.wt().args(["mirror", "list"]).json();
    assert!(
        !list.to_string().contains(&id),
        "removed mirror still listed:\n{}",
        pretty(&list)
    );
    // The remote keeps what was pushed.
    assert_eq!(git.out(&remote, &["rev-list", "--count", "main"]), "2");
}
