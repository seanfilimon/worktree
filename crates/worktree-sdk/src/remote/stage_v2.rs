//! staged-v2 (ENGINEERING_PLAN §16.8): labeled staged slots, their history,
//! push-time overlaps and `wt checkout --staged`.
//!
//! Staging itself stays on gRPC (`StageUploadRequest.label`); removing one
//! slot, the history, the overlaps and the staged file listing use the REST
//! API (`/api/v1/trees/{tenant}/{tree}/staged/...`).

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::credentials::CredentialStore;
use super::rest::{seg, RestClient};
use super::session::{self, Access};
use super::stage::summarize_changes;
use super::{PushReport, StagedInfo};
use crate::error::{Result, SdkError};
use crate::model::{Manifest, Snapshot, SwitchOptions};
use crate::repo::Repository;
use crate::store::ObjectHash;

/// Maximum labeled slots per (account, branch), default slot included.
pub const MAX_LABELS_PER_BRANCH: usize = 10;

/// Paths sent per overlaps request (keeps URLs short) and in total.
const OVERLAP_BATCH: usize = 200;
const MAX_OVERLAP_PATHS: usize = 5000;

/// Accept `""` (the default slot) or `[a-z0-9][a-z0-9_.-]{0,63}`.
pub fn validate_label(label: &str) -> Result<()> {
    let mut chars = label.chars();
    let ok = match chars.next() {
        None => true,
        Some(first) => {
            label.len() <= 64
                && (first.is_ascii_lowercase() || first.is_ascii_digit())
                && chars.all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
                })
        }
    };
    if ok {
        Ok(())
    } else {
        Err(SdkError::InvalidArgument(format!(
            "invalid label {label:?}: use 1-64 characters a-z, 0-9, '_', '.' or '-', starting with a letter or digit"
        )))
    }
}

/// Which staged snapshots [`unstage_slot`] removes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnstageScope {
    /// One slot of a branch (`label` "" = the default slot).
    Slot { branch: String, label: String },
    /// Every label of a branch.
    Branch(String),
    /// Every staged snapshot of the caller in the tree.
    All,
}

/// An ended staged snapshot (`GET {tree}/staged/history`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedHistoryEntry {
    pub id: String,
    pub snapshot_id: String,
    pub author: String,
    #[serde(rename = "branch_name")]
    pub branch: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub files_changed: Vec<String>,
    #[serde(default)]
    pub message: String,
    pub staged_at: DateTime<Utc>,
    /// "pushed", "cleared" or "expired".
    pub status: String,
    pub ended_at: DateTime<Utc>,
    pub ended_by: String,
}

/// Another account's live staged snapshot touching pushed paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedOverlap {
    pub author: String,
    pub branch: String,
    #[serde(default)]
    pub label: String,
    pub snapshot_id: String,
    #[serde(default)]
    pub message: String,
    pub staged_at: DateTime<Utc>,
    pub paths: Vec<String>,
}

/// One file of a staged working manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedFile {
    pub path: String,
    pub hash: String,
    pub size: u64,
    #[serde(default)]
    pub executable: bool,
}

/// The files of a staged snapshot (`GET {tree}/staged/{id}/files`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagedFiles {
    pub files: Vec<StagedFile>,
    /// Files left out because the caller may not read them.
    pub restricted_count: usize,
}

/// Options of [`checkout_staged`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckoutStaged {
    /// Author (username) of the staged snapshot.
    pub user: String,
    /// Slot ("" = the default slot).
    pub label: String,
    /// Branch the work was staged on (default: wherever `user/label` is
    /// staged, if unambiguous).
    pub branch: Option<String>,
    /// Local branch to create (default `staged/USER[/LABEL]`).
    pub new_branch: Option<String>,
    /// Discard uncommitted local changes when switching.
    pub force: bool,
}

