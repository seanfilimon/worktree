//! `git-remote-wt` driven by the real `git` against the SDK's in-process
//! fake server: stock Git clones, fetches and pushes a server tree while
//! the SDK plays the `wt` side.
//!
//! These tests need a `git` executable on `PATH`, so they are ignored by
//! default (`cargo test -p worktree-git -- --ignored`).

#[path = "../../worktree-sdk/tests/remote_fake_server/mod.rs"]
mod remote_fake_server;

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote;
use worktree_sdk::{InitOptions, Repository, Snapshot, SnapshotOptions};

const USER: &str = "alice";

/// `git` with an isolated configuration, the helper on `PATH` and the
/// test's `WT_HOME`.
struct Git {
    home: tempfile::TempDir,
    wt_home: PathBuf,
    path: OsString,
}

impl Git {
    fn new(wt_home: &Path) -> Self {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("gitconfig"),
            "[core]\n\tautocrlf = false\n[init]\n\tdefaultBranch = main\n\
             [user]\n\tname = Git User\n\temail = git.user@example.com\n\
             [commit]\n\tgpgsign = false\n[tag]\n\tgpgsign = false\n\
             [advice]\n\tdetachedHead = false\n",
        )
        .unwrap();
        let helper = Path::new(env!("CARGO_BIN_EXE_git-remote-wt"));
        let mut dirs = vec![helper.parent().unwrap().to_path_buf()];
        dirs.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        Self {
            home,
            wt_home: wt_home.to_path_buf(),
            path: std::env::join_paths(dirs).unwrap(),
        }
    }

    fn command(&self, dir: &Path, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.args(args)
            .current_dir(dir)
            .env("PATH", &self.path)
            .env("WT_HOME", &self.wt_home)
            .env("HOME", self.home.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", self.home.path().join("gitconfig"))
            .env("GIT_TERMINAL_PROMPT", "0")
            // A local time zone: commits made here are not "canonical".
            .env("TZ", "Europe/Paris")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("WT_TOKEN");
        cmd
    }

    fn run(&self, dir: &Path, args: &[&str]) -> Output {
        self.command(dir, args).output().expect("run git")
    }

    /// Run, require success, return trimmed stdout.
    #[track_caller]
    fn ok(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.run(dir, args);
        assert!(
            out.status.success(),
            "git {args:?} failed: {}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// Run, require failure, return stderr.
    #[track_caller]
    fn fails(&self, dir: &Path, args: &[&str]) -> String {
        let out = self.run(dir, args);
        assert!(!out.status.success(), "git {args:?} should have failed");
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    fn commit_file(&self, dir: &Path, path: &str, content: &str, message: &str) -> String {
        let full = dir.join(path);
        std::fs::create_dir_all(full.parent().unwrap()).unwrap();
        std::fs::write(full, content).unwrap();
        self.ok(dir, &["add", "-A"]);
        self.ok(dir, &["commit", "-q", "-m", message]);
        self.ok(dir, &["rev-parse", "HEAD"])
    }

    /// Like [`Git::commit_file`], dated `at` in UTC by author and committer
    /// alike: a commit a snapshot reproduces exactly.
    fn commit_file_utc(&self, dir: &Path, path: &str, content: &str, at: u64) -> String {
        std::fs::write(dir.join(path), content).unwrap();
        self.ok(dir, &["add", "-A"]);
        let date = format!("{at} +0000");
        let out = self
            .command(dir, &["commit", "-q", "-m", path])
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        self.ok(dir, &["rev-parse", "HEAD"])
    }
}

struct World {
    home: TestHome,
    server: FakeServer,
    git: Git,
    dirs: tempfile::TempDir,
}

impl World {
    /// A fake server with a logged-in user and an isolated `git`.
    async fn new() -> Self {
        let home = TestHome::new();
        let server = FakeServer::start().await;
        remote::register(&server.url, USER, "alice@example.com", "pw")
            .await
            .unwrap();
        let git = Git::new(home.path());
        Self {
            home,
            server,
            git,
            dirs: tempfile::tempdir().unwrap(),
        }
    }

    fn dir(&self, name: &str) -> PathBuf {
        self.dirs.path().join(name)
    }

    fn url(&self, tree: &str) -> String {
        format!("wt::{}/{USER}/{tree}", self.server.url)
    }

    /// `git clone` of `tree` into a new directory `name`.
    fn git_clone(&self, tree: &str, name: &str) -> PathBuf {
        self.git
            .ok(self.dirs.path(), &["clone", "-q", &self.url(tree), name]);
        self.dir(name)
    }

    /// A W0rkTree worktree for tree `proj` with two snapshots, pushed.
    async fn wt_repo(&self, name: &str) -> Repository {
        let mut repo = Repository::init(
            &self.dir(name),
            InitOptions {
                name: Some("proj".into()),
                default_branch: None,
            },
        )
        .unwrap();
        let mut config = repo.config().clone();
        config.remote.insert(
            "origin".into(),
            RemoteSection {
                url: self.server.url.clone(),
                tenant: USER.into(),
            },
        );
        repo.set_config(config).unwrap();
        write(&repo, "README.md", "hello\n");
        write(&repo, "src/lib.rs", "pub fn one() {}\n");
        snap(&repo, "first");
        write(&repo, "README.md", "hello again\n");
        snap(&repo, "second");
        push(&repo).await;
        repo
    }

    fn server_tip(&self, tree: &str, branch: &str) -> Option<String> {
        let id = self.server.tree_id(USER, tree)?;
        self.server.branch_tip(&id, branch)
    }
}

fn tree(repo: &Repository) -> String {
    repo.root_tree().unwrap().name
}

fn write(repo: &Repository, path: &str, content: &str) {
    let full = repo.root().join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn snap(repo: &Repository, message: &str) -> Snapshot {
    let opts = SnapshotOptions {
        message: message.into(),
        author: Some("Alice <alice@example.com>".into()),
        skip_secret_scan: true,
        ..Default::default()
    };
    repo.snapshot(&tree(repo), opts).unwrap()
}

async fn push(repo: &Repository) {
    remote::push(repo, &tree(repo), "origin", None)
        .await
        .unwrap();
}

fn read(dir: &Path, path: &str) -> String {
    std::fs::read_to_string(dir.join(path)).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "integration: needs git on PATH"]
async fn git_clone_commit_push_is_seen_by_wt() {
    let world = World::new().await;
    let git = &world.git;
    let wt = world.wt_repo("wt").await;

    let work = world.git_clone("proj", "work");
    assert_eq!(read(&work, "README.md"), "hello again\n");
    assert_eq!(read(&work, "src/lib.rs"), "pub fn one() {}\n");
    assert_eq!(git.ok(&work, &["log", "--format=%s"]), "second\nfirst");
    assert_eq!(
        git.ok(&work, &["log", "-1", "--format=%an <%ae>"]),
        "Alice <alice@example.com>"
    );
    assert_eq!(git.ok(&work, &["symbolic-ref", "--short", "HEAD"]), "main");
    assert_eq!(
        git.ok(&work, &["rev-parse", "--abbrev-ref", "main@{upstream}"]),
        "origin/main"
    );
    // The cache is hidden inside the Git directory.
    assert!(work.join(".git/wt/origin/.wt").is_dir());
    assert_eq!(git.ok(&work, &["status", "--porcelain"]), "");

    // Another clone computes the same commit ids.
    let other = world.git_clone("proj", "other");
    let cloned_head = git.ok(&work, &["rev-parse", "HEAD"]);
    assert_eq!(git.ok(&other, &["rev-parse", "HEAD"]), cloned_head);

    // Commit with stock Git and push.
    let pushed = git.commit_file(&work, "docs/guide.md", "# Guide\n", "add the guide");
    git.ok(&work, &["push", "-q", "origin", "main"]);
    assert_eq!(git.ok(&work, &["rev-parse", "origin/main"]), pushed);

    // `wt` sees the snapshot: a fresh clone and a pull into the old worktree.
    let fresh = remote::clone(&world.server.url, USER, "proj", &world.dir("fresh"))
        .await
        .unwrap();
    assert_eq!(read(fresh.root(), "docs/guide.md"), "# Guide\n");
    let log = fresh.log(&tree(&fresh), None, None).unwrap();
    let messages: Vec<_> = log.iter().map(|s| s.message.as_str()).collect();
    assert_eq!(messages, vec!["add the guide", "second", "first"]);
    assert_eq!(log[0].author, "Git User <git.user@example.com>");
    assert_eq!(log[0].branch, "main");
    assert_eq!(log[0].parents, vec![log[1].id.clone()]);
    assert_eq!(
        world.server_tip("proj", "main").as_deref(),
        Some(log[0].id.as_str())
    );
    remote::pull(&wt, &tree(&wt), "origin", None).await.unwrap();
    assert_eq!(read(wt.root(), "docs/guide.md"), "# Guide\n");

    // Fetching again changes nothing: the pusher keeps its own commit.
    git.ok(&work, &["fetch", "-q", "origin"]);
    assert_eq!(git.ok(&work, &["rev-parse", "origin/main"]), pushed);
    assert_eq!(git.ok(&work, &["status", "-sb"]), "## main...origin/main");
    // Even with the cache gone: the commit is recognised by its snapshot id.
    std::fs::remove_dir_all(work.join(".git/wt/origin")).unwrap();
    git.ok(&work, &["fetch", "-q", "origin"]);
    assert_eq!(git.ok(&work, &["rev-parse", "origin/main"]), pushed);

    // The other clone fast-forwards to the pushed snapshot.
    git.ok(&other, &["pull", "-q", "--ff-only"]);
    assert_eq!(read(&other, "docs/guide.md"), "# Guide\n");
    assert_eq!(git.ok(&other, &["rev-parse", "HEAD~1"]), cloned_head);
    assert_eq!(
        git.ok(&other, &["log", "-1", "--format=%s"]),
        "add the guide"
    );
    // Same content and history in both clones (the commit object itself is
    // rebuilt from the snapshot, in UTC).
    assert_eq!(
        git.ok(&other, &["rev-parse", "HEAD^{tree}"]),
        git.ok(&work, &["rev-parse", "HEAD^{tree}"])
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "integration: needs git on PATH"]
async fn wt_push_is_seen_by_git_fetch() {
    let world = World::new().await;
    let git = &world.git;
    let wt = world.wt_repo("wt").await;
    let work = world.git_clone("proj", "work");
    let before = git.ok(&work, &["rev-parse", "HEAD"]);

    write(&wt, "src/lib.rs", "pub fn two() {}\n");
    write(&wt, "bin/tool.sh", "#!/bin/sh\n");
    let third = snap(&wt, "third");
    wt.switch_branch(
        &tree(&wt),
        "feature",
        worktree_sdk::SwitchOptions {
            create: true,
            force: false,
        },
    )
    .unwrap();
    write(&wt, "feature.txt", "wip\n");
    snap(&wt, "on feature");
    push(&wt).await;
    remote::push(&wt, &tree(&wt), "origin", Some("main"))
        .await
        .unwrap();

    git.ok(&work, &["fetch", "-q", "origin"]);
    assert_eq!(git.ok(&work, &["rev-parse", "HEAD"]), before);
    assert_eq!(
        git.ok(&work, &["log", "--format=%s", "origin/main"]),
        "third\nsecond\nfirst"
    );
    assert_eq!(git.ok(&work, &["rev-parse", "origin/main~1"]), before);
    assert_eq!(
        git.ok(&work, &["log", "-1", "--format=%s", "origin/feature"]),
        "on feature"
    );
    // The commit time is the snapshot time, in UTC.
    assert_eq!(
        git.ok(&work, &["log", "-1", "--format=%ct %ci", "origin/main"])
            .split(' ')
            .next()
            .unwrap(),
        third.created_at.timestamp().to_string()
    );

    git.ok(&work, &["merge", "-q", "--ff-only", "origin/main"]);
    assert_eq!(read(&work, "src/lib.rs"), "pub fn two() {}\n");
    assert_eq!(read(&work, "bin/tool.sh"), "#!/bin/sh\n");

    // A second fetch has nothing to do and writes no new objects.
    let out = git.run(&work, &["fetch", "origin"]);
    assert!(out.status.success());
    assert!(
        !String::from_utf8_lossy(&out.stderr).contains("received"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "integration: needs git on PATH"]
async fn force_push_and_deletion_are_refused() {
    let world = World::new().await;
    let git = &world.git;
    world.wt_repo("wt").await;
    let work = world.git_clone("proj", "work");
    let tip = world.server_tip("proj", "main");

    // Rewrite the last commit, then try to replace the server's history.
    std::fs::write(work.join("README.md"), "rewritten\n").unwrap();
    git.ok(
        &work,
        &["commit", "-q", "-a", "--amend", "-m", "second, rewritten"],
    );
    let plain = git.fails(&work, &["push", "origin", "main"]);
    assert!(plain.contains("rejected"), "{plain}");

    let forced = git.fails(&work, &["push", "--force", "origin", "main"]);
    assert!(forced.contains("append-only"), "{forced}");
    assert!(forced.contains("[remote rejected]"), "{forced}");
    assert_eq!(world.server_tip("proj", "main"), tip);

    let deleted = git.fails(&work, &["push", "origin", ":main"]);
    assert!(deleted.contains("wt branch archive main"), "{deleted}");
    assert_eq!(world.server_tip("proj", "main"), tip);

    // A forced push that only adds snapshots is an ordinary push.
    git.ok(&work, &["reset", "-q", "--hard", "origin/main"]);
    git.commit_file(&work, "more.txt", "more\n", "third");
    git.ok(&work, &["push", "-q", "--force", "origin", "main"]);
    assert_ne!(world.server_tip("proj", "main"), tip);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "integration: needs git on PATH"]
async fn symlinks_are_pushed_as_links() {
    let world = World::new().await;
    let git = &world.git;
    world.wt_repo("wt").await;
    let work = world.git_clone("proj", "work");
    let tip = world.server_tip("proj", "main");

    // A symbolic link entry, created without needing symlink support here.
    let blob = git.ok(&work, &["rev-parse", "HEAD:README.md"]);
    git.ok(
        &work,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("120000,{blob},docs/link"),
        ],
    );
    git.ok(&work, &["commit", "-q", "-m", "add a link"]);
    git.ok(&work, &["push", "origin", "main"]);
    assert_ne!(world.server_tip("proj", "main"), tip);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "integration: needs git on PATH"]
async fn tags_travel_both_ways() {
    let world = World::new().await;
    let git = &world.git;
    let wt = world.wt_repo("wt").await;
    wt.create_tag(&tree(&wt), "v1", None, None).unwrap();
    wt.create_tag(&tree(&wt), "v1-notes", Some("HEAD"), Some("first release"))
        .unwrap();
    push(&wt).await;

    let work = world.git_clone("proj", "work");
    let head = git.ok(&work, &["rev-parse", "HEAD"]);
    assert_eq!(git.ok(&work, &["rev-parse", "v1"]), head);
    assert_eq!(git.ok(&work, &["cat-file", "-t", "v1-notes"]), "tag");
    assert_eq!(git.ok(&work, &["rev-parse", "v1-notes^{commit}"]), head);
    assert_eq!(
        git.ok(
            &work,
            &["tag", "-l", "--format=%(contents:subject)", "v1-notes"]
        ),
        "first release"
    );

    // Push a new commit with an annotated and a lightweight tag.
    git.commit_file(&work, "CHANGELOG.md", "v2\n", "prepare v2");
    git.ok(&work, &["tag", "-a", "v2", "-m", "second release"]);
    git.ok(&work, &["tag", "v2-light"]);
    let v2 = git.ok(&work, &["rev-parse", "v2"]);
    git.ok(&work, &["push", "-q", "origin", "main", "v2", "v2-light"]);

    let server_tree = world.server.tree_id(USER, "proj").unwrap();
    let tip = world.server_tip("proj", "main").unwrap();
    {
        let state = world.server.state();
        let tags = &state.tags[&server_tree];
        let tag = |name: &str| tags.iter().find(|t| t.name == name).expect(name);
        assert_eq!(tag("v2").target.as_ref().unwrap().value, tip);
        assert_eq!(tag("v2").message.as_deref(), Some("second release"));
        assert_eq!(tag("v2-light").target.as_ref().unwrap().value, tip);
    }

    // Fetching tags afterwards keeps the pusher's own tag object.
    git.ok(&work, &["fetch", "-q", "--tags", "origin"]);
    assert_eq!(git.ok(&work, &["rev-parse", "v2"]), v2);

    // Another clone gets both; tags cannot be moved or deleted from Git.
    let other = world.git_clone("proj", "other");
    assert_eq!(git.ok(&other, &["cat-file", "-t", "v2"]), "tag");
    assert_eq!(git.ok(&other, &["cat-file", "-t", "v2-light"]), "commit");
    let moved = git.fails(&work, &["push", "origin", "+HEAD~1:refs/tags/v2"]);
    assert!(moved.contains("not moved"), "{moved}");
    let deleted = git.fails(&work, &["push", "origin", ":refs/tags/v1"]);
    assert!(deleted.contains("wt tag delete v1 --remote"), "{deleted}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "integration: needs git on PATH"]
async fn pushing_creates_a_new_tree_and_ls_remote_needs_no_repository() {
    let world = World::new().await;
    let git = &world.git;

    // Reading a tree that does not exist is an error...
    let missing = git.fails(world.dirs.path(), &["ls-remote", &world.url("fresh")]);
    assert!(missing.contains("not found"), "{missing}");

    // ...pushing to it creates it (the way to move a Git repository over).
    let work = world.dir("local");
    std::fs::create_dir_all(&work).unwrap();
    git.ok(&work, &["init", "-q"]);
    let first = git.commit_file_utc(&work, "a.txt", "a\n", 1_700_000_000);
    let second = git.commit_file_utc(&work, "b.txt", "b\n", 1_700_000_100);
    git.ok(&work, &["remote", "add", "origin", &world.url("fresh")]);
    git.ok(&work, &["push", "-q", "-u", "origin", "main"]);
    assert!(world.server_tip("fresh", "main").is_some());

    // Outside any repository: lists HEAD and the branch, leaves nothing
    // behind. These commits (one identity, UTC) are rebuilt bit for bit.
    let listed = git.ok(world.dirs.path(), &["ls-remote", &world.url("fresh")]);
    assert_eq!(listed, format!("{second}\tHEAD\n{second}\trefs/heads/main"));
    assert!(!world.dirs.path().join("wt").exists());

    let cloned = remote::clone(&world.server.url, USER, "fresh", &world.dir("wt-clone"))
        .await
        .unwrap();
    assert_eq!(read(cloned.root(), "b.txt"), "b\n");
    let log = cloned.log(&tree(&cloned), None, None).unwrap();
    assert_eq!(log.len(), 2);

    // The pusher's commits are what it gets back.
    git.ok(&work, &["fetch", "-q", "origin"]);
    assert_eq!(git.ok(&work, &["rev-parse", "origin/main"]), second);
    assert_eq!(git.ok(&work, &["rev-parse", "origin/main~1"]), first);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "integration: needs git on PATH"]
async fn pushing_needs_a_login() {
    let world = World::new().await;
    let git = &world.git;
    world.wt_repo("wt").await;
    let work = world.git_clone("proj", "work");
    git.commit_file(&work, "x.txt", "x\n", "x");

    std::fs::remove_file(world.home.credentials_file()).unwrap();
    let err = git.fails(&work, &["push", "origin", "main"]);
    assert!(err.contains("wt login"), "{err}");
}
