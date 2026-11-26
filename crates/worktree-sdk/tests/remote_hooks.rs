//! `pre-push` / `post-push` and the merge hooks of `pull`, against the
//! in-process fake server.

mod hook_util;
mod remote_fake_server;

use std::path::Path;
use std::time::{Duration, Instant};

use hook_util::{
    clear_records, hook_env, hook_file, hook_log, hook_stdin, remove_hook, script, write_hook,
};
use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::{ConfigScope, RemoteSection};
use worktree_sdk::remote::{self, PullOptions, PushOptions};
use worktree_sdk::{InitOptions, MergeOutcome, Repository, Snapshot, SnapshotOptions};

const USER: &str = "alice";

async fn setup() -> (TestHome, FakeServer) {
    let home = TestHome::new();
    let server = FakeServer::start().await;
    remote::register(&server.url, USER, "alice@example.com", "pw")
        .await
        .unwrap();
    (home, server)
}

fn init_repo(dir: &Path, server: &FakeServer) -> Repository {
    let mut repo = Repository::init(
        dir,
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
            url: server.url.clone(),
            tenant: USER.into(),
        },
    );
    repo.set_config(config).unwrap();
    repo
}

fn tree(repo: &Repository) -> String {
    repo.root_tree().unwrap().name
}

fn commit(repo: &Repository, content: &str, message: &str) -> Snapshot {
    std::fs::write(repo.root().join("f.txt"), content).unwrap();
    repo.snapshot(
        &tree(repo),
        SnapshotOptions {
            message: message.into(),
            skip_secret_scan: true,
            ..Default::default()
        },
    )
    .unwrap()
}

fn read(repo: &Repository) -> String {
    std::fs::read_to_string(repo.root().join("f.txt")).unwrap()
}

fn tip(repo: &Repository) -> Option<String> {
    repo.branch_tip(&tree(repo), "main").unwrap()
}

fn server_tip(server: &FakeServer) -> Option<String> {
    let id = server.tree_id(USER, "proj")?;
    server.branch_tip(&id, "main")
}

/// Upload, negotiate and push calls the server has seen.
fn transfer_calls(server: &FakeServer) -> (usize, usize, usize, usize) {
    let st = server.state();
    (
        st.upload_calls,
        st.pack_upload_calls,
        st.negotiate_calls,
        st.push_calls.len(),
    )
}

fn skip_hooks() -> PushOptions {
    PushOptions {
        skip_hooks: true,
        ..PushOptions::default()
    }
}

async fn push(repo: &Repository) -> worktree_sdk::Result<remote::PushReport> {
    remote::push(repo, &tree(repo), "origin", None).await
}

#[tokio::test]
async fn push_hooks_get_the_push_context() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(dir.path(), &server);
    let root = a.root().to_path_buf();
    write_hook(&root, "pre-push", script::RECORD);
    write_hook(&root, "post-push", script::RECORD);

    // A new branch: no old tip.
    let s1 = commit(&a, "1\n", "one");
    push(&a).await.unwrap();
    assert_eq!(hook_log(&root), ["pre-push", "post-push"]);
    let pre = hook_env(&root, "pre-push");
    assert_eq!(pre["WT_HOOK"], "pre-push");
    assert_eq!(pre["WT_TREE"], tree(&a));
    assert_eq!(pre["WT_BRANCH"], "main");
    assert_eq!(pre["WT_REMOTE"], "origin");
    assert_eq!(
        pre["WT_REMOTE_URL"],
        remote::normalize_server_url(&server.url).unwrap()
    );
    assert_eq!(pre["WT_PUSH_OLD_TIP"], "");
    assert_eq!(pre["WT_PUSH_NEW_TIP"], s1.id);
    assert_eq!(pre["WT_PUSH_FORCE"], "0");
    assert_eq!(pre["WT_PUSH_SNAPSHOTS"], "1");
    assert_eq!(hook_stdin(&root, "pre-push"), format!("{}\n", s1.id));

    // An update: the old server tip and the new snapshots, oldest first.
    clear_records(&root);
    let s2 = commit(&a, "2\n", "two");
    let s3 = commit(&a, "3\n", "three");
    push(&a).await.unwrap();
    for phase in ["pre-push", "post-push"] {
        let env = hook_env(&root, phase);
        assert_eq!(env["WT_HOOK"], phase);
        assert_eq!(env["WT_PUSH_OLD_TIP"], s1.id, "{phase}");
        assert_eq!(env["WT_PUSH_NEW_TIP"], s3.id, "{phase}");
        assert_eq!(env["WT_PUSH_SNAPSHOTS"], "2", "{phase}");
        assert_eq!(
            hook_stdin(&root, phase),
            format!("{}\n{}\n", s2.id, s3.id),
            "{phase}"
        );
    }
    assert_eq!(server_tip(&server), Some(s3.id.clone()));

    // Nothing to push: no hooks.
    clear_records(&root);
    let report = push(&a).await.unwrap();
    assert!(report.up_to_date);
    assert!(hook_log(&root).is_empty());
}

