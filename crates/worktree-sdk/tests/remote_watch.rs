//! `remote::watch_trees` / `remote::unstage` (v0.5, ENGINEERING_PLAN §13.11)
//! against the in-process fake server.

mod remote_fake_server;

use std::path::Path;
use std::time::Duration;

use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote;
use worktree_sdk::{InitOptions, Repository, SdkError, SnapshotOptions};

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

fn snap(repo: &Repository, file: &str, content: &str) {
    std::fs::write(repo.root().join(file), content).unwrap();
    let opts = SnapshotOptions {
        message: format!("edit {file}"),
        skip_secret_scan: true,
        ..Default::default()
    };
    repo.snapshot(&tree(repo), opts).unwrap();
}

async fn next_event(watch: &mut remote::TreeWatch) -> remote::BranchEvent {
    tokio::time::timeout(Duration::from_secs(10), watch.stream.next())
        .await
        .expect("event within 10s")
        .expect("stream ok")
        .expect("stream open")
}

#[tokio::test]
async fn watch_streams_pushes_of_other_clients() {
    let (_home, server) = setup().await;
    let a = tempfile::tempdir().unwrap();
    let repo = init_repo(a.path(), &server);
    snap(&repo, "a.txt", "1");
    remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();

    let mut watch = remote::watch_trees(&repo, &[tree(&repo)], "origin")
        .await
        .unwrap();
    let server_id = server.tree_id(USER, "proj").unwrap();
    assert_eq!(watch.trees.get(&server_id), Some(&tree(&repo)));

    let old = server.branch_tip(&server_id, "main");
    snap(&repo, "a.txt", "2");
    remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    let ev = next_event(&mut watch).await;
    assert_eq!(ev.tree_id, server_id);
    assert_eq!(ev.branch, "main");
    assert_eq!(ev.old_tip, old);
    assert_eq!(ev.new_tip, server.branch_tip(&server_id, "main"));
    assert_eq!(ev.actor, USER);

    // Events of other trees are filtered out by the server.
    server.emit_branch_event("other-tree", "main", None, Some("x"));
    server.emit_branch_event(&server_id, "feature", None, Some("y"));
    let ev = next_event(&mut watch).await;
    assert_eq!(
        (ev.branch.as_str(), ev.new_tip.as_deref()),
        ("feature", Some("y"))
    );

    server.disconnect_watchers();
    let end = tokio::time::timeout(Duration::from_secs(10), watch.stream.next())
        .await
        .unwrap();
    assert!(matches!(end, Err(SdkError::Remote(_))), "{end:?}");
}

#[tokio::test]
async fn watch_needs_a_tree_on_the_server() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path(), &server);
    let err = remote::watch_trees(&repo, &[tree(&repo)], "origin")
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::TreeNotFound(_)), "{err:?}");
}

#[tokio::test]
async fn watch_errors_map_to_sdk_errors() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path(), &server);
    snap(&repo, "a.txt", "1");
    remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    server.state().watch_unavailable = true;
    let err = remote::watch_trees(&repo, &[tree(&repo)], "origin")
        .await
        .unwrap_err();
    assert!(
        matches!(err, SdkError::Remote(ref m) if m.contains("unavailable")),
        "{err:?}"
    );
}

#[tokio::test]
async fn unstage_removes_own_staged_snapshot() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path(), &server);
    snap(&repo, "a.txt", "1");
    std::fs::write(repo.root().join("wip.txt"), "wip").unwrap();
    remote::stage(&repo, &tree(&repo), "origin").await.unwrap();
    assert_eq!(server.state().staged.len(), 1);

    assert!(
        !remote::unstage(&repo, &tree(&repo), "origin", Some("other"))
            .await
            .unwrap()
    );
    assert!(remote::unstage(&repo, &tree(&repo), "origin", Some("main"))
        .await
        .unwrap());
    assert!(server.state().staged.is_empty());
    assert!(!remote::unstage(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap());

    remote::remove_credential(&server.url).unwrap();
    let err = remote::unstage(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::NotLoggedIn(_)), "{err:?}");
}
