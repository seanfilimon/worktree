//! `worktree_sdk::remote` against the in-process fake server, exercising
//! everything that does not need a local `Repository`: REST auth +
//! credentials, Ping, tree resolution, branches, tags, staged snapshots,
//! negotiation, raw Push/Pull and rejection mapping.

mod remote_fake_server;

use remote_fake_server::{seed_manifest, sid, snapshot_record, FakeServer, TestHome, VERSION};
use worktree_protocol::proto::sync as wire;
use worktree_sdk::remote::{self, SyncClient};
use worktree_sdk::{SdkError, TagInfo};

async fn client(server: &FakeServer, token: Option<&str>) -> SyncClient {
    SyncClient::connect(&server.url, token)
        .await
        .expect("connect to fake server")
}

#[tokio::test]
async fn remote_ping_anonymous_and_authenticated() {
    let server = FakeServer::start().await;
    let info = client(&server, None).await.ping().await.unwrap();
    assert_eq!(info.server_version, VERSION);
    assert_eq!(info.username, None);
    assert_eq!(info.tenant, None);

    let token = server.add_account("alice", "pw");
    let mut c = client(&server, Some(&token)).await;
    assert!(c.is_authenticated());
    let info = c.ping().await.unwrap();
    assert_eq!(info.username.as_deref(), Some("alice"));
    assert_eq!(info.tenant.as_deref(), Some("alice"));
}