#[tokio::test]
async fn failing_pre_push_uploads_nothing_and_skip_hooks_bypasses() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(dir.path(), &server);
    let root = a.root().to_path_buf();
    let s1 = commit(&a, "1\n", "one");
    push(&a).await.unwrap();
    let s2 = commit(&a, "2\n", "two");

    write_hook(&root, "pre-push", script::FAIL);
    write_hook(&root, "post-push", script::RECORD);
    let before = transfer_calls(&server);
    let reflog_before = a.reflog(&tree(&a), None).unwrap().len();
    let msg = push(&a).await.unwrap_err().to_string();
    assert!(msg.starts_with("pre-push hook failed:"), "{msg}");
    assert!(msg.contains("exit code 7"), "{msg}");
    assert!(msg.contains("blocked by hook"), "{msg}");
    assert_eq!(
        transfer_calls(&server),
        before,
        "nothing negotiated or sent"
    );
    assert_eq!(
        server_tip(&server),
        Some(s1.id.clone()),
        "remote tip unchanged"
    );
    assert_eq!(
        a.remote_tip(&tree(&a), "origin", "main").unwrap(),
        Some(s1.id.clone())
    );
    assert_eq!(a.reflog(&tree(&a), None).unwrap().len(), reflog_before);
    assert_eq!(hook_log(&root), ["pre-push"], "no post-push");

    // The bypass skips both push hooks.
    clear_records(&root);
    let report = remote::push_with_options(&a, &tree(&a), "origin", None, &skip_hooks())
        .await
        .unwrap();
    assert_eq!(report.new_tip, s2.id);
    assert_eq!(server_tip(&server), Some(s2.id));
    assert!(hook_log(&root).is_empty());
}

#[tokio::test]
async fn failing_post_push_keeps_the_push() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(dir.path(), &server);
    let root = a.root().to_path_buf();
    write_hook(&root, "post-push", script::FAIL);
    let s1 = commit(&a, "1\n", "one");
    let report = push(&a).await.unwrap();
    assert_eq!(report.new_tip, s1.id);
    assert_eq!(hook_log(&root), ["post-push"]);
    assert_eq!(server_tip(&server), Some(s1.id.clone()));
    assert_eq!(
        a.remote_tip(&tree(&a), "origin", "main").unwrap(),
        Some(s1.id)
    );
}

#[tokio::test]
async fn pre_push_timeout_aborts() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let mut a = init_repo(dir.path(), &server);
    let root = a.root().to_path_buf();
    a.set_config_value(&ConfigScope::Worktree, "hooks.timeout_secs", "1")
        .unwrap();
    write_hook(&root, "pre-push", script::SLEEP);
    commit(&a, "1\n", "one");
    let before = transfer_calls(&server);
    let started = Instant::now();
    let msg = push(&a).await.unwrap_err().to_string();
    assert!(msg.starts_with("pre-push hook failed:"), "{msg}");
    assert!(msg.contains("timed out after 1s"), "{msg}");
    assert!(started.elapsed() < Duration::from_secs(20));
    assert_eq!(transfer_calls(&server), before);
    assert_eq!(server_tip(&server), None);
}

/// `a` pushed `f.txt = "1"` (s1); `b` is a clone of it.
async fn two_clones(
    server: &FakeServer,
    a_dir: &Path,
    b_dir: &Path,
) -> (Repository, Repository, Snapshot) {
    let a = init_repo(a_dir, server);
    let s1 = commit(&a, "1\n", "one");
    push(&a).await.unwrap();
    let b = remote::clone(&server.url, USER, "proj", &b_dir.join("b"))
        .await
        .unwrap();
    (a, b, s1)
}

