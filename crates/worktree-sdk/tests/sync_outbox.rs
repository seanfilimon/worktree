//! One-step sync and the offline outbox (ENGINEERING_PLAN §15.5) against
//! the in-process fake server: `remote::sync` (merge and rebase
//! strategies, conflicts + `sync_continue` / `sync_abort`, `--all-trees`),
//! `remote::push_or_queue` and `remote::flush_outbox` with the fake's
//! offline switch.

mod remote_fake_server;

use std::path::Path;

use remote_fake_server::{FakeServer, TestHome};
use worktree_sdk::config::{RemoteSection, SyncStrategy};
use worktree_sdk::remote::{self, SyncIntegration, SyncOptions, SyncStatus};
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

fn write(repo: &Repository, path: &str, content: &str) {
    std::fs::write(repo.root().join(path), content).unwrap();
}

fn read(repo: &Repository, path: &str) -> String {
    std::fs::read_to_string(repo.root().join(path)).unwrap()
}

fn snap(repo: &Repository, path: &str, content: &str) -> Snapshot {
    write(repo, path, content);
    let opts = SnapshotOptions {
        message: format!("edit {path}"),
        skip_secret_scan: true,
        ..Default::default()
    };
    repo.snapshot(&tree(repo), opts).unwrap()
}

fn tip(repo: &Repository) -> Option<String> {
    repo.branch_tip(&tree(repo), "main").unwrap()
}

fn server_tip(server: &FakeServer) -> Option<String> {
    let id = server.tree_id(USER, "proj")?;
    server.branch_tip(&id, "main")
}

fn set_offline(server: &FakeServer, offline: bool) {
    server.state().offline = offline;
}

/// `a` pushed a first snapshot; `b` is a clone of it.
async fn two_clones(server: &FakeServer) -> (tempfile::TempDir, Repository, Repository) {
    let dir = tempfile::tempdir().unwrap();
    let a = init_repo(&dir.path().join("a"), server);
    snap(&a, "shared.txt", "one\ntwo\nthree\n");
    let first = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(first.status, SyncStatus::Synced, "{first:?}");
    assert!(
        first.fetch.is_none(),
        "tree did not exist on the server yet"
    );
    assert!(first.push.is_some());
    let b = remote::clone(&server.url, USER, "proj", &dir.path().join("b"))
        .await
        .unwrap();
    (dir, a, b)
}

#[tokio::test]
async fn sync_merges_diverged_work_and_pushes() {
    let (_home, server) = setup().await;
    let (_dir, a, b) = two_clones(&server).await;
    snap(&b, "b.txt", "from b\n");
    remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();
    let a_local = snap(&a, "a.txt", "from a\n");

    let report = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.status, SyncStatus::Synced);
    assert_eq!(report.strategy, SyncStrategy::Merge);
    assert_eq!(report.integration, SyncIntegration::Merged);
    assert_eq!(report.old_tip.as_deref(), Some(a_local.id.as_str()));
    let merged = a.get_snapshot(report.new_tip.as_deref().unwrap()).unwrap();
    assert_eq!(merged.parents.len(), 2);
    assert_eq!(merged.message, "Merge origin/main into main");
    assert_eq!(read(&a, "b.txt"), "from b\n");
    assert!(report.push.as_ref().is_some_and(|p| !p.up_to_date));
    assert_eq!(server_tip(&server), tip(&a));

    // b fast-forwards; nothing to push.
    let report = remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.integration, SyncIntegration::FastForward);
    assert!(report.push.is_none());
    assert_eq!(read(&b, "a.txt"), "from a\n");
    let again = remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(again.status, SyncStatus::UpToDate, "{again:?}");
}

#[tokio::test]
async fn sync_rebase_keeps_history_linear() {
    let (_home, server) = setup().await;
    let (_dir, a, b) = two_clones(&server).await;
    let b_tip = snap(&b, "b.txt", "from b\n");
    remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();

    // Configured strategy.
    let mut config = a.config().clone();
    config.set("sync.strategy", "rebase").unwrap();
    let mut a = a;
    a.set_config(config).unwrap();
    snap(&a, "a1.txt", "1\n");
    snap(&a, "a2.txt", "2\n");
    let report = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.strategy, SyncStrategy::Rebase);
    assert_eq!(report.integration, SyncIntegration::Rebased);
    assert_eq!(report.replayed, 2);
    let head = a.get_snapshot(&tip(&a).unwrap()).unwrap();
    assert_eq!(head.parents.len(), 1);
    let parent = a.get_snapshot(&head.parents[0]).unwrap();
    assert_eq!(parent.parents, vec![b_tip.id.clone()], "linear on top of b");
    assert_eq!(server_tip(&server), tip(&a));

    // --merge overrides the configured rebase.
    snap(&b, "b2.txt", "x\n");
    remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();
    snap(&a, "a3.txt", "3\n");
    let opts = SyncOptions {
        strategy: Some(SyncStrategy::Merge),
        ..Default::default()
    };
    let report = remote::sync(&a, &tree(&a), "origin", opts).await.unwrap();
    assert_eq!(report.integration, SyncIntegration::Merged);
}

