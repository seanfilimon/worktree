//! Full push → clone → fetch → pull round trips through
//! `worktree_sdk::remote` against the in-process fake server.
//!
//! These drive real `Repository` instances, so they need the `sdk-core`
//! workstream (repo/store/model) to be implemented.

mod remote_fake_server;

use std::path::Path;

use remote_fake_server::{FakeServer, TestHome};
use worktree_protocol::proto::sync as wire;
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote::{self, ObjectKind, SyncClient};
use worktree_sdk::{
    InitOptions, MergeOutcome, ObjectStore, Repository, SdkError, Snapshot, SnapshotOptions,
};

const USER: &str = "alice";

/// WT_HOME isolation + a fake server + a registered, logged-in user.
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

fn write(repo: &Repository, path: &str, content: &str) {
    let full = repo.root().join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn read(repo: &Repository, path: &str) -> String {
    std::fs::read_to_string(repo.root().join(path)).unwrap()
}

fn snap(repo: &Repository, message: &str) -> Snapshot {
    let opts = SnapshotOptions {
        message: message.into(),
        skip_secret_scan: true,
        ..Default::default()
    };
    repo.snapshot(&tree(repo), opts).unwrap()
}

fn tip(repo: &Repository, branch: &str) -> Option<String> {
    repo.branch_tip(&tree(repo), branch).unwrap()
}

/// A repo with two snapshots on main, pushed to the fake server.
async fn pushed_repo(dir: &Path, server: &FakeServer) -> (Repository, Snapshot) {
    let repo = init_repo(dir, server);
    write(&repo, "a.txt", "one\n");
    write(&repo, "dir/b.txt", "bee\n");
    snap(&repo, "first");
    write(&repo, "a.txt", "two\n");
    let s2 = snap(&repo, "second");
    remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    (repo, s2)
}

async fn clone_to(server: &FakeServer, dir: &Path) -> Repository {
    remote::clone(&server.url, USER, "proj", dir).await.unwrap()
}

#[tokio::test]
async fn remote_push_then_clone_round_trip() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let repo = init_repo(a_dir.path(), &server);
    write(&repo, "a.txt", "one\n");
    write(&repo, "dir/b.txt", "bee\n");
    snap(&repo, "first");
    write(&repo, "a.txt", "two\n");
    let s2 = snap(&repo, "second");

    let report = remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    assert_eq!(report.branch, "main");
    assert_eq!(report.old_tip, None);
    assert_eq!(report.new_tip, s2.id);
    assert_eq!(report.snapshots_sent, 2);
    assert_eq!(report.objects_sent, 5, "3 distinct blobs + 2 manifests");
    assert!(!report.up_to_date);

    let server_tree = server.tree_id(USER, "proj").expect("push creates the tree");
    assert_eq!(
        server.branch_tip(&server_tree, "main").as_deref(),
        Some(s2.id.as_str())
    );
    assert_eq!(
        repo.remote_tip(&tree(&repo), "origin", "main")
            .unwrap()
            .as_deref(),
        Some(s2.id.as_str())
    );
    assert_eq!(
        repo.root_tree().unwrap().id,
        server_tree,
        "local tree is linked to the server tree"
    );
    let reflog = repo.reflog(&tree(&repo), None).unwrap();
    assert_eq!(reflog[0].action, "push");
    assert_eq!(reflog[0].new.as_deref(), Some(s2.id.as_str()));

    let b_dir = tempfile::tempdir().unwrap();
    let dest = b_dir.path().join("clone");
    let cloned = clone_to(&server, &dest).await;
    assert_eq!(read(&cloned, "a.txt"), "two\n");
    assert_eq!(read(&cloned, "dir/b.txt"), "bee\n");
    assert_eq!(tip(&cloned, "main").as_deref(), Some(s2.id.as_str()));
    assert_eq!(cloned.config().remote["origin"].url, server.url);
    assert_eq!(cloned.config().remote["origin"].tenant, USER);
    assert_eq!(cloned.root_tree().unwrap().id, server_tree);
    assert_eq!(cloned.log(&tree(&cloned), None, None).unwrap().len(), 2);
    assert!(cloned.status(&tree(&cloned)).unwrap().is_clean());

    let again = remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap();
    assert!(again.up_to_date);
    assert_eq!((again.snapshots_sent, again.objects_sent), (0, 0));
}

