//! gRPC message compression of object transfers (ENGINEERING_PLAN §14.6):
//! the SDK sends and accepts gzip for UploadObjects / DownloadObjects and
//! falls back to uncompressed messages on a server without gzip.

mod remote_fake_server;

use remote_fake_server::{FakeServer, GrpcEncoding};
use worktree_sdk::remote::client::ObjectKind;
use worktree_sdk::remote::SyncClient;
use worktree_sdk::store::hash_bytes;
use worktree_sdk::ObjectStore;

fn encodings(server: &FakeServer, method: &str) -> Vec<GrpcEncoding> {
    server
        .state()
        .grpc_encodings
        .iter()
        .filter(|e| e.method == method)
        .cloned()
        .collect()
}

/// Two compressible objects in a fresh local store.
fn objects(store: &ObjectStore) -> Vec<(worktree_sdk::ObjectHash, ObjectKind)> {
    let a = "compressible upload content\n".repeat(400);
    let b = "another compressible object\n".repeat(300);
    vec![
        (store.put(a.as_bytes()).unwrap(), ObjectKind::Blob),
        (store.put(b.as_bytes()).unwrap(), ObjectKind::Blob),
    ]
}

#[tokio::test]
async fn object_transfers_negotiate_gzip() {
    let server = FakeServer::start().await;
    let token = server.add_account("alice", "pw");
    let tree = server.add_tree("alice", "app");
    let mut client = SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();
    assert!(client.compresses_objects());

    let dir = tempfile::tempdir().unwrap();
    let local = ObjectStore::open(dir.path().join("a"));
    let objs = objects(&local);
    assert_eq!(client.upload_objects(&local, &objs).await.unwrap(), 2);
    // The server stored the uncompressed content (hashes verified there).
    for (h, _) in &objs {
        let st = server.state();
        let data = st.objects.get(h.as_bytes().as_slice()).expect("uploaded");
        assert_eq!(hash_bytes(data), *h);
    }
    let up = encodings(&server, "UploadObjects");
    assert_eq!(up.len(), 1, "{up:?}");
    assert_eq!(up[0].request.as_deref(), Some("gzip"));
    assert!(up[0].accept.as_deref().unwrap_or("").contains("gzip"));

    let other = ObjectStore::open(dir.path().join("b"));
    let wanted: Vec<_> = objs.iter().map(|(h, _)| *h).collect();
    assert_eq!(
        client
            .download_objects(&other, &tree, &wanted)
            .await
            .unwrap(),
        2
    );
    for (h, _) in &objs {
        assert_eq!(other.get(h).unwrap(), local.get(h).unwrap());
    }
    let down = encodings(&server, "DownloadObjects");
    assert_eq!(down.len(), 1, "{down:?}");
    assert_eq!(down[0].request.as_deref(), Some("gzip"));
    assert_eq!(
        down[0].response.as_deref(),
        Some("gzip"),
        "responses compressed"
    );

    // Other RPCs are left uncompressed.
    client.ping().await.unwrap();
    let ping = encodings(&server, "Ping");
    assert_eq!(ping.last().unwrap().request, None);

    // Compression can be switched off explicitly.
    client.set_object_compression(false);
    client.upload_objects(&local, &objs).await.unwrap();
    assert_eq!(encodings(&server, "UploadObjects")[1].request, None);
}

#[tokio::test]
async fn falls_back_without_server_gzip() {
    let server = FakeServer::start_without_gzip().await;
    let token = server.add_account("alice", "pw");
    let tree = server.add_tree("alice", "app");
    let mut client = SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();

    let dir = tempfile::tempdir().unwrap();
    let local = ObjectStore::open(dir.path().join("a"));
    let objs = objects(&local);
    assert_eq!(client.upload_objects(&local, &objs).await.unwrap(), 2);
    assert!(!client.compresses_objects(), "fell back after the refusal");
    let up = encodings(&server, "UploadObjects");
    let requests: Vec<_> = up.iter().map(|e| e.request.as_deref()).collect();
    assert_eq!(requests, vec![Some("gzip"), None], "{up:?}");
    assert_eq!(server.state().objects.len(), 2);

    // Later transfers on the same client go uncompressed right away.
    let other = ObjectStore::open(dir.path().join("b"));
    let wanted: Vec<_> = objs.iter().map(|(h, _)| *h).collect();
    assert_eq!(
        client
            .download_objects(&other, &tree, &wanted)
            .await
            .unwrap(),
        2
    );
    let down = encodings(&server, "DownloadObjects");
    assert_eq!(down.len(), 1);
    assert_eq!(down[0].request, None);

    // A fresh client falls back on download as well.
    let mut fresh = SyncClient::connect(&server.url, Some(&token))
        .await
        .unwrap();
    let third = ObjectStore::open(dir.path().join("c"));
    assert_eq!(
        fresh
            .download_objects(&third, &tree, &wanted)
            .await
            .unwrap(),
        2
    );
    assert!(!fresh.compresses_objects());
    let down = encodings(&server, "DownloadObjects");
    let requests: Vec<_> = down[1..].iter().map(|e| e.request.as_deref()).collect();
    assert_eq!(requests, vec![Some("gzip"), None], "{down:?}");
}
