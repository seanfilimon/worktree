//! v0.5 chunked large files, shallow sync and stubbed (restricted) paths
//! (ENGINEERING_PLAN §13.2) through `worktree_sdk::remote` against the
//! in-process fake server.

mod remote_fake_server;

use std::path::Path;

use prost::Message;
use remote_fake_server::{FakeServer, TestHome};
use worktree_protocol::proto::sync as wire;
use worktree_sdk::config::RemoteSection;
use worktree_sdk::remote::{self, FetchOptions};
use worktree_sdk::{
    ChangeKind, DiffOptions, InitOptions, Repository, SdkError, Snapshot, SnapshotOptions,
    SwitchOptions,
};

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

fn write(repo: &Repository, path: &str, content: &[u8]) {
    let full = repo.root().join(path);
    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
    std::fs::write(full, content).unwrap();
}

fn snap(repo: &Repository, message: &str) -> Snapshot {
    let opts = SnapshotOptions {
        message: message.into(),
        skip_secret_scan: true,
        ..Default::default()
    };
    repo.snapshot(&tree(repo), opts).unwrap()
}

/// ~12 MiB: four copies of one 3 MiB pseudo-random block (dedupes well).
fn big_content(seed: u64) -> Vec<u8> {
    let mut state = seed | 1;
    let mut block = Vec::with_capacity(3 * 1024 * 1024);
    while block.len() < 3 * 1024 * 1024 {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        block.extend_from_slice(&state.to_le_bytes());
    }
    let mut data = Vec::with_capacity(block.len() * 4 + 16);
    for _ in 0..4 {
        data.extend_from_slice(&block);
    }
    data.extend_from_slice(b"the end\n");
    data
}

#[tokio::test]
async fn chunked_large_file_round_trips_through_push_and_clone() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    let big = big_content(42);
    write(&a, "small.txt", b"hello\n");
    write(&a, "assets/big.bin", &big);
    let s1 = snap(&a, "add big file");

    let manifest = a.manifest_of(&s1).unwrap();
    let entry = manifest.get("assets/big.bin").unwrap().clone();
    assert!(entry.chunked, "files over 8 MiB are chunked");
    assert_eq!(entry.size, big.len() as u64);
    assert!(!manifest.get("small.txt").unwrap().chunked);
    let list = a.store().read_chunk_list(&entry.hash).unwrap();
    let distinct: std::collections::HashSet<_> = list.chunks.iter().map(|c| c.hash).collect();
    assert!(
        distinct.len() < list.chunks.len(),
        "repeated content dedupes"
    );
    assert_eq!(a.store().read_entry(&entry).unwrap(), big);
    assert!(a.status(&tree(&a)).unwrap().is_clean());
    assert_eq!(a.read_file_at(&s1, "assets/big.bin").unwrap(), big);

    // Edit near the end: only nearby chunks change; diff sees a modification.
    let mut edited = big.clone();
    let n = edited.len();
    edited[n - 100] ^= 0xFF;
    write(&a, "assets/big.bin", &edited);
    let status = a.status(&tree(&a)).unwrap();
    assert_eq!(status.changes.len(), 1);
    assert_eq!(status.changes[0].kind, ChangeKind::Modified);
    let diffs = a
        .diff_working(&tree(&a), None, &DiffOptions::default())
        .unwrap();
    assert_eq!(diffs.len(), 1);
    assert_eq!(diffs[0].path, "assets/big.bin");
    let s2 = snap(&a, "edit big file");
    let e2 = a
        .manifest_of(&s2)
        .unwrap()
        .get("assets/big.bin")
        .unwrap()
        .clone();
    let list2 = a.store().read_chunk_list(&e2.hash).unwrap();
    let shared = list2
        .chunks
        .iter()
        .filter(|c| distinct.contains(&c.hash))
        .count();
    assert!(shared >= list2.chunks.len() - 2, "edit reuses most chunks");
    let revs = a
        .diff_revs(&tree(&a), &s1.id, &s2.id, &DiffOptions::default())
        .unwrap();
    assert_eq!(revs.len(), 1);
    assert!(revs[0].binary);

    remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    {
        let st = server.state();
        let list_bytes = st.objects.get(e2.hash.as_bytes().as_slice()).unwrap();
        let wire_list = wire::ChunkList::decode(list_bytes.as_slice()).unwrap();
        assert_eq!(wire_list.size, edited.len() as u64);
        for c in &wire_list.chunks {
            assert!(st.objects.contains_key(&c.hash.as_ref().unwrap().value));
        }
    }

    // Clone downloads chunk lists + chunks (scoped to the tree) and
    // reassembles the file on checkout.
    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone(&server.url, USER, "proj", b_dir.path())
        .await
        .unwrap();
    assert_eq!(
        std::fs::read(b.root().join("assets/big.bin")).unwrap(),
        edited
    );
    assert!(b.status(&tree(&b)).unwrap().is_clean());
    let tree_id = server.tree_id(USER, "proj").unwrap();
    {
        let st = server.state();
        assert!(!st.download_tree_ids.is_empty());
        assert!(st.download_tree_ids.iter().all(|t| *t == tree_id));
    }
    // Older snapshot checks out too.
    b.checkout(&tree(&b), &s1.id, false).unwrap();
    assert_eq!(std::fs::read(b.root().join("assets/big.bin")).unwrap(), big);
}