#[tokio::test]
async fn remote_fetch_then_pull_fast_forwards() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let (a, s2) = pushed_repo(a_dir.path(), &server).await;
    let b_dir = tempfile::tempdir().unwrap();
    let b = clone_to(&server, &b_dir.path().join("b")).await;

    write(&a, "a.txt", "three\n");
    write(&a, "c.txt", "sea\n");
    let s3 = snap(&a, "third");
    let report = remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    assert_eq!(report.old_tip.as_deref(), Some(s2.id.as_str()));
    assert_eq!(report.snapshots_sent, 1);
    assert_eq!(
        report.objects_sent, 3,
        "two new blobs + one manifest; dir/b.txt is deduplicated"
    );

    let fetched = remote::fetch(&b, &tree(&b), "origin").await.unwrap();
    assert_eq!(
        fetched.updated,
        vec![("main".to_string(), Some(s2.id.clone()), Some(s3.id.clone()))]
    );
    assert_eq!(fetched.snapshots_received, 1);
    assert_eq!(fetched.objects_received, 3);
    assert_eq!(
        tip(&b, "main").as_deref(),
        Some(s2.id.as_str()),
        "fetch never moves local branches"
    );
    assert_eq!(read(&b, "a.txt"), "two\n", "fetch never touches files");

    let pulled = remote::pull(&b, &tree(&b), "origin", None).await.unwrap();
    assert_eq!(pulled.branch, "main");
    assert!(pulled.fetch.updated.is_empty());
    assert_eq!(
        pulled.outcome,
        MergeOutcome::FastForward { to: s3.id.clone() }
    );
    assert_eq!(tip(&b, "main").as_deref(), Some(s3.id.as_str()));
    assert_eq!(read(&b, "a.txt"), "three\n");
    assert_eq!(read(&b, "c.txt"), "sea\n");
    let reflog = b.reflog(&tree(&b), Some(1)).unwrap();
    assert_eq!(reflog[0].action, "pull");
    assert_eq!(reflog[0].old.as_deref(), Some(s2.id.as_str()));
    assert_eq!(reflog[0].new.as_deref(), Some(s3.id.as_str()));

    let again = remote::pull(&b, &tree(&b), "origin", None).await.unwrap();
    assert_eq!(again.outcome, MergeOutcome::UpToDate);
}

