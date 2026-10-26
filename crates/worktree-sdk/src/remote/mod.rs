//! Client for the W0rkTree server: gRPC `worktree.sync.v1.SyncService`
//! (h2c, same base URL as REST) plus the REST auth endpoints.
//!
//! Credentials live in `$WT_HOME/credentials.toml`, keyed by server URL.
//!
//! Submodules:
//!
//! - [`credentials`] — the credential file ([`CredentialStore`])
//! - [`client`] — typed gRPC client ([`SyncClient`]) with batched,
//!   streaming object transfer
//! - [`access`] — declarative access documents: plan, apply, pull (§13.7)
//! - `auth` — REST login (including the MFA second step) and register
//! - `push`, `fetch` (fetch + pull), `clone`, `stage` — the sync flows of
//!   ENGINEERING_PLAN §2, built on the `Repository` public API
//!
//! Wire conventions: `BranchId.value` carries the branch name; snapshot
//! records are sent under the server's tree id, and a local tree is linked
//! to (takes the id of) its server tree on first contact.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::repo::Repository;

pub mod access;
mod auth;
pub mod client;
mod clone;
mod convert;
pub mod credentials;
mod fetch;
mod push;
pub mod rest;
mod server_url;
mod session;
mod stage;
// daemon workstream (§13.11): WatchBranches + Unstage.
mod watch;
pub use watch::{unstage, watch_trees, BranchEvent, BranchEventStream, TreeWatch};
// sparse (§15.4): the lazy object fetcher of partial clones.
pub(crate) use fetch::lazy_fetcher;
// history-v2 (§16.10): force push with lease, protection bypass, remote tag deletion.
mod force;
pub use force::{delete_remote_tag, push_with_options, PushOptions};
// v0.7 trust (§15.8): push one (signed) tag, list server tags.
mod tags;
pub use tags::{push_tag, remote_tags};
// sync workstream (§15.5): one-step sync and the offline outbox.
mod sync_all;
pub use sync_all::{
    flush_outbox, is_unreachable, push_or_queue, sync, sync_abort, sync_all, sync_continue,
    DeliveredPush, FailedPush, FlushReport, PushOrQueued, SyncIntegration, SyncOptions, SyncReport,
    SyncStatus, TreeSync,
};

pub use client::{
    ObjectKind, RemoteBranch, RemoteTree, SyncClient, MAX_MESSAGE_SIZE, TREE_ID_METADATA_KEY,
};
pub use clone::ORIGIN;
pub use credentials::CredentialStore;
pub use push::rejection_message;
pub use rest::RestClient;
pub use server_url::normalize_server_url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginInfo {
    pub server: String,
    pub username: String,
    pub account_id: String,
    pub tenant: String,
    pub token: String,
}

/// The second step an account with MFA enabled must complete after a
/// password login: a short-lived (5 minute) token to exchange, together
/// with a TOTP or recovery code, via [`login_mfa`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MfaChallenge {
    pub server: String,
    pub username: String,
    pub mfa_token: String,
}

/// Result of the first login step ([`login_start`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginOutcome {
    /// Logged in; the credential is saved.
    LoggedIn(LoginInfo),
    /// The account needs a second factor; nothing is saved yet.
    MfaRequired(MfaChallenge),
}

/// Saved credentials for one server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    pub username: String,
    pub token: String,
}

/// Load the credential for `server` (normalized URL, no trailing slash).
pub fn load_credential(server: &str) -> Result<Option<Credential>> {
    CredentialStore::default_location().get(server)
}

/// Save (or replace) the credential for `server`.
pub fn save_credential(server: &str, cred: &Credential) -> Result<()> {
    CredentialStore::default_location().put(server, cred)
}

/// Forget the credential for `server` (no error if there was none).
pub fn remove_credential(server: &str) -> Result<()> {
    CredentialStore::default_location()
        .remove(server)
        .map(|_| ())
}

/// Username of the saved credential for `server`, if logged in. Any error
/// (bad URL, unreadable file) reads as "not logged in".
pub fn logged_in_username(server: &str) -> Option<String> {
    load_credential(server).ok().flatten().map(|c| c.username)
}

/// POST /api/v1/auth/login; saves the credential on success. Accounts with
/// MFA enabled fail with a `PermissionDenied` error mentioning MFA; use
/// [`login_start`] and [`login_mfa`] for them.
pub async fn login(server: &str, username: &str, password: &str) -> Result<LoginInfo> {
    auth::login(
        &CredentialStore::default_location(),
        server,
        username,
        password,
    )
    .await
}

/// POST /api/v1/auth/login: a session (saved) or an MFA challenge.
pub async fn login_start(server: &str, username: &str, password: &str) -> Result<LoginOutcome> {
    auth::login_start(
        &CredentialStore::default_location(),
        server,
        username,
        password,
    )
    .await
}

/// POST /api/v1/auth/login/mfa with a TOTP or recovery code; saves the
/// credential on success.
pub async fn login_mfa(challenge: &MfaChallenge, code: &str) -> Result<LoginInfo> {
    auth::login_mfa(&CredentialStore::default_location(), challenge, code).await
}