#[tokio::test]
async fn pull_runs_the_merge_hooks_for_a_fast_forward() {
    let (_home, server) = setup().await;
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b, s1) = two_clones(&server, a_dir.path(), b_dir.path()).await;
    let s2 = commit(&a, "2\n", "two");
    push(&a).await.unwrap();

    let root = b.root().to_path_buf();
    write_hook(&root, "pre-merge", script::RECORD);
    write_hook(&root, "post-merge", script::RECORD);
    let pulled = remote::pull(&b, &tree(&b), "origin", None).await.unwrap();
    assert_eq!(
        pulled.outcome,
        MergeOutcome::FastForward { to: s2.id.clone() }
    );
    assert_eq!(hook_log(&root), ["pre-merge", "post-merge"]);
    let pre = hook_env(&root, "pre-merge");
    assert_eq!(pre["WT_BRANCH"], "main");
    assert_eq!(pre["WT_REMOTE"], "origin");
    assert_eq!(pre["WT_MERGE_SOURCE"], "origin/main");
    assert_eq!(pre["WT_MERGE_SOURCE_TIP"], s2.id);
    assert_eq!(pre["WT_MERGE_OLD_TIP"], s1.id);
    assert_eq!(pre["WT_MERGE_KIND"], "fast-forward");
    assert_eq!(hook_file(&root, "pre-merge"), "1\n");
    let post = hook_env(&root, "post-merge");
    assert_eq!(post["WT_MERGE_RESULT"], "fast-forward");
    assert_eq!(post["WT_MERGE_NEW_TIP"], s2.id);
    assert_eq!(hook_file(&root, "post-merge"), "2\n");
}

#[tokio::test]
async fn failing_pre_merge_stops_a_pull_after_the_fetch() {
    let (_home, server) = setup().await;
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b, s1) = two_clones(&server, a_dir.path(), b_dir.path()).await;
    let s2 = commit(&a, "2\n", "two");
    push(&a).await.unwrap();

    let root = b.root().to_path_buf();
    write_hook(&root, "pre-merge", script::FAIL);
    let msg = remote::pull(&b, &tree(&b), "origin", None)
        .await
        .unwrap_err()
        .to_string();
    assert!(msg.starts_with("pre-merge hook failed:"), "{msg}");
    assert_eq!(tip(&b), Some(s1.id.clone()), "local branch unchanged");
    assert_eq!(read(&b), "1\n", "working directory unchanged");
    assert_eq!(
        b.remote_tip(&tree(&b), "origin", "main").unwrap(),
        Some(s2.id.clone()),
        "the fetch itself is kept, like `wt fetch`"
    );

    let opts = PullOptions {
        skip_hooks: true,
        ..PullOptions::default()
    };
    let pulled = remote::pull_with(&b, &tree(&b), "origin", None, opts)
        .await
        .unwrap();
    assert_eq!(pulled.outcome, MergeOutcome::FastForward { to: s2.id });
    assert_eq!(read(&b), "2\n");
    remove_hook(&root, "pre-merge");
}

#[tokio::test]
async fn post_merge_of_a_conflicting_pull_lists_the_conflicts() {
    let (_home, server) = setup().await;
    let (a_dir, b_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b, _s1) = two_clones(&server, a_dir.path(), b_dir.path()).await;
    let s2 = commit(&a, "from a\n", "a");
    push(&a).await.unwrap();
    let b_tip = commit(&b, "from b\n", "b");

    let root = b.root().to_path_buf();
    write_hook(&root, "pre-merge", script::RECORD);
    write_hook(&root, "post-merge", script::RECORD);
    let pulled = remote::pull(&b, &tree(&b), "origin", None).await.unwrap();
    assert!(
        matches!(pulled.outcome, MergeOutcome::Conflicts { .. }),
        "{:?}",
        pulled.outcome
    );
    assert_eq!(hook_log(&root), ["pre-merge", "post-merge"]);
    let post = hook_env(&root, "post-merge");
    assert_eq!(post["WT_REMOTE"], "origin");
    assert_eq!(post["WT_MERGE_SOURCE"], "origin/main");
    assert_eq!(post["WT_MERGE_SOURCE_TIP"], s2.id);
    assert_eq!(post["WT_MERGE_KIND"], "merge");
    assert_eq!(post["WT_MERGE_RESULT"], "conflicts");
    assert_eq!(post["WT_MERGE_CONFLICTS"], "1");
    assert_eq!(post["WT_MERGE_NEW_TIP"], b_tip.id);
    assert_eq!(hook_stdin(&root, "post-merge"), "f.txt\n");
}
