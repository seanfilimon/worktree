//! In-process fake W0rkTree server for `tests/remote_*.rs`.
//!
//! Serves the gRPC `SyncService` and the REST auth endpoints on one
//! `127.0.0.1:0` listener (h2c + HTTP/1.1, like the Go server), backed by
//! in-memory maps. It implements the server-side checks of
//! ENGINEERING_PLAN §2 that clients depend on: bearer auth, BLAKE3
//! verification, "all referenced objects present", "parents known",
//! fast-forward-only pushes (`ConflictDetected`) and old-tip CAS.

// Each test crate uses a different subset of the helpers.
#![allow(dead_code)]
// `tonic::Status` is the error type the generated service trait mandates.
#![allow(clippy::result_large_err)]

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::{Json, Router};
use prost::Message;
use serde_json::{json, Value};
use tokio::sync::oneshot;
use tonic::{Request, Response, Status, Streaming};
use worktree_protocol::proto::sync as wire;
use worktree_protocol::proto::sync::sync_service_server::{SyncService, SyncServiceServer};

pub const VERSION: &str = "fake-1.0";
const MAX_MESSAGE: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct Account {
    pub id: String,
    pub username: String,
    pub email: String,
    pub password: String,
    pub tenant_id: String,
    pub token: String,
}

#[derive(Debug, Clone)]
pub struct Tree {
    pub id: String,
    pub tenant_id: String,
    pub tenant_slug: String,
    pub name: String,
    pub default_branch: String,
}

#[derive(Debug, Clone)]
pub struct Staged {
    pub tree_id: String,
    pub username: String,
    pub info: wire::StagedInfo,
}

/// Everything the fake server stores. Public so tests can seed and inspect it.
#[derive(Debug, Default)]
pub struct FakeState {
    pub accounts: BTreeMap<String, Account>,
    pub trees: Vec<Tree>,
    /// tree id -> branch name -> tip
    pub branches: HashMap<String, BTreeMap<String, Option<String>>>,
    pub snapshots: HashMap<String, wire::SnapshotRecord>,
    pub objects: HashMap<Vec<u8>, Vec<u8>>,
    pub tags: HashMap<String, Vec<wire::TagRecord>>,
    pub staged: Vec<Staged>,
    /// When set, every push is answered with this rejection.
    pub push_rejection: Option<wire::PushRejection>,
    pub negotiate_calls: usize,
    pub upload_calls: usize,
    pub download_calls: usize,
}

impl FakeState {
    fn account_by_token(&self, token: &str) -> Option<&Account> {
        self.accounts.values().find(|a| a.token == token)
    }

    fn tree(&self, id: &str) -> Result<&Tree, Status> {
        self.trees
            .iter()
            .find(|t| t.id == id)
            .ok_or_else(|| Status::not_found(format!("tree {id} not found")))
    }

    fn manifest_hashes(&self, manifest: &[u8]) -> Result<Vec<Vec<u8>>, Status> {
        let data = self
            .objects
            .get(manifest)
            .ok_or_else(|| Status::failed_precondition("manifest object missing"))?;
        let rec = wire::ManifestRecord::decode(data.as_slice())
            .map_err(|e| Status::invalid_argument(format!("manifest does not decode: {e}")))?;
        Ok(rec
            .entries
            .into_iter()
            .filter_map(|e| e.blob_hash.map(|h| h.value))
            .collect())
    }

    /// Snapshot ids reachable from `tip` (inclusive), parents before children.
    fn ancestry(&self, tip: &str, extra: &HashMap<String, wire::SnapshotRecord>) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        // Iterative post-order DFS.
        let mut stack = vec![(tip.to_string(), false)];
        while let Some((id, expanded)) = stack.pop() {
            if expanded {
                out.push(id);
                continue;
            }
            if !seen.insert(id.clone()) {
                continue;
            }
            let rec = self.snapshots.get(&id).or_else(|| extra.get(&id));
            let Some(rec) = rec else { continue };
            stack.push((id, true));
            for parent in rec.parents.iter().rev() {
                if !seen.contains(&parent.value) {
                    stack.push((parent.value.clone(), false));
                }
            }
        }
        out
    }
}