/// POST /api/v1/auth/register; saves the credential on success.
pub async fn register(
    server: &str,
    username: &str,
    email: &str,
    password: &str,
) -> Result<LoginInfo> {
    auth::register(
        &CredentialStore::default_location(),
        server,
        username,
        email,
        password,
    )
    .await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingInfo {
    pub server_version: String,
    pub username: Option<String>,
    pub tenant: Option<String>,
}

/// gRPC Ping with the saved credential (if any).
pub async fn ping(server: &str) -> Result<PingInfo> {
    SyncClient::connect_with_store(&CredentialStore::default_location(), server)
        .await?
        .ping()
        .await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushReport {
    pub tree: String,
    pub branch: String,
    pub old_tip: Option<String>,
    pub new_tip: String,
    pub snapshots_sent: usize,
    pub objects_sent: usize,
    pub up_to_date: bool,
    /// history-v2 (§16.10): the push rewound the server branch (`--force`).
    #[serde(default)]
    pub forced: bool,
}

/// Push `branch` (default current) of `tree` to `remote`: ResolveTree
/// (create_if_missing), Pull-style negotiation of missing snapshots,
/// UploadObjects for missing manifests/blobs, then Push with old_tip set to
/// the last known remote tip. Maps PushRejection to `PushRejected`. Updates
/// `refs/remotes/<remote>/<branch>`. Also pushes local tags not on the server.
pub async fn push(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<PushReport> {
    push::push(repo, tree, remote, branch).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchReport {
    pub tree: String,
    /// (branch, old remote tip, new remote tip) for every changed branch.
    pub updated: Vec<(String, Option<String>, Option<String>)>,
    pub snapshots_received: usize,
    pub objects_received: usize,
    pub tags_received: usize,
}

/// Download all remote branches' new snapshots + objects + tags and update
/// `refs/remotes/<remote>/*`. Never touches local branches or files.
pub async fn fetch(repo: &Repository, tree: &str, remote: &str) -> Result<FetchReport> {
    fetch::fetch(repo, tree, remote, FetchOptions::default()).await
}

/// Shallow-sync options for fetch, pull and clone (ENGINEERING_PLAN §13.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchOptions {
    /// Download at most this many new snapshots per branch
    /// (`PullRequest.depth`); older history is cut off and the oldest
    /// snapshots received become the shallow boundary
    /// (`.wt/trees/<tree>/shallow`).
    pub depth: Option<u32>,
    /// Extend a shallow history by this many snapshots per branch.
    pub deepen: Option<u32>,
}

/// [`fetch`] with shallow-sync options.
pub async fn fetch_with_options(
    repo: &Repository,
    tree: &str,
    remote: &str,
    opts: FetchOptions,
) -> Result<FetchReport> {
    fetch::fetch(repo, tree, remote, opts).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullReport {
    pub fetch: FetchReport,
    pub branch: String,
    pub outcome: crate::model::MergeOutcome,
}

/// Fetch, then integrate `<remote>/<branch>` into the local branch
/// (fast-forward + checkout when possible, else `Repository::merge`).
pub async fn pull(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<PullReport> {
    fetch::pull(repo, tree, remote, branch, FetchOptions::default()).await
}

/// [`pull`] with shallow-sync options for its fetch (`wt pull --depth N`).
pub async fn pull_with_options(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
    opts: FetchOptions,
) -> Result<PullReport> {
    fetch::pull(repo, tree, remote, branch, opts).await
}

/// Clone `<tenant>/<tree_name>` from `server` into `dest` (must not exist or
/// be empty): init, set `[remote.origin]`, set tree id/remote name, fetch,
/// create local branches for every remote branch, checkout default branch.
pub async fn clone(server: &str, tenant: &str, tree_name: &str, dest: &Path) -> Result<Repository> {
    clone::clone(
        server,
        tenant,
        tree_name,
        dest,
        FetchOptions::default(),
        &[],
    )
    .await
}

// --- sparse (ENGINEERING_PLAN §15.4) ---

/// `wt clone --sparse PATTERN...`: [`clone_with_options`] that enables
/// sparse checkout with `patterns` before fetching, so only blobs (and
/// chunks) of sparse paths are downloaded ("partial clone") and only those
/// files are checked out. Other content is downloaded on demand later.
pub async fn clone_sparse(
    server: &str,
    tenant: &str,
    tree_name: &str,
    dest: &Path,
    opts: FetchOptions,
    patterns: &[String],
) -> Result<Repository> {
    clone::clone(server, tenant, tree_name, dest, opts, patterns).await
}

// --- end sparse ---

/// [`clone`] with shallow-sync options (`wt clone --depth N`).
pub async fn clone_with_options(
    server: &str,
    tenant: &str,
    tree_name: &str,
    dest: &Path,
    opts: FetchOptions,
) -> Result<Repository> {
    clone::clone(server, tenant, tree_name, dest, opts, &[]).await
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedInfo {
    pub snapshot_id: String,
    pub author: String,
    pub branch: String,
    pub files_changed: Vec<String>,
    pub files_added: u32,
    pub files_modified: u32,
    pub files_deleted: u32,
    pub message: Option<String>,
    pub staged_at: chrono::DateTime<chrono::Utc>,
}

/// Upload the working state as a staged snapshot: builds a manifest from
/// the working directory, uploads missing objects, sends StageUpload with
/// the change summary vs. head.
///
/// "Head" is the shared branch head (the server tip of the current branch,
/// or the last fetched remote-tracking tip), so the summary covers all
/// unpushed work — local snapshots included. Requires a login; creates the
/// server tree if needed. When the working tree matches the local branch
/// tip, that snapshot's id is staged; otherwise a fresh id names the WIP.
pub async fn stage(repo: &Repository, tree: &str, remote: &str) -> Result<StagedInfo> {
    stage::stage(repo, tree, remote).await
}

/// Team-wide staged snapshots for `tree` (optionally one branch).
pub async fn list_staged(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<Vec<StagedInfo>> {
    stage::list_staged(repo, tree, remote, branch).await
}