#[tokio::test]
async fn remote_diverged_push_is_rejected_then_pull_merges() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let (a, s2) = pushed_repo(a_dir.path(), &server).await;
    let b_dir = tempfile::tempdir().unwrap();
    let b = clone_to(&server, &b_dir.path().join("b")).await;

    write(&b, "dir/b.txt", "bee from b\n");
    let b_snap = snap(&b, "b's change");
    remote::push(&b, &tree(&b), "origin", None).await.unwrap();

    write(&a, "a.txt", "a's change\n");
    snap(&a, "a's change");
    let uploads_before = server.state().upload_calls;
    let err = remote::push(&a, &tree(&a), "origin", None)
        .await
        .unwrap_err();
    match err {
        SdkError::PushRejected(m) => assert!(m.contains("wt pull"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        server.state().upload_calls,
        uploads_before,
        "nothing uploaded for a doomed push"
    );
    let server_tree = server.tree_id(USER, "proj").unwrap();
    assert_eq!(
        server.branch_tip(&server_tree, "main").as_deref(),
        Some(b_snap.id.as_str())
    );

    let pulled = remote::pull(&a, &tree(&a), "origin", None).await.unwrap();
    let merged = match pulled.outcome {
        MergeOutcome::Merged { snapshot } => snapshot,
        other => panic!("expected a merge, got {other:?}"),
    };
    assert_eq!(merged.parents.len(), 2);
    assert_eq!(read(&a, "a.txt"), "a's change\n");
    assert_eq!(read(&a, "dir/b.txt"), "bee from b\n");
    assert!(repo_has(&a, &s2.id));

    remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    let b_pull = remote::pull(&b, &tree(&b), "origin", None).await.unwrap();
    assert_eq!(
        b_pull.outcome,
        MergeOutcome::FastForward {
            to: merged.id.clone()
        }
    );
    assert_eq!(read(&b, "a.txt"), "a's change\n");
}

fn repo_has(repo: &Repository, id: &str) -> bool {
    repo.has_snapshot(id)
}

#[tokio::test]
async fn remote_pull_refuses_to_overwrite_uncommitted_changes() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let (a, s2) = pushed_repo(a_dir.path(), &server).await;
    let b_dir = tempfile::tempdir().unwrap();
    let b = clone_to(&server, &b_dir.path().join("b")).await;

    write(&a, "a.txt", "upstream\n");
    snap(&a, "upstream");
    remote::push(&a, &tree(&a), "origin", None).await.unwrap();

    write(&b, "a.txt", "local edit\n");
    let err = remote::pull(&b, &tree(&b), "origin", None)
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::DirtyWorkingTree(_)), "{err:?}");
    assert_eq!(tip(&b, "main").as_deref(), Some(s2.id.as_str()));
    assert_eq!(read(&b, "a.txt"), "local edit\n");
}

#[tokio::test]
async fn remote_tags_travel_with_push_clone_and_fetch() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    write(&a, "a.txt", "one\n");
    let s1 = snap(&a, "first");
    a.create_tag(&tree(&a), "v1", None, Some("first release"))
        .unwrap();
    remote::push(&a, &tree(&a), "origin", None).await.unwrap();

    let server_tree = server.tree_id(USER, "proj").unwrap();
    let names: Vec<_> = server.state().tags[&server_tree]
        .iter()
        .map(|t| t.name.clone())
        .collect();
    assert_eq!(names, vec!["v1"]);

    let b_dir = tempfile::tempdir().unwrap();
    let b = clone_to(&server, &b_dir.path().join("b")).await;
    let tags = b.tags(&tree(&b)).unwrap();
    assert_eq!(tags.len(), 1);
    assert_eq!(tags[0].target, s1.id);
    assert_eq!(tags[0].message.as_deref(), Some("first release"));

    // Tags are pushed even when the branch is already up to date.
    a.create_tag(&tree(&a), "v1.1", None, None).unwrap();
    assert!(
        remote::push(&a, &tree(&a), "origin", None)
            .await
            .unwrap()
            .up_to_date
    );
    let fetched = remote::fetch(&b, &tree(&b), "origin").await.unwrap();
    assert_eq!(fetched.tags_received, 1);
    assert!(fetched.updated.is_empty());
}

#[tokio::test]
async fn remote_stage_lists_wip_until_pushed() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let (a, _s2) = pushed_repo(a_dir.path(), &server).await;

    write(&a, "a.txt", "work in progress\n");
    write(&a, "new.txt", "brand new\n");
    std::fs::remove_file(a.root().join("dir/b.txt")).unwrap();
    let staged = remote::stage(&a, &tree(&a), "origin").await.unwrap();
    assert_eq!(staged.branch, "main");
    assert_eq!(staged.author, USER);
    assert_eq!(staged.files_changed, vec!["a.txt", "dir/b.txt", "new.txt"]);
    assert_eq!(
        (
            staged.files_added,
            staged.files_modified,
            staged.files_deleted
        ),
        (1, 1, 1)
    );
    assert_eq!(staged.message, None);

    let listed = remote::list_staged(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].snapshot_id, staged.snapshot_id);
    assert_eq!(listed[0].files_changed, staged.files_changed);
    assert!(remote::list_staged(&a, &tree(&a), "origin", Some("other"))
        .await
        .unwrap()
        .is_empty());

    // After a local snapshot the staged entry names that snapshot and still
    // reports everything not yet pushed.
    let s3 = snap(&a, "wip done");
    let staged = remote::stage(&a, &tree(&a), "origin").await.unwrap();
    assert_eq!(staged.snapshot_id, s3.id);
    assert_eq!(staged.message.as_deref(), Some("wip done"));
    assert_eq!(staged.files_changed.len(), 3);

    remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    assert!(
        remote::list_staged(&a, &tree(&a), "origin", None)
            .await
            .unwrap()
            .is_empty(),
        "push clears staged WIP"
    );
}