type Shared = Arc<Mutex<FakeState>>;

fn lock(state: &Shared) -> MutexGuard<'_, FakeState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// A running fake server; stops when dropped.
pub struct FakeServer {
    pub url: String,
    pub state: Shared,
    shutdown: Option<oneshot::Sender<()>>,
}

impl FakeServer {
    pub async fn start() -> Self {
        let state: Shared = Arc::default();
        let grpc = SyncServiceServer::new(Svc {
            state: state.clone(),
        })
        .max_decoding_message_size(MAX_MESSAGE)
        .max_encoding_message_size(MAX_MESSAGE);
        let rest = Router::new()
            .route("/api/v1/auth/login", post(rest_login))
            .route("/api/v1/auth/register", post(rest_register))
            .with_state(state.clone());
        let app = tonic::service::Routes::new(grpc)
            .into_axum_router()
            .merge(rest);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake server");
        let addr = listener.local_addr().expect("local addr");
        let (tx, rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            let _ = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = rx.await;
                })
                .await;
        });
        Self {
            url: format!("http://{addr}"),
            state,
            shutdown: Some(tx),
        }
    }

    pub fn state(&self) -> MutexGuard<'_, FakeState> {
        lock(&self.state)
    }

    /// Create an account (personal tenant slug = username); returns its token.
    pub fn add_account(&self, username: &str, password: &str) -> String {
        let mut st = self.state();
        create_account(
            &mut st,
            username,
            &format!("{username}@example.com"),
            password,
        )
        .token
    }

    /// Create a tree directly; returns its id.
    pub fn add_tree(&self, tenant: &str, name: &str) -> String {
        let mut st = self.state();
        let tenant_id = st
            .accounts
            .get(tenant)
            .map(|a| a.tenant_id.clone())
            .unwrap_or_else(new_id);
        create_tree(&mut st, tenant, &tenant_id, name).id
    }

    pub fn tree_id(&self, tenant: &str, name: &str) -> Option<String> {
        self.state()
            .trees
            .iter()
            .find(|t| t.tenant_slug == tenant && t.name == name)
            .map(|t| t.id.clone())
    }

    pub fn put_object(&self, data: &[u8]) -> Vec<u8> {
        let hash = blake3::hash(data).as_bytes().to_vec();
        self.state().objects.insert(hash.clone(), data.to_vec());
        hash
    }

    pub fn put_snapshot(&self, rec: wire::SnapshotRecord) {
        let id = rec.id.as_ref().map(|i| i.value.clone()).unwrap_or_default();
        self.state().snapshots.insert(id, rec);
    }

    pub fn set_branch(&self, tree_id: &str, branch: &str, tip: Option<&str>) {
        self.state()
            .branches
            .entry(tree_id.to_string())
            .or_default()
            .insert(branch.to_string(), tip.map(str::to_string));
    }

    pub fn branch_tip(&self, tree_id: &str, branch: &str) -> Option<String> {
        self.state()
            .branches
            .get(tree_id)
            .and_then(|b| b.get(branch))
            .cloned()
            .flatten()
    }

    pub fn set_push_rejection(&self, rejection: Option<wire::PushRejection>) {
        self.state().push_rejection = rejection;
    }
}

impl Drop for FakeServer {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            let _ = tx.send(());
        }
    }
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn create_account(st: &mut FakeState, username: &str, email: &str, password: &str) -> Account {
    let account = Account {
        id: new_id(),
        username: username.to_string(),
        email: email.to_string(),
        password: password.to_string(),
        tenant_id: new_id(),
        token: format!("wt_{}", uuid::Uuid::new_v4().simple()),
    };
    st.accounts.insert(username.to_string(), account.clone());
    account
}

fn create_tree(st: &mut FakeState, tenant_slug: &str, tenant_id: &str, name: &str) -> Tree {
    let tree = Tree {
        id: new_id(),
        tenant_id: tenant_id.to_string(),
        tenant_slug: tenant_slug.to_string(),
        name: name.to_string(),
        default_branch: "main".to_string(),
    };
    st.trees.push(tree.clone());
    st.branches.entry(tree.id.clone()).or_default();
    tree
}