/// Result of [`checkout_staged`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutStagedReport {
    /// The new local branch (now checked out).
    pub branch: String,
    /// The local snapshot created from the staged working state.
    pub snapshot_id: String,
    /// Its parent (the staged branch's tip, when known locally).
    pub parent: Option<String>,
    pub author: String,
    pub label: String,
    pub source_branch: String,
    pub staged_snapshot_id: String,
    pub staged_at: DateTime<Utc>,
    pub files: usize,
    pub objects_downloaded: usize,
}

/// A REST client for the server of `remote` plus the tree's API base path.
struct TreeRest {
    client: RestClient,
    base: String,
}

fn tree_rest(repo: &Repository, tree: &str, remote: &str, login: bool) -> Result<TreeRest> {
    let (server, tenant) = session::remote_target(repo, remote)?;
    let info = repo.tree(tree)?;
    let name = if info.remote_name.is_empty() {
        info.name
    } else {
        info.remote_name
    };
    let cred = CredentialStore::default_location().get(&server)?;
    if login && cred.is_none() {
        return Err(SdkError::NotLoggedIn(server));
    }
    Ok(TreeRest {
        client: RestClient::new(&server, cred.map(|c| c.token))?,
        base: format!("/api/v1/trees/{}/{}", seg(&tenant), seg(&name)),
    })
}

/// A staged entry as the REST API encodes it (`domain.StagedSnapshot`).
#[derive(Debug, Deserialize)]
struct RestStaged {
    snapshot_id: String,
    author: String,
    branch_name: String,
    #[serde(default)]
    label: String,
    #[serde(default)]
    files_changed: Vec<String>,
    #[serde(default)]
    files_added: u32,
    #[serde(default)]
    files_modified: u32,
    #[serde(default)]
    files_deleted: u32,
    #[serde(default)]
    message: String,
    staged_at: DateTime<Utc>,
    #[serde(default)]
    manifest_hash: String,
}

impl From<RestStaged> for StagedInfo {
    fn from(s: RestStaged) -> Self {
        StagedInfo {
            snapshot_id: s.snapshot_id,
            author: s.author,
            branch: s.branch_name,
            files_changed: s.files_changed,
            files_added: s.files_added,
            files_modified: s.files_modified,
            files_deleted: s.files_deleted,
            message: Some(s.message).filter(|m| !m.is_empty()),
            staged_at: s.staged_at,
            label: s.label,
            manifest_hash: Some(s.manifest_hash).filter(|h| !h.is_empty()),
        }
    }
}

/// Remove the caller's staged snapshots of `tree` on `remote` selected by
/// `scope`; returns the removed entries (empty when nothing matched).
pub async fn unstage_slot(
    repo: &Repository,
    tree: &str,
    remote: &str,
    scope: &UnstageScope,
) -> Result<Vec<StagedInfo>> {
    #[derive(Deserialize)]
    struct Removed {
        #[serde(default)]
        removed: Vec<RestStaged>,
    }
    let rest = tree_rest(repo, tree, remote, true)?;
    let (path, query) = match scope {
        UnstageScope::Slot { branch, label } => {
            validate_label(label)?;
            // `label=` must be sent even when empty (the default slot).
            (
                format!("{}/staged?label={}", rest.base, seg(label)),
                vec![("branch", branch.clone())],
            )
        }
        UnstageScope::Branch(branch) => (
            format!("{}/staged", rest.base),
            vec![("branch", branch.clone())],
        ),
        UnstageScope::All => (format!("{}/staged", rest.base), vec![]),
    };
    let resp: Removed = rest.client.delete_json(&path, &query).await?;
    Ok(resp.removed.into_iter().map(StagedInfo::from).collect())
}

