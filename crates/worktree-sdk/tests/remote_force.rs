//! history-v2 (ENGINEERING_PLAN §16.10): `push --force` with lease
//! semantics, the bypass metadata, operation ids and remote tag deletion,
//! against the in-process fake server.

mod remote_fake_server;

use std::path::Path;

use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote::{self, PushOptions};
use worktree_sdk::{InitOptions, Repository, SdkError, Snapshot, SnapshotOptions};

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

fn commit(repo: &Repository, path: &str, content: &str, message: &str) -> Snapshot {
    std::fs::write(repo.root().join(path), content).unwrap();
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

fn force() -> PushOptions {
    PushOptions {
        force: true,
        bypass_protection: false,
    }
}

#[tokio::test]
async fn force_push_rewinds_with_lease() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    let base = commit(&a, "f.txt", "1\n", "base");
    let s2 = commit(&a, "f.txt", "2\n", "second");
    let report = remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    assert!(!report.forced);
    let tree_id = server.tree_id(USER, "proj").unwrap();

    // Rewrite main locally: undo the second snapshot, commit something else.
    a.undo(&tree(&a), Default::default()).unwrap();
    assert_eq!(
        a.branch_tip(&tree(&a), "main").unwrap(),
        Some(base.id.clone())
    );
    let s3 = commit(&a, "f.txt", "3\n", "rewritten");

    // A plain push is refused; a force push rewinds (lease = s2, known).
    let err = remote::push(&a, &tree(&a), "origin", None)
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::PushRejected(_)), "{err:?}");
    let report = remote::push_with_options(&a, &tree(&a), "origin", None, &force())
        .await
        .unwrap();
    assert!(report.forced);
    assert_eq!(report.old_tip.as_deref(), Some(s2.id.as_str()));
    assert_eq!(
        server.branch_tip(&tree_id, "main").as_deref(),
        Some(s3.id.as_str())
    );
    {
        let st = server.state();
        let (op, forced, bypass) = st.push_calls.last().unwrap().clone();
        assert!(forced && !bypass && !op.is_empty(), "{:?}", st.push_calls);
        let ids: std::collections::HashSet<_> = st.push_calls.iter().map(|c| c.0.clone()).collect();
        assert_eq!(
            ids.len(),
            st.push_calls.len(),
            "every push gets its own operation id"
        );
    }
    assert_eq!(
        a.reflog(&tree(&a), Some(1)).unwrap()[0].action,
        "force-push"
    );

    // Someone else moves the server branch; a's lease (s3) no longer holds.
    let s4 = commit(&a, "f.txt", "4\n", "fourth");
    server.set_branch(&tree_id, "main", Some(&s4.id));
    let s5_parent = s3.id.clone();
    a.set_branch_tip(&tree(&a), "main", Some(&s5_parent))
        .unwrap();
    commit(&a, "g.txt", "5\n", "fifth");
    let err = remote::push_with_options(&a, &tree(&a), "origin", None, &force())
        .await
        .unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("refusing to force-push") && msg.contains("wt fetch"),
        "{msg}"
    );
    assert_eq!(
        server.branch_tip(&tree_id, "main").as_deref(),
        Some(s4.id.as_str())
    );
}

#[tokio::test]
async fn bypass_metadata_and_remote_tag_deletion() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    commit(&a, "f.txt", "1\n", "base");
    remote::push_with_options(
        &a,
        &tree(&a),
        "origin",
        None,
        &PushOptions {
            force: false,
            bypass_protection: true,
        },
    )
    .await
    .unwrap();
    assert!(
        server.state().push_calls.last().unwrap().2,
        "bypass metadata sent"
    );

    a.create_tag(&tree(&a), "v1", None, None).unwrap();
    remote::push_tag(&a, &tree(&a), "origin", "v1")
        .await
        .unwrap();
    let tree_id = server.tree_id(USER, "proj").unwrap();
    assert_eq!(server.state().tags.get(&tree_id).map(Vec::len), Some(1));
    remote::delete_remote_tag(&a, &tree(&a), "origin", "v1")
        .await
        .unwrap();
    assert_eq!(server.state().tags.get(&tree_id).map(Vec::len), Some(0));
    let err = remote::delete_remote_tag(&a, &tree(&a), "origin", "v1")
        .await
        .unwrap_err();
    assert!(err.to_string().contains("not found"), "{err}");
}