#[tokio::test]
async fn remote_invalid_token_maps_to_not_logged_in() {
    let server = FakeServer::start().await;
    let mut c = client(&server, Some("bogus")).await;
    match c.ping().await.unwrap_err() {
        SdkError::NotLoggedIn(s) => assert_eq!(s, server.url),
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn remote_server_url_is_normalized() {
    let server = FakeServer::start().await;
    let sloppy = format!("{}/", server.url.trim_start_matches("http://"));
    let c = SyncClient::connect(&sloppy, None).await.unwrap();
    assert_eq!(c.server(), server.url);
}

#[tokio::test]
async fn remote_register_login_ping_logout_with_saved_credentials() {
    let home = TestHome::new();
    let server = FakeServer::start().await;

    let info = remote::register(&server.url, "alice", "alice@example.com", "s3cret")
        .await
        .unwrap();
    assert_eq!(info.server, server.url);
    assert_eq!(info.username, "alice");
    assert_eq!(info.tenant, "alice");
    assert!(!info.account_id.is_empty());
    assert!(home.credentials_file().is_file());
    assert_eq!(
        remote::logged_in_username(&server.url).as_deref(),
        Some("alice")
    );
    let cred = remote::load_credential(&server.url).unwrap().unwrap();
    assert_eq!(cred.token, info.token);

    let ping = remote::ping(&server.url).await.unwrap();
    assert_eq!(ping.username.as_deref(), Some("alice"));

    // Logging in again (any URL spelling) refreshes the same entry.
    let sloppy = format!("{}/", server.url);
    let again = remote::login(&sloppy, "alice", "s3cret").await.unwrap();
    assert_eq!(again.server, server.url);
    assert_eq!(
        remote::load_credential(&server.url).unwrap().unwrap().token,
        again.token
    );

    remote::remove_credential(&server.url).unwrap();
    remote::remove_credential(&server.url).unwrap(); // idempotent
    assert_eq!(remote::logged_in_username(&server.url), None);
    assert_eq!(remote::ping(&server.url).await.unwrap().username, None);
}

#[tokio::test]
async fn remote_login_failures_are_mapped() {
    let _home = TestHome::new();
    let server = FakeServer::start().await;
    server.add_account("bob", "right");

    match remote::login(&server.url, "bob", "wrong")
        .await
        .unwrap_err()
    {
        SdkError::PermissionDenied(m) => assert!(m.contains("invalid username or password"), "{m}"),
        other => panic!("{other:?}"),
    }
    match remote::register(&server.url, "bob", "b@x", "pw")
        .await
        .unwrap_err()
    {
        SdkError::Remote(m) => assert!(m.contains("already taken") && m.contains("409"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert!(
        remote::load_credential(&server.url).unwrap().is_none(),
        "failed logins save nothing"
    );
    assert_eq!(remote::logged_in_username("not a url at all://"), None);
}

#[tokio::test]
async fn remote_resolve_tree_lookup_and_creation() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");

    let mut anon = client(&server, None).await;
    assert!(
        matches!(anon.resolve_tree("alice", "proj", false).await, Err(SdkError::TreeNotFound(t)) if t == "alice/proj")
    );
    assert!(matches!(
        anon.resolve_tree("alice", "proj", true).await,
        Err(SdkError::NotLoggedIn(_))
    ));

    let mut c = client(&server, Some(&token)).await;
    let created = c.resolve_tree("alice", "proj", true).await.unwrap();
    assert!(created.created);
    assert_eq!(created.tenant_slug, "alice");
    assert_eq!(created.name, "proj");
    assert_eq!(created.default_branch, "main");
    assert_eq!(
        server.tree_id("alice", "proj").as_deref(),
        Some(created.id.as_str())
    );

    let again = c.resolve_tree("alice", "proj", true).await.unwrap();
    assert!(!again.created);
    assert_eq!(again.id, created.id);

    assert!(matches!(
        c.resolve_tree("someone-else", "proj", true).await,
        Err(SdkError::PermissionDenied(_))
    ));
}

#[tokio::test]
async fn remote_branches_are_listed_sorted_with_tips() {
    let server = FakeServer::start().await;
    let tree = server.add_tree("alice", "proj");
    server.set_branch(&tree, "zeta", None);
    server.set_branch(&tree, "main", Some("s1"));
    let branches = client(&server, None)
        .await
        .list_branches(&tree)
        .await
        .unwrap();
    let summary: Vec<_> = branches
        .iter()
        .map(|b| (b.name.as_str(), b.tip.as_deref()))
        .collect();
    assert_eq!(summary, vec![("main", Some("s1")), ("zeta", None)]);

    assert!(matches!(
        client(&server, None).await.list_branches("nope").await,
        Err(SdkError::Remote(_))
    ));
}

#[tokio::test]
async fn remote_staged_snapshots_round_trip() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");
    let tree = server.add_tree("alice", "proj");
    let mut c = client(&server, Some(&token)).await;

    assert!(c.list_staged(&tree, None).await.unwrap().is_empty());

    for (branch, files) in [("main", vec!["a.txt", "b.txt"]), ("dev", vec!["c.txt"])] {
        c.stage(wire::StageUploadRequest {
            tree_id: Some(wire::TreeId {
                value: tree.clone(),
            }),
            branch_id: Some(wire::BranchId {
                value: branch.into(),
            }),
            branch_name: branch.into(),
            snapshot_id: Some(sid(&format!("wip-{branch}"))),
            files_changed: files.iter().map(|f| f.to_string()).collect(),
            files_added: files.len() as u32,
            message: Some("wip".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    }

    let all = c.list_staged(&tree, None).await.unwrap();
    assert_eq!(all.len(), 2);
    let main = c.list_staged(&tree, Some("main")).await.unwrap();
    assert_eq!(main.len(), 1);
    let m = &main[0];
    assert_eq!(m.snapshot_id, "wip-main");
    assert_eq!(m.author, "alice");
    assert_eq!(m.branch, "main");
    assert_eq!(m.files_changed, vec!["a.txt", "b.txt"]);
    assert_eq!(m.files_added, 2);
    assert_eq!(m.message.as_deref(), Some("wip"));
    assert!(m.staged_at.timestamp() > 0);

    // Staging requires a login.
    let err = client(&server, None)
        .await
        .stage(wire::StageUploadRequest {
            tree_id: Some(wire::TreeId { value: tree }),
            ..Default::default()
        })
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::NotLoggedIn(_)), "{err:?}");
}

#[tokio::test]
async fn remote_tags_round_trip_and_duplicates_are_rejected() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");
    let tree = server.add_tree("alice", "proj");
    let manifest = seed_manifest(&server, &[("a.txt", b"hello")]);
    server.put_snapshot(snapshot_record("s1", &tree, &[], &manifest));
    let mut c = client(&server, Some(&token)).await;

    let tag = TagInfo {
        name: "v1.0".into(),
        target: "s1".into(),
        message: Some("release".into()),
        tagger: "alice".into(),
        created_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
        signature: None,
    };
    c.push_tag(&tree, &tag).await.unwrap();
    assert_eq!(c.list_tags(&tree).await.unwrap(), vec![tag.clone()]);

    match c.push_tag(&tree, &tag).await.unwrap_err() {
        SdkError::Remote(m) => assert!(m.contains("v1.0") && m.contains("already exists"), "{m}"),
        other => panic!("{other:?}"),
    }
    // v0.7 trust (§15.8): a signed tag keeps its signature over the wire.
    let mut signed = TagInfo {
        name: "v1.1".into(),
        ..tag.clone()
    };
    worktree_sdk::signing::LocalKey::from_seed("k", &[4; 32]).sign_tag(&mut signed);
    c.push_tag(&tree, &signed).await.unwrap();
    let listed = c.list_tags(&tree).await.unwrap();
    assert_eq!(listed[1], signed);
    assert!(listed[1].signature.is_some());

    let dangling = TagInfo {
        name: "v2".into(),
        target: "missing".into(),
        ..tag
    };
    assert!(
        matches!(c.push_tag(&tree, &dangling).await, Err(SdkError::Remote(m)) if m.contains("unknown target"))
    );
}

#[tokio::test]
async fn remote_negotiation_reports_only_missing_objects_in_batches() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");
    let present: Vec<_> = (0..3)
        .map(|i| server.put_object(format!("have {i}").as_bytes()))
        .collect();
    let mut c = client(&server, Some(&token)).await;

    let present_hashes: Vec<_> = present
        .iter()
        .map(|h| worktree_sdk::ObjectHash::from_bytes(h.as_slice().try_into().unwrap()))
        .collect();
    let absent: Vec<_> = (0..5000)
        .map(|i| worktree_sdk::store::hash_bytes(format!("absent {i}").as_bytes()))
        .collect();
    let mut candidates = present_hashes.clone();
    candidates.extend(absent.iter().copied());
    candidates.push(absent[0]); // duplicates are collapsed

    let missing = c.missing_objects(&candidates).await.unwrap();
    assert_eq!(
        missing, absent,
        "order preserved, present and duplicate hashes dropped"
    );
    assert!(
        server.state().negotiate_calls > 1,
        "large candidate lists are split into bounded requests"
    );
    assert!(c.missing_objects(&[]).await.unwrap().is_empty());
}

/// Seed `s1 <- s2` on main and return (tree id, token, manifest hash).
fn seed_history(server: &FakeServer) -> (String, String) {
    let token = server.add_account("alice", "pw");
    let tree = server.add_tree("alice", "proj");
    let m1 = seed_manifest(server, &[("a.txt", b"one")]);
    let m2 = seed_manifest(server, &[("a.txt", b"two"), ("b.txt", b"bee")]);
    server.put_snapshot(snapshot_record("s1", &tree, &[], &m1));
    server.put_snapshot(snapshot_record("s2", &tree, &["s1"], &m2));
    server.set_branch(&tree, "main", Some("s2"));
    (tree, token)
}

fn pull_req(tree: &str, current: Option<&str>) -> wire::PullRequest {
    wire::PullRequest {
        tree_id: Some(wire::TreeId { value: tree.into() }),
        branch_id: Some(wire::BranchId {
            value: "main".into(),
        }),
        current_tip: current.map(sid),
        ..Default::default()
    }
}

#[tokio::test]
async fn remote_raw_pull_returns_missing_history_oldest_first() {
    let server = FakeServer::start().await;
    let (tree, _token) = seed_history(&server);
    let mut c = client(&server, None).await;

    let full = c.pull(pull_req(&tree, None)).await.unwrap();
    assert!(full.has_updates);
    let ids: Vec<_> = full
        .snapshots
        .iter()
        .map(|s| s.id.as_ref().unwrap().value.as_str())
        .collect();
    assert_eq!(ids, vec!["s1", "s2"]);
    assert_eq!(
        full.objects_to_fetch.len(),
        5,
        "2 manifests + 3 distinct blobs"
    );

    let delta = c.pull(pull_req(&tree, Some("s1"))).await.unwrap();
    let ids: Vec<_> = delta
        .snapshots
        .iter()
        .map(|s| s.id.as_ref().unwrap().value.as_str())
        .collect();
    assert_eq!(ids, vec!["s2"]);

    let none = c.pull(pull_req(&tree, Some("s2"))).await.unwrap();
    assert!(!none.has_updates && none.snapshots.is_empty());
}

#[tokio::test]
async fn remote_raw_push_conflicts_and_rejections_map_to_clear_messages() {
    use wire::push_rejection::{BranchProtection, Reason};
    let server = FakeServer::start().await;
    let (tree, token) = seed_history(&server);
    let manifest = seed_manifest(&server, &[("c.txt", b"sea")]);
    let mut c = client(&server, Some(&token)).await;

    // A snapshot built on s1 while the server is already at s2.
    let side = snapshot_record("side", &tree, &["s1"], &manifest);
    let req = wire::PushRequest {
        tree_id: Some(wire::TreeId {
            value: tree.clone(),
        }),
        branch_id: Some(wire::BranchId {
            value: "main".into(),
        }),
        branch_name: "main".into(),
        old_tip: Some(sid("s1")),
        new_tip: Some(sid("side")),
        snapshot_chain: vec![sid("side")],
        snapshots: vec![side.clone()],
        ..Default::default()
    };
    let resp = c.push(req.clone()).await.unwrap();
    assert!(!resp.accepted);
    let msg = remote::rejection_message("origin", "main", resp.rejection_reason.as_ref());
    assert!(
        msg.contains("wt pull") && msg.contains("origin/main"),
        "{msg}"
    );
    assert_eq!(
        server.branch_tip(&tree, "main").as_deref(),
        Some("s2"),
        "rejected push changes nothing"
    );

    // A fast-forward push succeeds.
    let ff = wire::PushRequest {
        old_tip: Some(sid("s2")),
        new_tip: Some(sid("s3")),
        snapshot_chain: vec![sid("s3")],
        snapshots: vec![snapshot_record("s3", &tree, &["s2"], &manifest)],
        ..req.clone()
    };
    assert!(c.push(ff.clone()).await.unwrap().accepted);
    assert_eq!(server.branch_tip(&tree, "main").as_deref(), Some("s3"));

    // Policy rejections come back with their reason.
    server.set_push_rejection(Some(wire::PushRejection {
        reason: Some(Reason::BranchProtection(BranchProtection {
            rule: "main requires a merge request".into(),
        })),
    }));
    let resp = c.push(ff).await.unwrap();
    let msg = remote::rejection_message("origin", "main", resp.rejection_reason.as_ref());
    assert!(
        msg.contains("protected") && msg.contains("main requires a merge request"),
        "{msg}"
    );
    server.set_push_rejection(None);

    // Snapshots referencing objects the server lacks are a protocol error.
    let bogus = snapshot_record("s4", &tree, &["s3"], &[7u8; 32]);
    let err = c
        .push(wire::PushRequest {
            old_tip: Some(sid("s3")),
            new_tip: Some(sid("s4")),
            snapshot_chain: vec![sid("s4")],
            snapshots: vec![bogus],
            ..req
        })
        .await
        .unwrap_err();
    assert!(
        matches!(err, SdkError::Remote(ref m) if m.contains("missing")),
        "{err:?}"
    );

    // Anonymous pushes are refused.
    let err = client(&server, None)
        .await
        .push(wire::PushRequest::default())
        .await
        .unwrap_err();
    assert!(matches!(err, SdkError::NotLoggedIn(_)), "{err:?}");
}

#[tokio::test]
async fn remote_clone_of_unknown_tree_creates_nothing() {
    let _home = TestHome::new();
    let server = FakeServer::start().await;
    let dest = tempfile::tempdir().unwrap();
    let target = dest.path().join("clone");
    let err = remote::clone(&server.url, "alice", "nope", &target)
        .await
        .expect_err("clone must fail");
    assert!(
        matches!(err, SdkError::TreeNotFound(ref t) if t == "alice/nope"),
        "{err:?}"
    );
    assert!(!target.exists());
}

#[tokio::test]
async fn remote_stopped_server_is_unreachable() {
    let server = FakeServer::start().await;
    let url = server.url.clone();
    drop(server);
    // Give the listener a moment to close.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let err = SyncClient::connect(&url, None).await.unwrap_err();
    assert!(
        matches!(err, SdkError::Remote(ref m) if m.contains("cannot connect")),
        "{err:?}"
    );
}