/// The tree's staged history (ended entries, newest first, 30 days).
pub async fn staged_history(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<StagedHistoryEntry>> {
    #[derive(Deserialize)]
    struct History {
        #[serde(default)]
        history: Vec<StagedHistoryEntry>,
    }
    let rest = tree_rest(repo, tree, remote, false)?;
    let mut query = vec![("branch", branch.unwrap_or_default().to_string())];
    if let Some(n) = limit {
        query.push(("limit", n.to_string()));
    }
    let resp: History = rest
        .client
        .get_query(&format!("{}/staged/history", rest.base), &query)
        .await?;
    Ok(resp.history)
}

/// The files of the live staged snapshot `snapshot_id` (paths the caller
/// may not read are left out and counted).
pub async fn staged_files(
    repo: &Repository,
    tree: &str,
    remote: &str,
    snapshot_id: &str,
) -> Result<StagedFiles> {
    let rest = tree_rest(repo, tree, remote, false)?;
    rest.client
        .get(&format!("{}/staged/{}/files", rest.base, seg(snapshot_id)))
        .await
}

/// Other accounts' live staged snapshots whose changed files include some of
/// `paths` (requires a login).
pub async fn staged_overlaps(
    repo: &Repository,
    tree: &str,
    remote: &str,
    paths: &[String],
) -> Result<Vec<StagedOverlap>> {
    #[derive(Deserialize)]
    struct Overlaps {
        #[serde(default)]
        overlaps: Vec<StagedOverlap>,
    }
    let rest = tree_rest(repo, tree, remote, true)?;
    let mut merged: Vec<StagedOverlap> = Vec::new();
    let paths = &paths[..paths.len().min(MAX_OVERLAP_PATHS)];
    for batch in paths.chunks(OVERLAP_BATCH) {
        let query: Vec<(&str, String)> = batch.iter().map(|p| ("path", p.clone())).collect();
        let resp: Overlaps = rest
            .client
            .get_query(&format!("{}/staged/overlaps", rest.base), &query)
            .await?;
        for ov in resp.overlaps {
            match merged
                .iter_mut()
                .find(|m| m.author == ov.author && m.branch == ov.branch && m.label == ov.label)
            {
                Some(m) => m.paths.extend(ov.paths),
                None => merged.push(ov),
            }
        }
    }
    for m in &mut merged {
        m.paths.sort();
        m.paths.dedup();
    }
    Ok(merged)
}

/// Push-time advisory: the staged work of other people touching the paths
/// `report` pushed. Never fails (an unreachable server, a server without
/// staged-v2 or an anonymous remote yield an empty list): it must not block
/// or fail a push.
pub async fn push_overlaps(
    repo: &Repository,
    tree: &str,
    remote: &str,
    report: &PushReport,
) -> Vec<StagedOverlap> {
    if report.up_to_date {
        return Vec::new();
    }
    let Ok(paths) = pushed_paths(repo, report) else {
        return Vec::new();
    };
    if paths.is_empty() {
        return Vec::new();
    }
    staged_overlaps(repo, tree, remote, &paths)
        .await
        .unwrap_or_default()
}

/// Paths changed between the old remote tip (or, for a new branch, the
/// first parent of the new tip) and the new tip.
fn pushed_paths(repo: &Repository, report: &PushReport) -> Result<Vec<String>> {
    let new = repo.get_snapshot(&report.new_tip)?;
    let base_id = report
        .old_tip
        .clone()
        .or_else(|| new.parents.first().cloned());
    let base = match base_id.filter(|id| repo.has_snapshot(id)) {
        Some(id) => repo.read_manifest(&repo.get_snapshot(&id)?.manifest)?,
        None => Manifest::default(),
    };
    let working = repo.read_manifest(&new.manifest)?;
    Ok(summarize_changes(&base, &working).files_changed)
}

/// Default local branch name of `wt checkout --staged USER[/LABEL]`.
pub fn staged_branch_name(user: &str, label: &str) -> String {
    if label.is_empty() {
        format!("staged/{user}")
    } else {
        format!("staged/{user}/{label}")
    }
}

/// `wt checkout --staged USER[/LABEL]`: download a teammate's staged working
/// state, record it as a local snapshot (author = the teammate, parent = the
/// staged branch's tip when known locally) on a new local branch and switch
/// to it. Nothing is pushed or changed on the server.
pub async fn checkout_staged(
    repo: &Repository,
    tree: &str,
    remote: &str,
    opts: &CheckoutStaged,
) -> Result<CheckoutStagedReport> {
    validate_label(&opts.label)?;
    if opts.user.trim().is_empty() {
        return Err(SdkError::InvalidArgument(
            "a user is required (USER[/LABEL])".into(),
        ));
    }
    let tree_name = repo.tree(tree)?.name;
    // Check the working tree first so that a refused switch leaves no
    // half-created branch behind.
    let status = repo.status(&tree_name)?;
    if status.merge.is_some() {
        return Err(SdkError::MergeInProgress);
    }
    if !opts.force && !status.is_clean() {
        return Err(SdkError::DirtyWorkingTree(status.changes.len()));
    }
    let new_branch = opts
        .new_branch
        .clone()
        .unwrap_or_else(|| staged_branch_name(&opts.user, &opts.label));
    if repo
        .branches(&tree_name, true)?
        .iter()
        .any(|b| b.name == new_branch)
    {
        return Err(SdkError::BranchExists(new_branch));
    }
    let mut session = session::open(repo, &tree_name, remote, Access::Read).await?;
    let staged = session
        .client
        .list_staged(&session.tree.id, opts.branch.as_deref())
        .await?;
    let slot = describe_slot(&opts.user, &opts.label);
    let matches: Vec<&StagedInfo> = staged
        .iter()
        .filter(|s| s.author == opts.user && s.label == opts.label)
        .collect();
    let entry = match matches.as_slice() {
        [] => {
            let scope = opts
                .branch
                .as_deref()
                .map(|b| format!(" on branch {b}"))
                .unwrap_or_default();
            return Err(SdkError::InvalidArgument(format!(
                "{slot} has nothing staged{scope} (see `wt stage list`)"
            )));
        }
        [one] => *one,
        many => {
            let branches: Vec<&str> = many.iter().map(|s| s.branch.as_str()).collect();
            return Err(SdkError::InvalidArgument(format!(
                "{slot} has staged work on several branches ({}); pass --branch",
                branches.join(", ")
            )));
        }
    };
    let manifest_hex = entry.manifest_hash.as_deref().ok_or_else(|| {
        SdkError::InvalidArgument(format!(
            "the staged snapshot of {slot} has no uploaded working state to check out"
        ))
    })?;
    let manifest_hash = ObjectHash::from_hex(manifest_hex).map_err(|e| {
        SdkError::Remote(format!("invalid staged manifest hash {manifest_hex}: {e}"))
    })?;

    // Download the manifest, then its missing blobs / chunk lists, then chunks.
    let store = repo.store();
    let server_tree = session.tree.id.clone();
    let mut downloaded = 0;
    if !store.has(&manifest_hash) {
        downloaded += session
            .client
            .download_objects(store, &server_tree, &[manifest_hash])
            .await?;
    }
    if !store.has(&manifest_hash) {
        return Err(SdkError::Remote(format!(
            "the server did not provide the staged working state of {slot}"
        )));
    }
    let manifest = repo.read_manifest(&manifest_hash)?;
    let wanted: BTreeSet<ObjectHash> = manifest
        .entries
        .iter()
        .map(|e| e.hash)
        .filter(|h| !store.has(h))
        .collect();
    if !wanted.is_empty() {
        let wanted: Vec<ObjectHash> = wanted.into_iter().collect();
        downloaded += session
            .client
            .download_objects(store, &server_tree, &wanted)
            .await?;
    }
    let mut chunks = BTreeSet::new();
    for e in manifest.entries.iter().filter(|e| e.chunked) {
        if let Ok(list) = store.read_chunk_list_local(&e.hash) {
            chunks.extend(list.chunks.iter().map(|c| c.hash).filter(|h| !store.has(h)));
        }
    }
    if !chunks.is_empty() {
        let chunks: Vec<ObjectHash> = chunks.into_iter().collect();
        downloaded += session
            .client
            .download_objects(store, &server_tree, &chunks)
            .await?;
    }
    if let Some(missing) = manifest.entries.iter().find(|e| !store.has(&e.hash)) {
        return Err(SdkError::Remote(format!(
            "the content of {} in the staged snapshot of {slot} is not available on the server",
            missing.path
        )));
    }

    // Parent: the staged branch's tip when we have it locally.
    let server_tip = session
        .client
        .list_branches(&server_tree)
        .await?
        .into_iter()
        .find(|b| b.name == entry.branch)
        .and_then(|b| b.tip);
    let parent = match server_tip.filter(|t| repo.has_snapshot(t)) {
        Some(tip) => Some(tip),
        None => repo
            .remote_tip(&tree_name, remote, &entry.branch)?
            .filter(|t| repo.has_snapshot(t)),
    };

    let info = repo.tree(&tree_name)?;
    let staged_msg = entry.message.clone().unwrap_or_default();
    let mut message = format!("Staged work of {slot} on {}", entry.branch);
    if !staged_msg.is_empty() {
        message.push_str(": ");
        message.push_str(&staged_msg);
    }
    let snapshot = Snapshot {
        id: uuid::Uuid::new_v4().to_string(),
        tree_id: info.id,
        branch: new_branch.clone(),
        parents: parent.iter().cloned().collect(),
        manifest: manifest_hash,
        author: entry.author.clone(),
        message,
        created_at: entry.staged_at,
        auto_generated: false,
        dependencies: Vec::new(),
        reverts: None,
        signature: None,
    };
    repo.write_snapshot(&snapshot)?;
    repo.create_branch(&tree_name, &new_branch, Some(&snapshot.id))?;
    repo.switch_branch(
        &tree_name,
        &new_branch,
        SwitchOptions {
            create: false,
            force: opts.force,
        },
    )?;
    Ok(CheckoutStagedReport {
        branch: new_branch,
        snapshot_id: snapshot.id,
        parent,
        author: entry.author.clone(),
        label: entry.label.clone(),
        source_branch: entry.branch.clone(),
        staged_snapshot_id: entry.snapshot_id.clone(),
        staged_at: entry.staged_at,
        files: manifest.entries.len(),
        objects_downloaded: downloaded,
    })
}

/// "alice" or "alice/label".
fn describe_slot(user: &str, label: &str) -> String {
    if label.is_empty() {
        user.to_string()
    } else {
        format!("{user}/{label}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        for ok in ["", "a", "0", "feature-x", "v1.2_rc", &"a".repeat(64)] {
            assert!(validate_label(ok).is_ok(), "{ok}");
        }
        for bad in ["A", "-x", ".x", "_x", "a b", "a/b", &"a".repeat(65), "é"] {
            assert!(validate_label(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn branch_names() {
        assert_eq!(staged_branch_name("alice", ""), "staged/alice");
        assert_eq!(staged_branch_name("alice", "exp"), "staged/alice/exp");
        assert_eq!(describe_slot("bob", ""), "bob");
        assert_eq!(describe_slot("bob", "x"), "bob/x");
    }

    #[test]
    fn rest_staged_converts() {
        let s: RestStaged = serde_json::from_str(
            r#"{"snapshot_id":"s","author":"a","branch_name":"main","label":"x",
                "files_changed":["f"],"files_added":1,"message":"","staged_at":"2026-01-02T03:04:05Z",
                "manifest_hash":""}"#,
        )
        .unwrap();
        let info = StagedInfo::from(s);
        assert_eq!(info.label, "x");
        assert_eq!(info.message, None);
        assert_eq!(info.manifest_hash, None);
        assert_eq!(info.files_added, 1);
    }
}