/// A test hash wrapper.
pub fn hash(bytes: &[u8]) -> wire::ContentHash {
    wire::ContentHash {
        value: bytes.to_vec(),
    }
}

pub fn sid(id: &str) -> wire::SnapshotId {
    wire::SnapshotId {
        value: id.to_string(),
    }
}

/// Protobuf-encoded manifest over `(path, content)` pairs, plus the blob
/// hashes, stored in the fake's object map.
pub fn seed_manifest(server: &FakeServer, files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut entries: Vec<wire::ManifestEntry> = files
        .iter()
        .map(|(path, content)| wire::ManifestEntry {
            path: path.to_string(),
            blob_hash: Some(hash(&server.put_object(content))),
            size: content.len() as u64,
            executable: false,
            chunked: false,
        })
        .collect();
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    server.put_object(&wire::ManifestRecord { entries }.encode_to_vec())
}

pub fn snapshot_record(
    id: &str,
    tree_id: &str,
    parents: &[&str],
    manifest: &[u8],
) -> wire::SnapshotRecord {
    wire::SnapshotRecord {
        id: Some(sid(id)),
        tree_id: Some(wire::TreeId {
            value: tree_id.to_string(),
        }),
        branch_name: "main".into(),
        parents: parents.iter().map(|p| sid(p)).collect(),
        manifest_hash: Some(hash(manifest)),
        author: "seed".into(),
        message: format!("snapshot {id}"),
        created_at: Some(prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        }),
        auto_generated: false,
        ..Default::default()
    }
}

// --------------------------------------------------------------------
// gRPC
// --------------------------------------------------------------------

struct Svc {
    state: Shared,
}

impl Svc {
    /// The caller's account; `Ok(None)` when anonymous, `Unauthenticated` for a bad token.
    fn caller<T>(&self, req: &Request<T>) -> Result<Option<Account>, Status> {
        let Some(value) = req.metadata().get("authorization") else {
            return Ok(None);
        };
        let token = value
            .to_str()
            .ok()
            .and_then(|v| v.strip_prefix("Bearer "))
            .ok_or_else(|| Status::unauthenticated("malformed authorization header"))?;
        lock(&self.state)
            .account_by_token(token)
            .cloned()
            .map(Some)
            .ok_or_else(|| Status::unauthenticated("invalid or expired token"))
    }

    fn require<T>(&self, req: &Request<T>) -> Result<Account, Status> {
        self.caller(req)?
            .ok_or_else(|| Status::unauthenticated("login required"))
    }
}

fn tree_of(id: &Option<wire::TreeId>) -> String {
    id.as_ref().map(|t| t.value.clone()).unwrap_or_default()
}

fn sid_of(id: &Option<wire::SnapshotId>) -> Option<String> {
    id.as_ref()
        .map(|s| s.value.clone())
        .filter(|v| !v.is_empty())
}

fn rejection(reason: wire::push_rejection::Reason) -> wire::PushResponse {
    wire::PushResponse {
        accepted: false,
        rejection_reason: Some(wire::PushRejection {
            reason: Some(reason),
        }),
        new_tip: None,
    }
}

#[tonic::async_trait]
impl SyncService for Svc {
    async fn stage(
        &self,
        req: Request<wire::StageUploadRequest>,
    ) -> Result<Response<wire::StageUploadResponse>, Status> {
        let caller = self.require(&req)?;
        let r = req.into_inner();
        let tree_id = tree_of(&r.tree_id);
        let mut st = lock(&self.state);
        st.tree(&tree_id)?;
        st.staged.retain(|s| {
            !(s.tree_id == tree_id
                && s.username == caller.username
                && s.info.branch_name == r.branch_name)
        });
        let now = chrono::Utc::now();
        st.staged.push(Staged {
            tree_id,
            username: caller.username.clone(),
            info: wire::StagedInfo {
                snapshot_id: r.snapshot_id,
                author: caller.username,
                branch_name: r.branch_name,
                files_changed: r.files_changed,
                files_added: r.files_added,
                files_modified: r.files_modified,
                files_deleted: r.files_deleted,
                message: r.message,
                staged_at: Some(prost_types::Timestamp {
                    seconds: now.timestamp(),
                    nanos: 0,
                }),
            },
        });
        Ok(Response::new(wire::StageUploadResponse {
            accepted: true,
            error: None,
        }))
    }

