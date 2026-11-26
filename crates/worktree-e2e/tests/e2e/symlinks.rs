//! Symbolic links end to end: recorded as links by `wt snapshot`, pushed to
//! the real server, recreated by `wt clone` and `wt pull`, shown as links
//! by the REST API, and exported to / imported from Git as mode 120000.
//!
//! Real symlinks are needed, so the scenario runs on Unix only.

use std::os::unix::fs::symlink;
use std::path::Path;

use worktree_e2e::{require_e2e, Git, Repo, TestEnv};

use crate::common::{add_member, TREE};

fn target(repo: &Repo, rel: &str) -> String {
    std::fs::read_link(repo.path(rel))
        .unwrap_or_else(|e| panic!("{rel} is not a symlink: {e}"))
        .to_string_lossy()
        .into_owned()
}

fn mode(git: &Git, repo: &Path, path: &str) -> String {
    let line = git.out(repo, &["ls-tree", "main", path]);
    line.split_whitespace()
        .next()
        .unwrap_or_default()
        .to_owned()
}

#[test]
fn symlinks_push_clone_pull_and_git() {
    require_e2e!();
    let env = TestEnv::with_server("symlinks");
    let url = env.url();

    // --- alice snapshots a tree with links and pushes it
    let alice = env.user("alice");
    alice.register();
    let a = alice.init(TREE);
    a.write("docs/guide.md", "# guide\n");
    a.write("src/app.txt", "app\n");
    symlink("guide.md", a.path("docs/latest")).unwrap();
    symlink("src", a.path("source")).unwrap();
    symlink("../outside/elsewhere", a.path("dangling")).unwrap();
    a.snapshot("links");
    a.assert_clean();
    a.add_remote("origin", &url, &alice.name);
    a.push();
    let head = a.head_id();

    // --- the server lists them as links and serves the target text
    let api = alice.api();
    let detail = api
        .get(&format!("/api/v1/trees/alice/{TREE}/snapshots/{head}"))
        .ok_json();
    let files = detail["files"].as_array().expect("files");
    let is_link = |path: &str| {
        files
            .iter()
            .find(|f| f["path"] == path)
            .unwrap_or_else(|| panic!("{path} not listed: {detail}"))["symlink"]
            .as_bool()
            .unwrap_or(false)
    };
    assert!(is_link("docs/latest") && is_link("source") && is_link("dangling"));
    assert!(!is_link("docs/guide.md"));
    assert!(
        !files.iter().any(|f| f["path"] == "source/app.txt"),
        "nothing is recorded through a directory link"
    );
    let blob = api.get(&format!(
        "/api/v1/trees/alice/{TREE}/blob/{head}/docs/latest"
    ));
    blob.assert_ok();
    assert_eq!(blob.text(), "guide.md", "the blob is the target text");

    // --- bob clones: the links come back as links
    let bob = env.user("bob");
    bob.register();
    add_member(&api, "alice", "bob", "developer");
    let b = bob.clone_tree("alice", TREE, TREE);
    assert_eq!(target(&b, "docs/latest"), "guide.md");
    assert_eq!(target(&b, "source"), "src");
    assert_eq!(target(&b, "dangling"), "../outside/elsewhere");
    assert_eq!(b.read("docs/latest"), "# guide\n");
    assert_eq!(b.head_id(), head);
    b.assert_clean();

    // --- bob retargets one, removes another; alice pulls
    std::fs::remove_file(b.path("docs/latest")).unwrap();
    b.write("docs/v2.md", "# v2\n");
    symlink("v2.md", b.path("docs/latest")).unwrap();
    std::fs::remove_file(b.path("dangling")).unwrap();
    b.snapshot("retarget");
    b.push();
    a.pull();
    assert_eq!(target(&a, "docs/latest"), "v2.md");
    assert!(std::fs::symlink_metadata(a.path("dangling")).is_err());
    assert_eq!(a.read("src/app.txt"), "app\n");
    a.assert_clean();

    // --- Git: exported as mode 120000, and imported back as links
    if !Git::available() {
        eprintln!("skipping the Git half: no `git` binary");
        return;
    }
    let git = env.git();
    let out = env.path("exported.git");
    std::fs::create_dir_all(&out).unwrap();
    git.run(&out, &["init", "-q", "--bare"]);
    a.wt().args(["git", "export"]).arg(&out).ok();
    assert_eq!(mode(&git, &out, "docs/latest"), "120000");
    assert_eq!(mode(&git, &out, "source"), "120000");
    assert_eq!(mode(&git, &out, "docs/guide.md"), "100644");
    assert_eq!(
        git.out(&out, &["cat-file", "-p", "main:docs/latest"]),
        "v2.md"
    );

    let dest = alice.dir.join("reimported");
    alice.wt().args(["git", "import"]).arg(&out).arg(&dest).ok();
    let again = alice.repo(&dest, "reimported");
    assert_eq!(target(&again, "docs/latest"), "v2.md");
    assert_eq!(target(&again, "source"), "src");
    again.assert_clean();
}
