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

    // Rewrite main locally: move it back to base, commit something else.
    // (`undo` would now undo the push on the server, see undo_push_*.)
    a.set_branch_tip(&tree(&a), "main", Some(&base.id)).unwrap();
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

/// `wt undo` of a push (specs/tree/Undo.md §2.2): pushes are recorded in
/// the oplog, are not undone locally, and `remote::undo_push` restores the
/// server branch with the pushed tip as lease.
#[tokio::test]
async fn undo_push_moves_the_server_branch_back() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    let t = tree(&a);
    let base = commit(&a, "f.txt", "1\n", "base");
    remote::push(&a, &t, "origin", None).await.unwrap();
    let tree_id = server.tree_id(USER, "proj").unwrap();

    // The first push created the branch: nothing to go back to.
    let first = a.push_to_undo(&t).unwrap().expect("push recorded");
    assert_eq!(
        (first.old_tip.as_deref(), first.new_tip.as_str()),
        (None, base.id.as_str())
    );
    let err = remote::plan_push_undo(&a, &first).await.unwrap_err();
    assert!(err.to_string().contains("created origin/main"), "{err}");

    let s2 = commit(&a, "f.txt", "2\n", "second");
    let s3 = commit(&a, "f.txt", "3\n", "third");
    remote::push(&a, &t, "origin", None).await.unwrap();
    let ops = a.oplog(&t).unwrap();
    assert_eq!(ops[0].op, "push");
    assert!(
        ops[0].description.contains("2 snapshots"),
        "{}",
        ops[0].description
    );
    assert_eq!(
        ops[0].changes[0].describe(),
        format!("origin/main: {} -> {}", &base.id[..8], &s3.id[..8])
    );
    // Local undo refuses pushes.
    let err = a.undo(&t, Default::default()).unwrap_err();
    assert!(err.to_string().contains("locally"), "{err}");

    // Someone pushed on top: refused before anything changes.
    let undo = a.push_to_undo(&t).unwrap().unwrap();
    assert_eq!(undo.old_tip.as_deref(), Some(base.id.as_str()));
    server.set_branch(&tree_id, "main", Some(&base.id));
    let err = remote::plan_push_undo(&a, &undo).await.unwrap_err();
    assert!(
        matches!(&err, SdkError::PushRejected(m) if m.contains("has moved since your push") && m.contains("wt revert")),
        "{err:?}"
    );
    server.set_branch(&tree_id, "main", Some(&s3.id));

    // The plan lists the snapshots that leave the server branch.
    let plan = remote::plan_push_undo(&a, &undo).await.unwrap();
    let removed: Vec<&str> = plan.removed.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(removed, vec![s3.id.as_str(), s2.id.as_str()]);
    assert_eq!(plan.restore_to, base.id);
    assert_eq!(plan.server_tree, "alice/proj");

    // Refusals by the server surface with their own variants.
    server.state().restore_refusal = Some((
        403,
        "forbidden".into(),
        "restoring branch 'main' requires branch:force_push".into(),
    ));
    let err = remote::undo_push(&a, &plan, false).await.unwrap_err();
    assert!(
        matches!(&err, SdkError::PermissionDenied(m) if m.contains("branch:force_push")),
        "{err:?}"
    );
    server.state().restore_refusal =
        Some((403, "branch_protection".into(), "no_force_push".into()));
    let err = remote::undo_push(&a, &plan, false).await.unwrap_err();
    assert!(matches!(err, SdkError::BranchProtection(_)), "{err:?}");
    assert!(
        a.push_to_undo(&t).unwrap().is_some(),
        "refused undos are not recorded"
    );
    server.state().restore_refusal = None;
    // A push between the plan and the restore loses the lease.
    server.set_branch(&tree_id, "main", Some(&s2.id));
    let err = remote::undo_push(&a, &plan, false).await.unwrap_err();
    assert!(
        matches!(&err, SdkError::PushRejected(m) if m.contains("cannot undo the push")),
        "{err:?}"
    );
    server.set_branch(&tree_id, "main", Some(&s3.id));

    let report = remote::undo_push(&a, &plan, false).await.unwrap();
    assert_eq!(
        (report.old_tip.as_str(), report.new_tip.as_str()),
        (s3.id.as_str(), base.id.as_str())
    );
    assert_eq!(report.entry.kind, "restore");
    assert_eq!(
        server.branch_tip(&tree_id, "main").as_deref(),
        Some(base.id.as_str())
    );
    let last = server.state().restore_calls.last().cloned().unwrap();
    assert_eq!(last["expected_tip"], s3.id.as_str());
    assert_eq!(last["tip"], base.id.as_str());
    // Local main keeps its snapshots; the remote-tracking ref moved back,
    // the reflog and oplog record the undo, and redo points at `wt push`.
    assert_eq!(a.branch_tip(&t, "main").unwrap(), Some(s3.id.clone()));
    assert_eq!(
        a.remote_tip(&t, "origin", "main").unwrap(),
        Some(base.id.clone())
    );
    assert_eq!(a.reflog(&t, Some(1)).unwrap()[0].action, "undo-push");
    assert!(a.push_to_undo(&t).unwrap().is_none());
    assert_eq!(a.redo_list(&t).unwrap()[0].op, "push");
    let err = a.redo(&t, Default::default()).unwrap_err();
    assert!(err.to_string().contains("wt push"), "{err}");

    // The server reflog is readable from the SDK.
    let entries = remote::remote_reflog(&a, &t, "origin", Some("main"), Some(10))
        .await
        .unwrap();
    assert_eq!(entries[0].kind, "restore");
    assert_eq!(entries[0].old_tip, s3.id);
}
