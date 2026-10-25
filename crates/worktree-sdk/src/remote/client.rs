//! Typed gRPC client for `worktree.sync.v1.SyncService`.
//!
//! [`SyncClient`] wraps the generated tonic client with: endpoint setup
//! (HTTP/2 prior knowledge for `http://`, TLS for `https://`), a bearer-token
//! interceptor, 64 MiB message limits, `tonic::Status` → [`SdkError`]
//! mapping, and bounded, streaming object transfer.

use std::collections::HashSet;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};
use tonic::{Code, Status};
use worktree_protocol::proto::sync as wire;
use worktree_protocol::proto::sync::sync_service_client::SyncServiceClient;

use super::convert::{
    hash_from_wire, opt_snapshot_id, staged_from_wire, tag_from_record, tag_to_record, tree_id,
    wire_hash,
};
use super::credentials::CredentialStore;
use super::server_url::normalize_server_url;
use super::{PingInfo, StagedInfo};
use crate::error::{Result, SdkError};
use crate::model::TagInfo;
use crate::store::{ObjectHash, ObjectStore};

/// gRPC max message size, both directions (ENGINEERING_PLAN §2).
pub const MAX_MESSAGE_SIZE: usize = 64 * 1024 * 1024;
/// Largest object that fits in one `ObjectData` message (leaves room for
/// the hash and protobuf framing).
pub const MAX_OBJECT_SIZE: usize = MAX_MESSAGE_SIZE - 4096;
/// Hashes per `Negotiate` / `DownloadObjects` request.
pub(crate) const HASH_BATCH: usize = 2048;
/// Objects per `UploadObjects` call.
pub(crate) const UPLOAD_BATCH_OBJECTS: usize = 1024;
/// Approximate payload bytes per `UploadObjects` call.
pub(crate) const UPLOAD_BATCH_BYTES: u64 = 128 * 1024 * 1024;
/// Objects buffered between the disk reader and the upload stream.
const UPLOAD_CHANNEL_CAPACITY: usize = 8;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(30);

/// Adds `authorization: Bearer <token>` to every request when a token is set.
#[derive(Debug, Clone)]
pub struct BearerAuth {
    header: Option<MetadataValue<Ascii>>,
}

impl BearerAuth {
    /// `InvalidConfig` if the token cannot be sent as an HTTP header.
    pub fn new(token: Option<&str>) -> Result<Self> {
        let header = token
            .map(|t| {
                format!("Bearer {t}")
                    .parse::<MetadataValue<Ascii>>()
                    .map_err(|_| {
                        SdkError::InvalidConfig(
                            "saved token contains invalid characters; run `wt login` again".into(),
                        )
                    })
            })
            .transpose()?;
        Ok(Self { header })
    }
}

impl Interceptor for BearerAuth {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> std::result::Result<tonic::Request<()>, Status> {
        if let Some(header) = &self.header {
            request
                .metadata_mut()
                .insert("authorization", header.clone());
        }
        Ok(request)
    }
}

/// A tree as resolved on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteTree {
    pub id: String,
    pub tenant_id: String,
    pub tenant_slug: String,
    pub name: String,
    pub default_branch: String,
    /// The tree was created by this `ResolveTree` call.
    pub created: bool,
}

/// A branch as listed by the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteBranch {
    pub name: String,
    pub tip: Option<String>,
    pub protected: bool,
}

/// Kind of an object being uploaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ObjectKind {
    Blob,
    Manifest,
    /// Chunk list of a large file (ENGINEERING_PLAN §13.2).
    ChunkList,
}

impl From<ObjectKind> for wire::ObjectKind {
    fn from(kind: ObjectKind) -> Self {
        match kind {
            ObjectKind::Blob => wire::ObjectKind::Blob,
            ObjectKind::Manifest => wire::ObjectKind::Manifest,
            ObjectKind::ChunkList => wire::ObjectKind::ChunkList,
        }
    }
}

/// gRPC metadata key scoping `DownloadObjects` to one tree.
pub const TREE_ID_METADATA_KEY: &str = "x-wt-tree-id";