#[tokio::test]
async fn sync_conflicts_stop_then_continue_pushes() {
    let (_home, server) = setup().await;
    let (_dir, a, b) = two_clones(&server).await;
    snap(&b, "shared.txt", "one\nTWO (b)\nthree\n");
    remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();
    let before = snap(&a, "shared.txt", "one\nTWO (a)\nthree\n");
    let server_before = server_tip(&server);

    let report = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.status, SyncStatus::Conflicts);
    assert_eq!(report.conflicts.len(), 1);
    assert_eq!(report.conflicts[0].path, "shared.txt");
    assert!(report.push.is_none());
    assert!(read(&a, "shared.txt").contains("<<<<<<<"));
    assert_eq!(server_tip(&server), server_before, "nothing pushed");

    // A second sync is refused while the merge is pending.
    let err = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap_err();
    assert!(err.to_string().contains("--continue"), "{err}");
    // Continue refuses while markers remain.
    let err = remote::sync_continue(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::MergeConflict(_)), "{err:?}");

    write(&a, "shared.txt", "one\nTWO (both)\nthree\n");
    let report = remote::sync_continue(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.status, SyncStatus::Synced);
    assert_eq!(report.old_tip.as_deref(), Some(before.id.as_str()));
    assert!(report.push.is_some());
    assert_eq!(server_tip(&server), tip(&a));
    assert!(matches!(
        remote::sync_continue(&a, &tree(&a), "origin", SyncOptions::default()).await,
        Err(SdkError::InvalidArgument(_))
    ));
}

#[tokio::test]
async fn sync_rebase_conflict_abort_and_auto_abort() {
    let (_home, server) = setup().await;
    let (_dir, a, b) = two_clones(&server).await;
    snap(&b, "shared.txt", "one\nTWO (b)\nthree\n");
    remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();
    let mine = snap(&a, "shared.txt", "one\nTWO (a)\nthree\n");
    let rebase = SyncOptions {
        strategy: Some(SyncStrategy::Rebase),
        ..Default::default()
    };
    let report = remote::sync(&a, &tree(&a), "origin", rebase).await.unwrap();
    assert_eq!(report.status, SyncStatus::Conflicts);
    assert_eq!(
        remote::sync_abort(&a, &tree(&a)).unwrap(),
        SyncStrategy::Rebase
    );
    assert_eq!(tip(&a).as_deref(), Some(mine.id.as_str()));
    assert_eq!(read(&a, "shared.txt"), "one\nTWO (a)\nthree\n");
    assert!(remote::sync_abort(&a, &tree(&a)).is_err());

    // Unattended: conflicts are undone right away.
    for strategy in [SyncStrategy::Merge, SyncStrategy::Rebase] {
        let opts = SyncOptions {
            strategy: Some(strategy),
            abort_on_conflict: true,
            ..Default::default()
        };
        let report = remote::sync(&a, &tree(&a), "origin", opts).await.unwrap();
        assert_eq!(report.status, SyncStatus::Aborted, "{strategy}");
        assert_eq!(tip(&a).as_deref(), Some(mine.id.as_str()));
        assert!(a.merge_state(&tree(&a)).unwrap().is_none());
        assert!(a.rewrite_state(&tree(&a)).unwrap().is_none());
        assert_eq!(read(&a, "shared.txt"), "one\nTWO (a)\nthree\n");
    }
}