    async fn push(
        &self,
        req: Request<wire::PushRequest>,
    ) -> Result<Response<wire::PushResponse>, Status> {
        use wire::push_rejection::{ConflictDetected, Reason};
        let caller = self.require(&req)?;
        let r = req.into_inner();
        let tree_id = tree_of(&r.tree_id);
        let mut st = lock(&self.state);
        st.tree(&tree_id)?;
        if let Some(rej) = st.push_rejection.clone() {
            return Ok(Response::new(wire::PushResponse {
                accepted: false,
                rejection_reason: Some(rej),
                new_tip: None,
            }));
        }
        let branch = r.branch_name.clone();
        let new_tip =
            sid_of(&r.new_tip).ok_or_else(|| Status::invalid_argument("new_tip required"))?;
        let current = st
            .branches
            .get(&tree_id)
            .and_then(|b| b.get(&branch))
            .cloned()
            .flatten();

        // Every snapshot's parents known; every referenced object present.
        let mut incoming = HashMap::new();
        for rec in &r.snapshots {
            let id =
                sid_of(&rec.id).ok_or_else(|| Status::invalid_argument("snapshot without id"))?;
            for p in &rec.parents {
                if !st.snapshots.contains_key(&p.value) && !incoming.contains_key(&p.value) {
                    return Err(Status::failed_precondition(format!(
                        "unknown parent {}",
                        p.value
                    )));
                }
            }
            let manifest = rec
                .manifest_hash
                .as_ref()
                .map(|h| h.value.clone())
                .unwrap_or_default();
            for blob in st.manifest_hashes(&manifest)? {
                if !st.objects.contains_key(&blob) {
                    return Err(Status::failed_precondition("blob object missing"));
                }
            }
            incoming.insert(id, rec.clone());
        }
        if !st.snapshots.contains_key(&new_tip) && !incoming.contains_key(&new_tip) {
            return Err(Status::failed_precondition(format!(
                "unknown new_tip {new_tip}"
            )));
        }

        if let Some(cur) = &current {
            if *cur != new_tip && !st.ancestry(&new_tip, &incoming).contains(cur) {
                let reason = Reason::ConflictDetected(ConflictDetected {
                    server_tip: Some(sid(cur)),
                });
                return Ok(Response::new(rejection(reason)));
            }
        }
        if sid_of(&r.old_tip) != current {
            let reason = Reason::ConflictDetected(ConflictDetected {
                server_tip: current.as_deref().map(sid),
            });
            return Ok(Response::new(rejection(reason)));
        }

        for (id, mut rec) in incoming {
            rec.tree_id = Some(wire::TreeId {
                value: tree_id.clone(),
            });
            st.snapshots.insert(id, rec);
        }
        st.branches
            .entry(tree_id.clone())
            .or_default()
            .insert(branch.clone(), Some(new_tip.clone()));
        st.staged.retain(|s| {
            !(s.tree_id == tree_id && s.username == caller.username && s.info.branch_name == branch)
        });
        Ok(Response::new(wire::PushResponse {
            accepted: true,
            rejection_reason: None,
            new_tip: Some(sid(&new_tip)),
        }))
    }

