//! Client for the W0rkTree server: gRPC `worktree.sync.v1.SyncService`
//! (h2c, same base URL as REST) plus the REST auth endpoints.
//!
//! Credentials live in `$WT_HOME/credentials.toml`, keyed by server URL.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::Result;
use crate::repo::Repository;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginInfo {
    pub server: String,
    pub username: String,
    pub account_id: String,
    pub tenant: String,
    pub token: String,
}

/// Saved credentials for one server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Credential {
    pub username: String,
    pub token: String,
}

/// Load the credential for `server` (normalized URL, no trailing slash).
pub fn load_credential(server: &str) -> Result<Option<Credential>> {
    let _ = server;
    todo!("sdk-remote: load_credential")
}

pub fn save_credential(server: &str, cred: &Credential) -> Result<()> {
    let _ = (server, cred);
    todo!("sdk-remote: save_credential")
}

pub fn remove_credential(server: &str) -> Result<()> {
    let _ = server;
    todo!("sdk-remote: remove_credential")
}

/// POST /api/v1/auth/login; saves the credential on success.
pub async fn login(server: &str, username: &str, password: &str) -> Result<LoginInfo> {
    let _ = (server, username, password);
    todo!("sdk-remote: login")
}

/// POST /api/v1/auth/register; saves the credential on success.
pub async fn register(server: &str, username: &str, email: &str, password: &str) -> Result<LoginInfo> {
    let _ = (server, username, email, password);
    todo!("sdk-remote: register")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PingInfo {
    pub server_version: String,
    pub username: Option<String>,
    pub tenant: Option<String>,
}

/// gRPC Ping with the saved credential (if any).
pub async fn ping(server: &str) -> Result<PingInfo> {
    let _ = server;
    todo!("sdk-remote: ping")
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
}

/// Push `branch` (default current) of `tree` to `remote`: ResolveTree
/// (create_if_missing), Pull-style negotiation of missing snapshots,
/// UploadObjects for missing manifests/blobs, then Push with old_tip set to
/// the last known remote tip. Maps PushRejection to `PushRejected`. Updates
/// `refs/remotes/<remote>/<branch>`. Also pushes local tags not on the server.
pub async fn push(repo: &Repository, tree: &str, remote: &str, branch: Option<&str>) -> Result<PushReport> {
    let _ = (repo, tree, remote, branch);
    todo!("sdk-remote: push")
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
    let _ = (repo, tree, remote);
    todo!("sdk-remote: fetch")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullReport {
    pub fetch: FetchReport,
    pub branch: String,
    pub outcome: crate::model::MergeOutcome,
}

/// Fetch, then integrate `<remote>/<branch>` into the local branch
/// (fast-forward + checkout when possible, else `Repository::merge`).
pub async fn pull(repo: &Repository, tree: &str, remote: &str, branch: Option<&str>) -> Result<PullReport> {
    let _ = (repo, tree, remote, branch);
    todo!("sdk-remote: pull")
}

/// Clone `<tenant>/<tree_name>` from `server` into `dest` (must not exist or
/// be empty): init, set `[remote.origin]`, set tree id/remote name, fetch,
/// create local branches for every remote branch, checkout default branch.
pub async fn clone(server: &str, tenant: &str, tree_name: &str, dest: &Path) -> Result<Repository> {
    let _ = (server, tenant, tree_name, dest);
    todo!("sdk-remote: clone")
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
pub async fn stage(repo: &Repository, tree: &str, remote: &str) -> Result<StagedInfo> {
    let _ = (repo, tree, remote);
    todo!("sdk-remote: stage")
}

/// Team-wide staged snapshots for `tree` (optionally one branch).
pub async fn list_staged(repo: &Repository, tree: &str, remote: &str, branch: Option<&str>) -> Result<Vec<StagedInfo>> {
    let _ = (repo, tree, remote, branch);
    todo!("sdk-remote: list_staged")
}