#[tokio::test]
async fn push_needs_every_chunk_of_a_large_file() {
    let (_home, server) = setup().await;
    let dir = tempfile::tempdir().unwrap();
    let repo = init_repo(dir.path(), &server);
    write(&repo, "big.bin", &big_content(7));
    let s = snap(&repo, "big");
    let entry = repo.manifest_of(&s).unwrap().entries[0].clone();
    let list = repo.store().read_chunk_list(&entry.hash).unwrap();
    // Simulate a client that "forgot" one chunk: pre-seed everything else.
    for c in list.chunks.iter().skip(1) {
        server.put_object(&repo.store().get(&c.hash).unwrap());
    }
    std::fs::remove_file(repo.store().path_of(&list.chunks[0].hash)).unwrap();
    let err = remote::push(&repo, &tree(&repo), "origin", None)
        .await
        .unwrap_err();
    assert!(
        matches!(err, SdkError::ObjectNotFound(_) | SdkError::Remote(_)),
        "{err:?}"
    );
    assert!(server
        .branch_tip(&server.tree_id(USER, "proj").unwrap(), "main")
        .is_none());
}

#[tokio::test]
async fn shallow_clone_fetch_deepen_and_merge_base_error() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    let t = tree(&a);
    let mut ids = Vec::new();
    for i in 0..5 {
        write(&a, "f.txt", format!("v{i}\n").as_bytes());
        ids.push(snap(&a, &format!("s{i}")).id);
    }
    // feature forks from s0.
    a.create_branch(&t, "feature", Some(&ids[0])).unwrap();
    a.switch_branch(&t, "feature", SwitchOptions::default())
        .unwrap();
    write(&a, "g.txt", b"feature\n");
    let x = snap(&a, "x").id;
    remote::push(&a, &t, "origin", Some("main")).await.unwrap();
    remote::push(&a, &t, "origin", Some("feature"))
        .await
        .unwrap();

    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone_with_options(
        &server.url,
        USER,
        "proj",
        b_dir.path(),
        FetchOptions {
            depth: Some(2),
            deepen: None,
        },
    )
    .await
    .unwrap();
    let bt = tree(&b);
    let log = b.log(&bt, None, None).unwrap();
    assert_eq!(
        log.iter().map(|s| s.id.clone()).collect::<Vec<_>>(),
        vec![ids[4].clone(), ids[3].clone()]
    );
    let boundary = b.shallow_boundary(&bt).unwrap();
    assert!(boundary.contains(&ids[3]), "{boundary:?}");
    assert!(b.is_shallow(&bt).unwrap());
    assert_eq!(
        std::fs::read_to_string(b.root().join("f.txt")).unwrap(),
        "v4\n"
    );

    // main and feature only meet below the boundary.
    let err = b.merge_base(&ids[4], &x).unwrap_err();
    assert!(err.to_string().contains("--deepen"), "{err}");

    // Deepen by two: s1 and s2 arrive (and feature's history reaches s0).
    remote::fetch_with_options(
        &b,
        &bt,
        "origin",
        FetchOptions {
            depth: None,
            deepen: Some(2),
        },
    )
    .await
    .unwrap();
    let log = b.log(&bt, None, None).unwrap();
    assert!(log.len() >= 4, "{log:?}");
    assert!(b.has_snapshot(&ids[1]));
    assert!(!b.shallow_boundary(&bt).unwrap().contains(&ids[3]));

    // Deepen until complete: the boundary disappears and merge-base works.
    remote::fetch_with_options(
        &b,
        &bt,
        "origin",
        FetchOptions {
            depth: None,
            deepen: Some(10),
        },
    )
    .await
    .unwrap();
    assert!(!b.is_shallow(&bt).unwrap());
    assert_eq!(b.merge_base(&ids[4], &x).unwrap(), Some(ids[0].clone()));

    // A depth-limited fetch of new work leaves a gap -> new boundary.
    a.switch_branch(&t, "main", SwitchOptions::default())
        .unwrap();
    let mut newer = Vec::new();
    for i in 5..9 {
        write(&a, "f.txt", format!("v{i}\n").as_bytes());
        newer.push(snap(&a, &format!("s{i}")).id);
    }
    remote::push(&a, &t, "origin", Some("main")).await.unwrap();
    remote::pull_with_options(
        &b,
        &bt,
        "origin",
        None,
        FetchOptions {
            depth: Some(1),
            deepen: None,
        },
    )
    .await
    .unwrap_err(); // diverged history cannot be fast-forwarded past the gap
    assert!(b.has_snapshot(&newer[3]));
    assert!(!b.has_snapshot(&newer[2]));
    assert_eq!(
        b.shallow_boundary(&bt)
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec![newer[3].clone()]
    );
}