    async fn pull(
        &self,
        req: Request<wire::PullRequest>,
    ) -> Result<Response<wire::PullResponse>, Status> {
        self.caller(&req)?;
        let r = req.into_inner();
        let tree_id = tree_of(&r.tree_id);
        let branch = r.branch_id.map(|b| b.value).unwrap_or_default();
        let st = lock(&self.state);
        st.tree(&tree_id)?;
        let tip = st
            .branches
            .get(&tree_id)
            .and_then(|b| b.get(&branch))
            .cloned()
            .ok_or_else(|| Status::not_found(format!("branch {branch} not found")))?;
        let current = sid_of(&r.current_tip);
        let Some(tip) = tip.filter(|t| Some(t) != current.as_ref()) else {
            return Ok(Response::new(wire::PullResponse::default()));
        };
        let none = HashMap::new();
        let have: HashSet<String> = current
            .map(|c| st.ancestry(&c, &none).into_iter().collect())
            .unwrap_or_default();
        let mut chain: Vec<String> = st
            .ancestry(&tip, &none)
            .into_iter()
            .filter(|id| !have.contains(id))
            .collect();
        if let Some(depth) = r.depth {
            let skip = chain.len().saturating_sub(depth as usize);
            chain.drain(..skip);
        }
        let snapshots: Vec<wire::SnapshotRecord> =
            chain.iter().map(|id| st.snapshots[id].clone()).collect();
        let mut objects = Vec::new();
        let mut seen = HashSet::new();
        for rec in &snapshots {
            let manifest = rec
                .manifest_hash
                .as_ref()
                .map(|h| h.value.clone())
                .unwrap_or_default();
            for h in st.manifest_hashes(&manifest)?.into_iter().chain([manifest]) {
                if seen.insert(h.clone()) {
                    objects.push(hash(&h));
                }
            }
        }
        Ok(Response::new(wire::PullResponse {
            has_updates: true,
            new_tip: Some(sid(&tip)),
            snapshot_chain: chain.iter().map(|id| sid(id)).collect(),
            objects_to_fetch: objects,
            snapshots,
            stubbed_paths: Vec::new(),
        }))
    }

    async fn negotiate(
        &self,
        req: Request<wire::NegotiateRequest>,
    ) -> Result<Response<wire::ObjectTransferPlan>, Status> {
        self.require(&req)?;
        let have = req.into_inner().have.map(|h| h.hashes).unwrap_or_default();
        let mut st = lock(&self.state);
        st.negotiate_calls += 1;
        let missing = have
            .into_iter()
            .filter(|h| !st.objects.contains_key(&h.value))
            .collect();
        Ok(Response::new(wire::ObjectTransferPlan {
            objects_to_send: missing,
            total_size: 0,
        }))
    }

    async fn sync_access_config(
        &self,
        _req: Request<wire::AccessConfigSyncRequest>,
    ) -> Result<Response<wire::AccessConfigSyncResponse>, Status> {
        Err(Status::unimplemented("not in the fake"))
    }

    async fn upload_objects(
        &self,
        req: Request<Streaming<wire::ObjectData>>,
    ) -> Result<Response<wire::UploadObjectsResponse>, Status> {
        self.require(&req)?;
        let mut stream = req.into_inner();
        let mut resp = wire::UploadObjectsResponse::default();
        lock(&self.state).upload_calls += 1;
        while let Some(obj) = stream.message().await? {
            resp.received += 1;
            let h = obj.hash.map(|h| h.value).unwrap_or_default();
            if blake3::hash(&obj.data).as_bytes().as_slice() != h.as_slice() {
                resp.rejected.push(hash(&h));
                continue;
            }
            if lock(&self.state).objects.insert(h, obj.data).is_none() {
                resp.stored += 1;
            }
        }
        Ok(Response::new(resp))
    }

    type DownloadObjectsStream =
        tokio_stream::Iter<std::vec::IntoIter<Result<wire::ObjectData, Status>>>;

    async fn download_objects(
        &self,
        req: Request<wire::WantMessage>,
    ) -> Result<Response<Self::DownloadObjectsStream>, Status> {
        self.caller(&req)?;
        let mut st = lock(&self.state);
        st.download_calls += 1;
        let items: Vec<_> = req
            .into_inner()
            .hashes
            .into_iter()
            .filter_map(|h| {
                st.objects.get(&h.value).map(|data| {
                    Ok(wire::ObjectData {
                        hash: Some(h.clone()),
                        kind: wire::ObjectKind::Unspecified as i32,
                        data: data.clone(),
                    })
                })
            })
            .collect();
        Ok(Response::new(tokio_stream::iter(items)))
    }