/// The objects to upload for one manifest entry: its chunks (blobs) and
/// chunk list, or its blob. A chunked entry whose chunk list is not stored
/// locally (restricted content) contributes only the list hash.
pub(crate) fn entry_upload_objects(
    store: &ObjectStore,
    entry: &crate::model::ManifestEntry,
) -> Result<Vec<(ObjectHash, ObjectKind)>> {
    let objects = store.entry_objects(entry)?;
    if !entry.chunked {
        return Ok(objects.into_iter().map(|h| (h, ObjectKind::Blob)).collect());
    }
    // Chunks first, then the list that references them.
    let mut out: Vec<_> = objects[1..]
        .iter()
        .map(|h| (*h, ObjectKind::Blob))
        .collect();
    out.push((objects[0], ObjectKind::ChunkList));
    Ok(out)
}

type Inner = SyncServiceClient<InterceptedService<Channel, BearerAuth>>;

/// Connected client for one server.
#[derive(Debug, Clone)]
pub struct SyncClient {
    server: String,
    authenticated: bool,
    // pub(super): daemon workstream (§13.11) adds RPCs in remote/watch.rs.
    pub(super) inner: Inner,
}

impl SyncClient {
    /// Connect to `server` (any URL form; it is normalized), authenticating
    /// with `token` when given.
    pub async fn connect(server: &str, token: Option<&str>) -> Result<Self> {
        let server = normalize_server_url(server)?;
        let auth = BearerAuth::new(token)?;
        let mut endpoint = Endpoint::from_shared(server.clone())
            .map_err(|e| SdkError::InvalidArgument(format!("invalid server URL '{server}': {e}")))?
            .connect_timeout(CONNECT_TIMEOUT)
            .tcp_nodelay(true)
            .http2_keep_alive_interval(KEEPALIVE_INTERVAL)
            .http2_adaptive_window(true);
        if server.starts_with("https://") {
            endpoint = endpoint
                .tls_config(ClientTlsConfig::new().with_enabled_roots())
                .map_err(|e| {
                    SdkError::Remote(format!(
                        "TLS setup for {server} failed: {}",
                        error_chain(&e)
                    ))
                })?;
        }
        let channel = endpoint.connect().await.map_err(|e| {
            SdkError::Remote(format!("cannot connect to {server}: {}", error_chain(&e)))
        })?;
        let inner = SyncServiceClient::with_interceptor(channel, auth)
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE);
        Ok(Self {
            server,
            authenticated: token.is_some(),
            inner,
        })
    }

    /// Connect with the credential saved in `store` for `server`, if any.
    pub async fn connect_with_store(store: &CredentialStore, server: &str) -> Result<Self> {
        let cred = store.get(server)?;
        Self::connect(server, cred.as_ref().map(|c| c.token.as_str())).await
    }

    /// Normalized server URL.
    pub fn server(&self) -> &str {
        &self.server
    }

    /// Whether requests carry a bearer token.
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    pub(super) fn status(&self, status: Status) -> SdkError {
        status_to_error(&self.server, &status)
    }

    /// Liveness + identity probe.
    pub async fn ping(&mut self) -> Result<PingInfo> {
        let resp = self
            .inner
            .ping(wire::PingRequest {})
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        Ok(PingInfo {
            server_version: resp.server_version,
            username: resp.username.filter(|u| !u.is_empty()),
            tenant: resp.tenant_slug.filter(|t| !t.is_empty()),
        })
    }

    /// Resolve `<tenant>/<name>`; `TreeNotFound` if it does not exist and
    /// `create_if_missing` is false.
    pub async fn resolve_tree(
        &mut self,
        tenant: &str,
        name: &str,
        create_if_missing: bool,
    ) -> Result<RemoteTree> {
        let req = wire::ResolveTreeRequest {
            tenant_slug: tenant.to_string(),
            tree_name: name.to_string(),
            create_if_missing,
        };
        let info = match self.inner.resolve_tree(req).await {
            Ok(resp) => resp.into_inner(),
            Err(s) if s.code() == Code::NotFound => {
                return Err(SdkError::TreeNotFound(format!("{tenant}/{name}")))
            }
            Err(s) => return Err(self.status(s)),
        };
        let id = info
            .tree_id
            .map(|t| t.value)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                SdkError::Remote(format!("server returned no id for tree {tenant}/{name}"))
            })?;
        Ok(RemoteTree {
            id,
            tenant_id: info.tenant_id.map(|t| t.value).unwrap_or_default(),
            tenant_slug: if info.tenant_slug.is_empty() {
                tenant.to_string()
            } else {
                info.tenant_slug
            },
            name: if info.name.is_empty() {
                name.to_string()
            } else {
                info.name
            },
            default_branch: info.default_branch,
            created: info.created,
        })
    }

    /// Branches of a tree, sorted by name.
    pub async fn list_branches(&mut self, tree: &str) -> Result<Vec<RemoteBranch>> {
        let req = wire::ListBranchesRequest {
            tree_id: Some(tree_id(tree)),
        };
        let resp = self
            .inner
            .list_branches(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        let mut branches: Vec<RemoteBranch> = resp
            .branches
            .into_iter()
            .map(|b| RemoteBranch {
                tip: opt_snapshot_id(b.tip.as_ref()),
                name: b.name,
                protected: b.protected,
            })
            .collect();
        branches.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(branches)
    }

    /// Tags of a tree, sorted by name.
    pub async fn list_tags(&mut self, tree: &str) -> Result<Vec<TagInfo>> {
        let req = wire::ListTagsRequest {
            tree_id: Some(tree_id(tree)),
        };
        let resp = self
            .inner
            .list_tags(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        let mut tags = resp
            .tags
            .iter()
            .map(tag_from_record)
            .collect::<Result<Vec<_>>>()?;
        tags.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(tags)
    }

    /// Create a tag on the server; `Remote` with the server's reason if it
    /// refuses.
    pub async fn push_tag(&mut self, tree: &str, tag: &TagInfo) -> Result<()> {
        let req = wire::PushTagRequest {
            tree_id: Some(tree_id(tree)),
            tag: Some(tag_to_record(tag)),
        };
        let resp = self
            .inner
            .push_tag(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        if resp.accepted {
            Ok(())
        } else {
            Err(SdkError::Remote(format!(
                "tag '{}' rejected: {}",
                tag.name,
                resp.error.unwrap_or_else(|| "no reason given".into())
            )))
        }
    }

    /// Staged (WIP) snapshots of a tree, optionally for one branch.
    pub async fn list_staged(
        &mut self,
        tree: &str,
        branch: Option<&str>,
    ) -> Result<Vec<StagedInfo>> {
        let req = wire::ListStagedRequest {
            tree_id: Some(tree_id(tree)),
            branch_name: branch.map(str::to_string),
        };
        let resp = self
            .inner
            .list_staged(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        Ok(resp.staged.iter().map(staged_from_wire).collect())
    }

    /// Upload a staged snapshot summary.
    pub async fn stage(&mut self, req: wire::StageUploadRequest) -> Result<()> {
        let resp = self
            .inner
            .stage(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        if resp.accepted {
            Ok(())
        } else {
            Err(SdkError::Remote(format!(
                "staged snapshot rejected: {}",
                resp.error.unwrap_or_else(|| "no reason given".into())
            )))
        }
    }

    /// Raw `Pull`.
    pub async fn pull(&mut self, req: wire::PullRequest) -> Result<wire::PullResponse> {
        Ok(self
            .inner
            .pull(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner())
    }

    /// Raw `Push`. A rejected push is an `Ok` response with
    /// `accepted == false`; see [`super::rejection_message`].
    pub async fn push(&mut self, req: wire::PushRequest) -> Result<wire::PushResponse> {
        Ok(self
            .inner
            .push(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner())
    }

    // --- access workstream (§13.7): raw access-config RPCs used by
    // `remote::access` ---

    /// Raw `SyncAccessConfig`. A refused document is an `Ok` response with
    /// `accepted == false` and `validation_errors`.
    pub async fn sync_access_config(
        &mut self,
        req: wire::AccessConfigSyncRequest,
    ) -> Result<wire::AccessConfigSyncResponse> {
        Ok(self
            .inner
            .sync_access_config(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner())
    }

    /// Raw `GetAccessConfig`.
    pub async fn get_access_config(
        &mut self,
        req: wire::GetAccessConfigRequest,
    ) -> Result<wire::GetAccessConfigResponse> {
        Ok(self
            .inner
            .get_access_config(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner())
    }

    // --- end access workstream ---

    /// The subset of `candidates` the server does not have (`Negotiate`,
    /// in batches of bounded size). Order follows `candidates`; duplicates
    /// are removed.
    pub async fn missing_objects(&mut self, candidates: &[ObjectHash]) -> Result<Vec<ObjectHash>> {
        let unique = dedup(candidates);
        let mut missing = Vec::new();
        for chunk in unique.chunks(HASH_BATCH) {
            let req = wire::NegotiateRequest {
                have: Some(wire::HaveMessage {
                    hashes: chunk.iter().map(wire_hash).collect(),
                }),
                want: None,
            };
            let plan = self
                .inner
                .negotiate(req)
                .await
                .map_err(|s| self.status(s))?
                .into_inner();
            let server_missing = plan
                .objects_to_send
                .iter()
                .map(|h| hash_from_wire(Some(h)))
                .collect::<Result<HashSet<_>>>()?;
            missing.extend(chunk.iter().filter(|h| server_missing.contains(h)).copied());
        }
        Ok(missing)
    }

    /// Upload `objects` from `store`, streaming them from disk in bounded
    /// batches. Returns the number of objects sent.
    pub async fn upload_objects(
        &mut self,
        store: &ObjectStore,
        objects: &[(ObjectHash, ObjectKind)],
    ) -> Result<usize> {
        let sized: Vec<_> = objects
            .iter()
            .map(|&(hash, kind)| {
                // Size is only a batching hint; a missing file errors on read.
                let size = std::fs::metadata(store.path_of(&hash))
                    .map(|m| m.len())
                    .unwrap_or(0);
                (hash, kind, size)
            })
            .collect();
        let mut sent = 0;
        for batch in batch_by_size(sized, |o| o.2, UPLOAD_BATCH_OBJECTS, UPLOAD_BATCH_BYTES) {
            let batch: Vec<_> = batch.into_iter().map(|(h, k, _)| (h, k)).collect();
            sent += self.upload_batch(store.clone(), batch).await?;
        }
        Ok(sent)
    }

    async fn upload_batch(
        &mut self,
        store: ObjectStore,
        batch: Vec<(ObjectHash, ObjectKind)>,
    ) -> Result<usize> {
        let expected = batch.len();
        let (tx, rx) = mpsc::channel(UPLOAD_CHANNEL_CAPACITY);
        let producer = tokio::task::spawn_blocking(move || -> Result<()> {
            for (hash, kind) in batch {
                let data = store.get(&hash)?;
                if data.len() > MAX_OBJECT_SIZE {
                    return Err(SdkError::Remote(format!(
                        "object {hash} is {} bytes, larger than the {} MiB transfer limit",
                        data.len(),
                        MAX_MESSAGE_SIZE / (1024 * 1024)
                    )));
                }
                let msg = wire::ObjectData {
                    hash: Some(wire_hash(&hash)),
                    kind: wire::ObjectKind::from(kind) as i32,
                    data,
                };
                if tx.blocking_send(msg).is_err() {
                    // The RPC ended early; its status explains why.
                    break;
                }
            }
            Ok(())
        });
        let rpc = self.inner.upload_objects(ReceiverStream::new(rx)).await;
        producer
            .await
            .map_err(|e| SdkError::Other(format!("object upload task failed: {e}")))??;
        let resp = rpc.map_err(|s| self.status(s))?.into_inner();
        if let Some(first) = resp.rejected.first() {
            let first = hash_from_wire(Some(first))
                .map(|h| h.to_hex())
                .unwrap_or_else(|_| "?".into());
            return Err(SdkError::Remote(format!(
                "server rejected {} uploaded object(s) as corrupt (first: {first})",
                resp.rejected.len()
            )));
        }
        if resp.received as usize != expected {
            return Err(SdkError::Remote(format!(
                "server acknowledged {} of {expected} uploaded objects",
                resp.received
            )));
        }
        Ok(expected)
    }

    /// Download `wanted` into `store` (verifying every hash), in bounded
    /// batches. The request is scoped to the server tree `tree` (metadata
    /// `x-wt-tree-id`, required by the server for non-admins). Objects the
    /// server does not have, or does not let the caller read, are skipped;
    /// callers check completeness. Returns the number of objects stored.
    pub async fn download_objects(
        &mut self,
        store: &ObjectStore,
        tree: &str,
        wanted: &[ObjectHash],
    ) -> Result<usize> {
        let tree_header: MetadataValue<Ascii> = tree.parse().map_err(|_| {
            SdkError::InvalidArgument(format!("tree id {tree:?} cannot be sent as metadata"))
        })?;
        let unique = dedup(wanted);
        let mut received = 0;
        for chunk in unique.chunks(HASH_BATCH) {
            let mut pending: HashSet<ObjectHash> = chunk.iter().copied().collect();
            let mut req = tonic::Request::new(wire::WantMessage {
                hashes: chunk.iter().map(wire_hash).collect(),
            });
            req.metadata_mut()
                .insert(TREE_ID_METADATA_KEY, tree_header.clone());
            let mut stream = self
                .inner
                .download_objects(req)
                .await
                .map_err(|s| self.status(s))?
                .into_inner();
            while let Some(obj) = stream.message().await.map_err(|s| self.status(s))? {
                let hash = hash_from_wire(obj.hash.as_ref())?;
                if !pending.remove(&hash) {
                    tracing::warn!(%hash, "server sent an object that was not requested; ignoring");
                    continue;
                }
                store.put_with_hash(&hash, &obj.data)?;
                received += 1;
            }
        }
        Ok(received)
    }
}

fn dedup(hashes: &[ObjectHash]) -> Vec<ObjectHash> {
    let mut seen = HashSet::with_capacity(hashes.len());
    hashes
        .iter()
        .filter(|h| seen.insert(**h))
        .copied()
        .collect()
}

/// Split `items` (in order) into batches of at most `max_count` items and,
/// where possible, at most `max_bytes` total size. An item larger than
/// `max_bytes` gets a batch of its own.
pub(crate) fn batch_by_size<T>(
    items: Vec<T>,
    size: impl Fn(&T) -> u64,
    max_count: usize,
    max_bytes: u64,
) -> Vec<Vec<T>> {
    let max_count = max_count.max(1);
    let mut batches = Vec::new();
    let mut current = Vec::new();
    let mut current_bytes = 0u64;
    for item in items {
        let item_bytes = size(&item);
        let full = current.len() >= max_count
            || (!current.is_empty() && current_bytes.saturating_add(item_bytes) > max_bytes);
        if full {
            batches.push(std::mem::take(&mut current));
            current_bytes = 0;
        }
        current_bytes = current_bytes.saturating_add(item_bytes);
        current.push(item);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// Map a gRPC status to an [`SdkError`].
pub(crate) fn status_to_error(server: &str, status: &Status) -> SdkError {
    let message = status.message();
    match status.code() {
        Code::Unauthenticated => SdkError::NotLoggedIn(server.to_string()),
        Code::PermissionDenied => SdkError::PermissionDenied(if message.is_empty() {
            "permission denied".into()
        } else {
            message.to_string()
        }),
        Code::Unavailable => SdkError::Remote(format!("server {server} is unavailable: {message}")),
        Code::DeadlineExceeded => {
            SdkError::Remote(format!("request to {server} timed out: {message}"))
        }
        Code::Unimplemented => SdkError::Remote(format!(
            "server {server} does not support this operation: {message}"
        )),
        code => SdkError::Remote(if message.is_empty() {
            format!("{code:?}")
        } else {
            format!("{message} ({code:?})")
        }),
    }
}

/// `err` followed by each of its sources, `": "`-separated (transport
/// errors are otherwise just "transport error").
pub(crate) fn error_chain(err: &(dyn std::error::Error + 'static)) -> String {
    let mut out = err.to_string();
    let mut source = err.source();
    while let Some(s) = source {
        let text = s.to_string();
        if !out.contains(&text) {
            out.push_str(": ");
            out.push_str(&text);
        }
        source = s.source();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_respect_count_limit() {
        let batches = batch_by_size((0..10).collect(), |_| 1, 4, u64::MAX);
        assert_eq!(
            batches,
            vec![vec![0, 1, 2, 3], vec![4, 5, 6, 7], vec![8, 9]]
        );
    }

    #[test]
    fn batches_respect_byte_limit_and_isolate_huge_items() {
        let sizes = vec![40u64, 40, 40, 500, 10, 10];
        let batches = batch_by_size(sizes, |s| *s, 100, 100);
        assert_eq!(
            batches,
            vec![vec![40, 40], vec![40], vec![500], vec![10, 10]]
        );
    }

    #[test]
    fn batches_of_nothing_and_zero_count() {
        assert!(batch_by_size(Vec::<u8>::new(), |_| 1, 10, 10).is_empty());
        // A zero count limit is treated as one per batch, never an infinite loop.
        assert_eq!(
            batch_by_size(vec![1, 2], |_| 0, 0, 10),
            vec![vec![1], vec![2]]
        );
    }

    #[test]
    fn dedup_keeps_first_occurrence_order() {
        let a = crate::store::hash_bytes(b"a");
        let b = crate::store::hash_bytes(b"b");
        assert_eq!(dedup(&[a, b, a, b, a]), vec![a, b]);
    }

    #[test]
    fn status_mapping() {
        let s = "http://localhost:7420";
        assert!(
            matches!(status_to_error(s, &Status::unauthenticated("x")), SdkError::NotLoggedIn(u) if u == s)
        );
        assert!(matches!(
            status_to_error(s, &Status::permission_denied("needs tree:write")),
            SdkError::PermissionDenied(m) if m == "needs tree:write"
        ));
        assert!(
            matches!(status_to_error(s, &Status::permission_denied("")), SdkError::PermissionDenied(m) if m == "permission denied")
        );
        assert!(
            matches!(status_to_error(s, &Status::unavailable("down")), SdkError::Remote(m) if m.contains("unavailable"))
        );
        assert!(
            matches!(status_to_error(s, &Status::not_found("no tree")), SdkError::Remote(m) if m.contains("no tree") && m.contains("NotFound"))
        );
        assert!(
            matches!(status_to_error(s, &Status::internal("")), SdkError::Remote(m) if m == "Internal")
        );
    }

    #[test]
    fn bearer_header_is_added_only_with_token() {
        let mut with = BearerAuth::new(Some("abc")).unwrap();
        let req = with.call(tonic::Request::new(())).unwrap();
        assert_eq!(req.metadata().get("authorization").unwrap(), "Bearer abc");

        let mut without = BearerAuth::new(None).unwrap();
        let req = without.call(tonic::Request::new(())).unwrap();
        assert!(req.metadata().get("authorization").is_none());

        assert!(matches!(
            BearerAuth::new(Some("bad\ntoken")),
            Err(SdkError::InvalidConfig(_))
        ));
    }

    #[test]
    fn object_kind_maps_to_wire() {
        assert_eq!(
            wire::ObjectKind::from(ObjectKind::Blob),
            wire::ObjectKind::Blob
        );
        assert_eq!(
            wire::ObjectKind::from(ObjectKind::Manifest),
            wire::ObjectKind::Manifest
        );
        assert_eq!(
            wire::ObjectKind::from(ObjectKind::ChunkList),
            wire::ObjectKind::ChunkList
        );
    }

    #[test]
    fn chunked_entries_upload_chunks_before_their_list() {
        let dir = tempfile::tempdir().unwrap();
        let store = ObjectStore::open(dir.path());
        let small = b"small".to_vec();
        let (h, chunked) = store.put_file(&small).unwrap();
        let entry = |hash, size, chunked| crate::model::ManifestEntry {
            path: "f".into(),
            hash,
            size,
            executable: false,
            chunked,
        };
        assert_eq!(
            entry_upload_objects(&store, &entry(h, 5, chunked)).unwrap(),
            vec![(h, ObjectKind::Blob)]
        );

        let big: Vec<u8> = (0..9 * 1024 * 1024u32)
            .map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8)
            .collect();
        let (h, chunked) = store.put_file(&big).unwrap();
        assert!(chunked);
        let objects = entry_upload_objects(&store, &entry(h, big.len() as u64, true)).unwrap();
        assert_eq!(objects.last(), Some(&(h, ObjectKind::ChunkList)));
        assert!(objects.len() >= 3);
        assert!(objects[..objects.len() - 1]
            .iter()
            .all(|(_, k)| *k == ObjectKind::Blob));
    }

    #[tokio::test]
    async fn connect_to_closed_port_is_remote_error() {
        let err = SyncClient::connect("http://127.0.0.1:1", None)
            .await
            .unwrap_err();
        assert!(
            matches!(err, SdkError::Remote(ref m) if m.contains("cannot connect")),
            "{err:?}"
        );
    }
}