#[tokio::test]
async fn remote_server_rejections_are_reported() {
    use wire::push_rejection::{Reason, ReviewRequired};
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let (a, s2) = pushed_repo(a_dir.path(), &server).await;

    server.set_push_rejection(Some(wire::PushRejection {
        reason: Some(Reason::ReviewRequired(ReviewRequired {
            required: 2,
            current: 0,
        })),
    }));
    write(&a, "a.txt", "needs review\n");
    snap(&a, "needs review");
    match remote::push(&a, &tree(&a), "origin", None)
        .await
        .unwrap_err()
    {
        SdkError::PushRejected(m) => assert!(m.contains("2 approval"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        a.remote_tip(&tree(&a), "origin", "main")
            .unwrap()
            .as_deref(),
        Some(s2.id.as_str()),
        "a rejected push does not move the remote-tracking ref"
    );
}

#[tokio::test]
async fn remote_push_preconditions() {
    let (home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path(), &server);

    let err = remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SdkError::InvalidArgument(ref m) if m.contains("no snapshots")),
        "{err:?}"
    );

    write(&repo, "a.txt", "x\n");
    snap(&repo, "first");
    let err = remote::push(&repo, &tree(&repo), "upstream", None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SdkError::RemoteNotConfigured(ref r) if r == "upstream"),
        "{err:?}"
    );

    std::fs::remove_file(home.credentials_file()).unwrap();
    let err = remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SdkError::NotLoggedIn(ref s) if *s == server.url),
        "{err:?}"
    );
    let err = remote::stage(&repo, &tree(&repo), "origin")
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::NotLoggedIn(_)), "{err:?}");
}

#[tokio::test]
async fn remote_objects_stream_up_and_down_in_batches() {
    let (_home, server) = setup().await;
    let token = remote::load_credential(&server.url).unwrap().unwrap().token;
    let mut client = SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();

    let src_dir = tempfile::tempdir().unwrap();
    let src = ObjectStore::open(src_dir.path());
    let objects: Vec<_> = (0..1500)
        .map(|i| {
            (
                src.put(format!("object number {i}").as_bytes()).unwrap(),
                ObjectKind::Blob,
            )
        })
        .collect();
    let hashes: Vec<_> = objects.iter().map(|(h, _)| *h).collect();

    assert_eq!(client.missing_objects(&hashes).await.unwrap().len(), 1500);
    assert_eq!(client.upload_objects(&src, &objects).await.unwrap(), 1500);
    assert!(
        server.state().upload_calls >= 2,
        "uploads are split into bounded batches"
    );
    assert!(client.missing_objects(&hashes).await.unwrap().is_empty());

    let dst_dir = tempfile::tempdir().unwrap();
    let dst = ObjectStore::open(dst_dir.path());
    let mut wanted = hashes.clone();
    wanted.push(worktree_sdk::store::hash_bytes(b"not on the server"));
    let tree_id = server.add_tree(USER, "objects");
    assert_eq!(
        client
            .download_objects(&dst, &tree_id, &wanted)
            .await
            .unwrap(),
        1500
    );
    // hardening (§15.10): the download is spread over parallel streams,
    // each scoped to the tree.
    let ids = server.state().download_tree_ids.clone();
    assert!(
        ids.len() >= 2 && ids.iter().all(|id| *id == tree_id),
        "{ids:?}"
    );
    assert!(hashes.iter().all(|h| dst.has(h)));
    assert_eq!(dst.get(&hashes[7]).unwrap(), b"object number 7");
}
