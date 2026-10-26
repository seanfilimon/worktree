//! staged-v2 (ENGINEERING_PLAN §16.8): labeled slots, `checkout_staged` and
//! the never-failing push advisory, against the in-process fake server.

mod remote_fake_server;

use std::path::Path;

use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote::{self, CheckoutStaged};
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

fn write(repo: &Repository, path: &str, content: &str) {
    let full = repo.root().join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn read(repo: &Repository, path: &str) -> String {
    std::fs::read_to_string(repo.root().join(path)).unwrap()
}

#[tokio::test]
async fn labeled_slots_and_checkout_staged() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    write(&a, "a.txt", "one\n");
    let opts = SnapshotOptions {
        message: "first".into(),
        skip_secret_scan: true,
        ..Default::default()
    };
    let first = a.snapshot(&tree(&a), opts).unwrap();
    remote::push(&a, &tree(&a), "origin", None).await.unwrap();

    // Two slots on main.
    write(&a, "a.txt", "two\n");
    let default = remote::stage(&a, &tree(&a), "origin").await.unwrap();
    assert_eq!(default.label, "");
    write(&a, "exp.txt", "experiment\n");
    let exp = remote::stage_label(&a, &tree(&a), "origin", "exp")
        .await
        .unwrap();
    assert_eq!(exp.label, "exp");
    assert!(exp.manifest_hash.is_some());
    assert!(matches!(
        remote::stage_label(&a, &tree(&a), "origin", "Not Valid").await,
        Err(SdkError::InvalidArgument(_))
    ));
    let mut listed = remote::list_staged(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    listed.sort_by(|x, y| x.label.cmp(&y.label));
    assert_eq!(
        listed.iter().map(|s| s.label.as_str()).collect::<Vec<_>>(),
        vec!["", "exp"]
    );
    assert!(listed.iter().all(|s| s.manifest_hash.is_some()));

    // A second worktree checks alice's "exp" slot out into a new branch.
    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone(&server.url, USER, "proj", b_dir.path())
        .await
        .unwrap();
    let report = remote::checkout_staged(
        &b,
        &tree(&b),
        "origin",
        &CheckoutStaged {
            user: USER.into(),
            label: "exp".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(report.branch, "staged/alice/exp");
    assert_eq!(report.source_branch, "main");
    assert_eq!(report.parent.as_deref(), Some(first.id.as_str()));
    assert_eq!(b.current_branch(&tree(&b)).unwrap(), "staged/alice/exp");
    assert_eq!(read(&b, "a.txt"), "two\n");
    assert_eq!(read(&b, "exp.txt"), "experiment\n");
    let head = b.get_snapshot(&report.snapshot_id).unwrap();
    assert_eq!(head.author, USER);
    assert_eq!(head.parents, vec![first.id.clone()]);

    // The branch name is taken now; a dirty tree is refused; unknown slots too.
    let again = remote::checkout_staged(
        &b,
        &tree(&b),
        "origin",
        &CheckoutStaged {
            user: USER.into(),
            label: "exp".into(),
            ..Default::default()
        },
    )
    .await;
    assert!(matches!(again, Err(SdkError::BranchExists(_))), "{again:?}");
    write(&b, "dirty.txt", "x\n");
    let dirty = remote::checkout_staged(
        &b,
        &tree(&b),
        "origin",
        &CheckoutStaged {
            user: USER.into(),
            new_branch: Some("try".into()),
            ..Default::default()
        },
    )
    .await;
    assert!(
        matches!(dirty, Err(SdkError::DirtyWorkingTree(_))),
        "{dirty:?}"
    );
    std::fs::remove_file(b.root().join("dirty.txt")).unwrap();
    let missing = remote::checkout_staged(
        &b,
        &tree(&b),
        "origin",
        &CheckoutStaged {
            user: "nobody".into(),
            new_branch: Some("try".into()),
            ..Default::default()
        },
    )
    .await;
    assert!(
        matches!(missing, Err(SdkError::InvalidArgument(ref m)) if m.contains("nothing staged")),
        "{missing:?}"
    );

    // The push advisory never fails, even against a server without the
    // REST endpoint.
    write(&a, "a.txt", "three\n");
    let opts = SnapshotOptions {
        message: "three".into(),
        skip_secret_scan: true,
        ..Default::default()
    };
    a.snapshot(&tree(&a), opts).unwrap();
    let report = remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    assert!(remote::push_overlaps(&a, &tree(&a), "origin", &report)
        .await
        .is_empty());
}