    async fn resolve_tree(
        &self,
        req: Request<wire::ResolveTreeRequest>,
    ) -> Result<Response<wire::TreeInfo>, Status> {
        let caller = self.caller(&req)?;
        let r = req.into_inner();
        let mut st = lock(&self.state);
        let existing = st
            .trees
            .iter()
            .find(|t| t.tenant_slug == r.tenant_slug && t.name == r.tree_name)
            .cloned();
        let (tree, created) = match existing {
            Some(t) => (t, false),
            None if !r.create_if_missing => {
                return Err(Status::not_found(format!(
                    "tree {}/{} not found",
                    r.tenant_slug, r.tree_name
                )))
            }
            None => {
                let caller = caller
                    .ok_or_else(|| Status::unauthenticated("login required to create trees"))?;
                if caller.username != r.tenant_slug {
                    return Err(Status::permission_denied(format!(
                        "cannot create trees in tenant {}",
                        r.tenant_slug
                    )));
                }
                (
                    create_tree(&mut st, &r.tenant_slug, &caller.tenant_id, &r.tree_name),
                    true,
                )
            }
        };
        Ok(Response::new(wire::TreeInfo {
            tree_id: Some(wire::TreeId { value: tree.id }),
            tenant_id: Some(wire::TenantId {
                value: tree.tenant_id,
            }),
            tenant_slug: tree.tenant_slug,
            name: tree.name,
            default_branch: tree.default_branch,
            created,
        }))
    }