#[tokio::test]
async fn stubbed_paths_are_not_checked_out_and_survive_snapshots() {
    let (_home, server) = setup().await;
    let a_dir = tempfile::tempdir().unwrap();
    let a = init_repo(a_dir.path(), &server);
    write(&a, "open.txt", b"public\n");
    write(&a, "vendor/gpl.c", b"int restricted;\n");
    snap(&a, "first");
    remote::push(&a, &tree(&a), "origin", None).await.unwrap();
    let tree_id = server.tree_id(USER, "proj").unwrap();
    server.set_stubbed(&tree_id, &["vendor/gpl.c"]);

    let b_dir = tempfile::tempdir().unwrap();
    let b = remote::clone(&server.url, USER, "proj", b_dir.path())
        .await
        .unwrap();
    let bt = tree(&b);
    assert!(b.root().join("open.txt").is_file());
    assert!(
        !b.root().join("vendor/gpl.c").exists(),
        "stubbed file not written"
    );
    assert_eq!(
        b.stubbed_paths(&bt)
            .unwrap()
            .into_iter()
            .collect::<Vec<_>>(),
        vec!["vendor/gpl.c".to_string()]
    );
    let stubbed_file = b.wt_dir().join("trees").join(&bt).join("stubbed");
    assert_eq!(
        std::fs::read_to_string(stubbed_file).unwrap(),
        "vendor/gpl.c\n"
    );
    assert_eq!(b.restricted_paths(&bt).unwrap(), vec!["vendor/gpl.c"]);
    assert!(
        b.status(&bt).unwrap().is_clean(),
        "restricted is not deleted"
    );

    // Work on top keeps the restricted entry and pushes fine.
    write(&b, "open.txt", b"public v2\n");
    let s = snap(&b, "edit");
    assert!(b.manifest_of(&s).unwrap().get("vendor/gpl.c").is_some());
    remote::push(&b, &bt, "origin", None).await.unwrap();

    // The server refuses the withheld blob even when asked directly.
    let token = remote::load_credential(&server.url).unwrap().unwrap().token;
    let mut client = remote::SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();
    let hidden = a
        .manifest_of(&a.log(&tree(&a), None, Some(1)).unwrap()[0])
        .unwrap()
        .get("vendor/gpl.c")
        .unwrap()
        .hash;
    let dst = tempfile::tempdir().unwrap();
    let store = worktree_sdk::ObjectStore::open(dst.path());
    assert_eq!(
        client
            .download_objects(&store, &tree_id, &[hidden])
            .await
            .unwrap(),
        0
    );
}