#[tokio::test]
async fn offline_sync_queues_and_flush_delivers() {
    let (_home, server) = setup().await;
    let (_dir, a, _b) = two_clones(&server).await;
    let pushed = server_tip(&server);

    set_offline(&server, true);
    // Nothing to push: plain offline.
    let report = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.status, SyncStatus::Offline);
    assert!(report.offline_error.is_some());
    assert!(a.outbox(&tree(&a)).unwrap().is_empty());

    let local = snap(&a, "offline.txt", "written offline\n");
    let report = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.status, SyncStatus::Queued);
    assert_eq!(report.integration, SyncIntegration::None);
    let queued = report.queued.expect("queued entry");
    assert_eq!(queued.tip, local.id);
    assert_eq!(server_tip(&server), pushed);
    // --no-push never queues.
    let no_push = SyncOptions {
        no_push: true,
        ..Default::default()
    };
    let report = remote::sync(&a, &tree(&a), "origin", no_push)
        .await
        .unwrap();
    assert_eq!(report.status, SyncStatus::Offline);

    // push --queue while offline updates the same entry.
    let later = snap(&a, "offline.txt", "more offline work\n");
    let out = remote::push_or_queue(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    assert!(out.push.is_none());
    let entry = out.queued.unwrap();
    assert_eq!(entry.id, queued.id);
    assert_eq!(entry.tip, later.id);
    assert_eq!(a.outbox(&tree(&a)).unwrap().len(), 1);

    // Flushing while offline keeps the entry and records the attempt.
    let flush = remote::flush_outbox(&a, &tree(&a)).await.unwrap();
    assert!(flush.offline());
    assert!(flush.delivered.is_empty());
    assert_eq!(flush.remaining, 1);
    let e = &a.outbox(&tree(&a)).unwrap()[0];
    assert_eq!(e.attempts, 1);
    assert!(e.last_error.is_some());

    set_offline(&server, false);
    let flush = remote::flush_outbox(&a, &tree(&a)).await.unwrap();
    assert_eq!(flush.delivered.len(), 1, "{flush:?}");
    assert_eq!(flush.remaining, 0);
    assert_eq!(flush.delivered[0].push.new_tip, later.id);
    assert_eq!(server_tip(&server).as_deref(), Some(later.id.as_str()));
    assert!(a.outbox(&tree(&a)).unwrap().is_empty());

    // Online push_or_queue pushes directly and clears stale entries.
    let next = snap(&a, "c.txt", "c\n");
    set_offline(&server, true);
    remote::push_or_queue(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    set_offline(&server, false);
    let out = remote::push_or_queue(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    assert!(out.push.is_some() && out.queued.is_none());
    assert!(a.outbox(&tree(&a)).unwrap().is_empty());
    assert_eq!(server_tip(&server).as_deref(), Some(next.id.as_str()));
}

#[tokio::test]
async fn a_rejected_queued_push_stays_with_its_error() {
    let (_home, server) = setup().await;
    let (_dir, a, b) = two_clones(&server).await;
    set_offline(&server, true);
    snap(&a, "a.txt", "a\n");
    remote::push_or_queue(&a, &tree(&a), "origin", None)
        .await
        .unwrap();
    set_offline(&server, false);
    // Someone else pushed meanwhile: the queued push is now behind.
    snap(&b, "b.txt", "b\n");
    remote::sync(&b, &tree(&b), "origin", SyncOptions::default())
        .await
        .unwrap();
    let flush = remote::flush_outbox(&a, &tree(&a)).await.unwrap();
    assert_eq!(flush.failed.len(), 1);
    assert!(!flush.offline());
    assert!(flush.failed[0].error.contains("wt pull"), "{flush:?}");
    assert_eq!(flush.remaining, 1);
    // A full sync integrates, pushes and clears the entry.
    let report = remote::sync(&a, &tree(&a), "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(report.integration, SyncIntegration::Merged);
    assert!(a.outbox(&tree(&a)).unwrap().is_empty());
    assert_eq!(server_tip(&server), tip(&a));
}

#[tokio::test]
async fn sync_all_reports_every_tree() {
    let (_home, server) = setup().await;
    let (_dir, a, _b) = two_clones(&server).await;
    snap(&a, "x.txt", "x\n");
    let all = remote::sync_all(&a, "origin", SyncOptions::default())
        .await
        .unwrap();
    assert_eq!(all.len(), 1);
    let r = all[0].report.as_ref().unwrap();
    assert_eq!(r.tree, tree(&a));
    assert!(r.push.is_some());
    assert!(all[0].error.is_none());
    let all = remote::sync_all(&a, "nope", SyncOptions::default())
        .await
        .unwrap();
    assert!(all[0].error.as_deref().unwrap().contains("nope"), "{all:?}");
}