    async fn list_branches(
        &self,
        req: Request<wire::ListBranchesRequest>,
    ) -> Result<Response<wire::ListBranchesResponse>, Status> {
        self.caller(&req)?;
        let tree_id = tree_of(&req.into_inner().tree_id);
        let st = lock(&self.state);
        st.tree(&tree_id)?;
        let branches = st
            .branches
            .get(&tree_id)
            .map(|b| {
                b.iter()
                    .map(|(name, tip)| wire::BranchInfo {
                        name: name.clone(),
                        tip: tip.as_deref().map(sid),
                        protected: false,
                        updated_at: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(Response::new(wire::ListBranchesResponse { branches }))
    }

    async fn list_staged(
        &self,
        req: Request<wire::ListStagedRequest>,
    ) -> Result<Response<wire::ListStagedResponse>, Status> {
        self.caller(&req)?;
        let r = req.into_inner();
        let tree_id = tree_of(&r.tree_id);
        let st = lock(&self.state);
        st.tree(&tree_id)?;
        let staged = st
            .staged
            .iter()
            .filter(|s| {
                s.tree_id == tree_id
                    && r.branch_name
                        .as_ref()
                        .is_none_or(|b| *b == s.info.branch_name)
            })
            .map(|s| s.info.clone())
            .collect();
        Ok(Response::new(wire::ListStagedResponse { staged }))
    }

    async fn push_tag(
        &self,
        req: Request<wire::PushTagRequest>,
    ) -> Result<Response<wire::PushTagResponse>, Status> {
        self.require(&req)?;
        let r = req.into_inner();
        let tree_id = tree_of(&r.tree_id);
        let tag = r
            .tag
            .ok_or_else(|| Status::invalid_argument("tag required"))?;
        let mut st = lock(&self.state);
        st.tree(&tree_id)?;
        let refuse = |why: &str| {
            Ok(Response::new(wire::PushTagResponse {
                accepted: false,
                error: Some(why.to_string()),
            }))
        };
        if !sid_of(&tag.target).is_some_and(|t| st.snapshots.contains_key(&t)) {
            return refuse("unknown target snapshot");
        }
        let tags = st.tags.entry(tree_id).or_default();
        if tags.iter().any(|t| t.name == tag.name) {
            return refuse("tag already exists");
        }
        tags.push(tag);
        Ok(Response::new(wire::PushTagResponse {
            accepted: true,
            error: None,
        }))
    }

    async fn list_tags(
        &self,
        req: Request<wire::ListTagsRequest>,
    ) -> Result<Response<wire::ListTagsResponse>, Status> {
        self.caller(&req)?;
        let tree_id = tree_of(&req.into_inner().tree_id);
        let st = lock(&self.state);
        st.tree(&tree_id)?;
        Ok(Response::new(wire::ListTagsResponse {
            tags: st.tags.get(&tree_id).cloned().unwrap_or_default(),
        }))
    }

    async fn ping(
        &self,
        req: Request<wire::PingRequest>,
    ) -> Result<Response<wire::PingResponse>, Status> {
        let caller = self.caller(&req)?;
        Ok(Response::new(wire::PingResponse {
            server_version: VERSION.into(),
            account_id: caller.as_ref().map(|a| wire::AccountId {
                value: a.id.clone(),
            }),
            username: caller.as_ref().map(|a| a.username.clone()),
            tenant_slug: caller.map(|a| a.username),
        }))
    }

    type WatchBranchesStream =
        tokio_stream::Iter<std::vec::IntoIter<Result<wire::BranchEvent, Status>>>;

    async fn watch_branches(
        &self,
        req: Request<wire::WatchBranchesRequest>,
    ) -> Result<Response<Self::WatchBranchesStream>, Status> {
        self.require(&req)?;
        Ok(Response::new(tokio_stream::iter(Vec::new())))
    }

    async fn unstage(
        &self,
        req: Request<wire::UnstageRequest>,
    ) -> Result<Response<wire::UnstageResponse>, Status> {
        self.require(&req)?;
        Ok(Response::new(wire::UnstageResponse { removed: false }))
    }

    async fn get_access_config(
        &self,
        req: Request<wire::GetAccessConfigRequest>,
    ) -> Result<Response<wire::GetAccessConfigResponse>, Status> {
        self.require(&req)?;
        Ok(Response::new(wire::GetAccessConfigResponse::default()))
    }
}

// --------------------------------------------------------------------
// REST
// --------------------------------------------------------------------

fn api_error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<Value>) {
    (
        status,
        Json(json!({ "error": { "code": code, "message": message } })),
    )
}

fn auth_body(a: &Account) -> Value {
    json!({
        "token": a.token,
        "account": {
            "id": a.id, "username": a.username, "email": a.email, "display_name": "",
            "tenant_id": a.tenant_id, "is_admin": false, "disabled": false,
            "created_at": "2026-01-01T00:00:00Z"
        }
    })
}

fn field<'a>(body: &'a Value, name: &str) -> &'a str {
    body.get(name).and_then(Value::as_str).unwrap_or_default()
}

async fn rest_login(
    State(state): State<Shared>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let st = lock(&state);
    match st.accounts.get(field(&body, "username")) {
        Some(a) if a.password == field(&body, "password") => (StatusCode::OK, Json(auth_body(a))),
        _ => api_error(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "invalid username or password",
        ),
    }
}

async fn rest_register(
    State(state): State<Shared>,
    Json(body): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let (username, email, password) = (
        field(&body, "username"),
        field(&body, "email"),
        field(&body, "password"),
    );
    if username.is_empty() || password.is_empty() {
        return api_error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "username and password are required",
        );
    }
    let mut st = lock(&state);
    if st.accounts.contains_key(username) {
        return api_error(StatusCode::CONFLICT, "conflict", "username already taken");
    }
    let account = create_account(&mut st, username, email, password);
    (StatusCode::CREATED, Json(auth_body(&account)))
}

// --------------------------------------------------------------------
// Per-user state isolation
// --------------------------------------------------------------------

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Points `WT_HOME` at a fresh temp dir for the lifetime of the value.
/// Holds a process-wide lock: env vars are global, so tests touching
/// `WT_HOME` run one at a time.
pub struct TestHome {
    dir: tempfile::TempDir,
    _guard: MutexGuard<'static, ()>,
}

impl TestHome {
    pub fn new() -> Self {
        let guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().expect("temp WT_HOME");
        std::env::set_var("WT_HOME", dir.path());
        Self { dir, _guard: guard }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn credentials_file(&self) -> PathBuf {
        self.dir.path().join("credentials.toml")
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        std::env::remove_var("WT_HOME");
    }
}
