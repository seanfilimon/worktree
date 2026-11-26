//! The server reflog (specs/tree/Undo.md §4): every branch move the server
//! accepted (pushes, force pushes, restores), kept 90 days.
//!
//! - [`RestClient::server_reflog`] reads it (`GET {tree}/reflog`;
//!   `wt reflog --remote`);
//! - [`RestClient::restore_branch`] moves a branch back to a tip of its
//!   reflog (`POST {tree}/reflog/restore`, needs `branch:force_push`), with
//!   lease semantics: the branch must still be at `expected_tip`;
//! - [`plan_push_undo`] and [`undo_push`] undo the newest operation of a
//!   tree when it is a push (`wt undo`): the server branch moves back to the
//!   tip it had before the push, refused when someone pushed on top since.

use chrono::{DateTime, Utc};
use reqwest::Method;
use serde::{Deserialize, Deserializer, Serialize};

use super::auth::map_api_error;
use super::rest::{seg, RestClient};
use super::session::remote_target;
use crate::error::{Result, SdkError};
use crate::model::Snapshot;
use crate::repo::{PushUndo, Repository};

/// One branch move in the server reflog (`GET {tree}/reflog`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerReflogEntry {
    #[serde(default)]
    pub tree_id: String,
    pub branch: String,
    /// Tip before the move ("" = the push created the branch).
    #[serde(default)]
    pub old_tip: String,
    pub new_tip: String,
    /// "push", "force_push" or "restore".
    pub kind: String,
    /// The move rewound the branch (old_tip is not an ancestor of new_tip).
    #[serde(default)]
    pub forced: bool,
    /// Branch protection was bypassed; `bypassed` lists the lifted rules.
    #[serde(default)]
    pub bypass: bool,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub bypassed: Vec<String>,
    #[serde(default)]
    pub actor: String,
    #[serde(default)]
    pub actor_id: String,
    /// Snapshots the push added.
    #[serde(default)]
    pub count: u64,
    pub at: DateTime<Utc>,
}

/// Body of `POST {tree}/reflog/restore`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreBranchRequest {
    pub branch: String,
    /// Snapshot to restore: the old or new tip of a reflog entry of the
    /// branch.
    pub tip: String,
    /// The branch tip the caller saw (lease).
    pub expected_tip: String,
    /// Bypass branch protection (`branch:bypass_protection`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bypass: bool,
}

/// Result of [`RestClient::restore_branch`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchRestore {
    pub branch: String,
    /// The tip the restore replaced and the restored one.
    pub old_tip: String,
    pub new_tip: String,
    /// The restore rewound the branch.
    #[serde(default)]
    pub forced: bool,
    #[serde(default)]
    pub bypass: bool,
    #[serde(default, deserialize_with = "null_as_empty")]
    pub bypassed: Vec<String>,
    /// The reflog entry recording the restore (kind "restore").
    pub entry: ServerReflogEntry,
}

fn null_as_empty<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Vec<String>, D::Error> {
    Ok(Option::<Vec<String>>::deserialize(d)?.unwrap_or_default())
}

/// `/api/v1/trees/{tenant}/{tree}`.
fn tree_api(tenant: &str, tree: &str) -> String {
    format!("/api/v1/trees/{}/{}", seg(tenant), seg(tree))
}

impl RestClient {
    /// The server reflog of `tenant/tree`, newest first: one branch when
    /// `branch` is given, at most `limit` entries (server default 50, at
    /// most 500).
    pub async fn server_reflog(
        &self,
        tenant: &str,
        tree: &str,
        branch: Option<&str>,
        limit: Option<usize>,
    ) -> Result<Vec<ServerReflogEntry>> {
        #[derive(Deserialize)]
        struct Page {
            #[serde(default)]
            entries: Option<Vec<ServerReflogEntry>>,
        }
        let mut query = Vec::new();
        if let Some(b) = branch {
            query.push(("branch", b.to_string()));
        }
        if let Some(n) = limit {
            query.push(("limit", n.to_string()));
        }
        let page: Page = self
            .get_query(&format!("{}/reflog", tree_api(tenant, tree)), &query)
            .await?;
        Ok(page.entries.unwrap_or_default())
    }

    /// Move a branch of `tenant/tree` to a tip of its reflog. Errors:
    /// `PermissionDenied` without `branch:force_push`, `BranchProtection`
    /// when a protection refuses, `PushRejected` when the branch is no
    /// longer at `expected_tip` (someone pushed since), `Remote` otherwise.
    pub async fn restore_branch(
        &self,
        tenant: &str,
        tree: &str,
        req: &RestoreBranchRequest,
    ) -> Result<BranchRestore> {
        let path = format!("{}/reflog/restore", tree_api(tenant, tree));
        match self
            .request_raw(Method::POST, &path, &[], Some(req))
            .await?
        {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
                SdkError::Remote(format!(
                    "unexpected response from {}{path}: {e}",
                    self.server()
                ))
            }),
            Err(failure) => Err(restore_error(self.server(), failure.status, &failure.body)),
        }
    }

    /// The tip of `branch` on `tenant/tree` (`None` when the branch does
    /// not exist).
    pub async fn server_branch_tip(
        &self,
        tenant: &str,
        tree: &str,
        branch: &str,
    ) -> Result<Option<String>> {
        #[derive(Deserialize)]
        struct Branch {
            name: String,
            #[serde(default)]
            tip: String,
        }
        let branches: Option<Vec<Branch>> = self
            .get(&format!("{}/branches", tree_api(tenant, tree)))
            .await?;
        Ok(branches
            .unwrap_or_default()
            .into_iter()
            .find(|b| b.name == branch)
            .map(|b| b.tip))
    }
}

/// Map an error answer of the restore endpoint: protection refusals and
/// lost leases get their own variants.
fn restore_error(server: &str, status: u16, body: &str) -> SdkError {
    #[derive(Deserialize)]
    struct Envelope {
        error: Detail,
    }
    #[derive(Deserialize)]
    struct Detail {
        #[serde(default)]
        code: String,
        #[serde(default)]
        message: String,
    }
    match serde_json::from_str::<Envelope>(body) {
        Ok(env) if status == 403 && env.error.code == "branch_protection" => {
            SdkError::BranchProtection(env.error.message)
        }
        Ok(env) if status == 409 => SdkError::PushRejected(env.error.message),
        _ => map_api_error(server, status, body),
    }
}

/// A REST client for `remote` plus the server tree (`tenant`, name) backing
/// local `tree`. With `login`, a saved credential is required; otherwise
/// the client falls back to anonymous access (public trees).
fn remote_rest(
    repo: &Repository,
    tree: &str,
    remote: &str,
    login: bool,
) -> Result<(RestClient, String, String)> {
    let (server, tenant) = remote_target(repo, remote)?;
    let info = repo.tree(tree)?;
    let name = if info.remote_name.is_empty() {
        info.name
    } else {
        info.remote_name
    };
    let client = match RestClient::for_server(&server) {
        Ok(client) => client,
        Err(SdkError::NotLoggedIn(_)) if !login => RestClient::new(&server, None)?,
        Err(e) => return Err(e),
    };
    Ok((client, tenant, name))
}

/// The server reflog of local `tree` on `remote` (`wt reflog --remote`),
/// newest first; see [`RestClient::server_reflog`].
pub async fn remote_reflog(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<ServerReflogEntry>> {
    let (client, tenant, name) = remote_rest(repo, tree, remote, false)?;
    client.server_reflog(&tenant, &name, branch, limit).await
}

/// What [`undo_push`] will change, checked against the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushUndoPlan {
    pub undo: PushUndo,
    /// Server URL and `tenant/tree` of the remote.
    pub server: String,
    pub server_tree: String,
    /// The tip the server branch moves back to.
    pub restore_to: String,
    /// Snapshots that leave the server branch (those of the push), newest
    /// first; empty when the restored tip is not stored locally.
    pub removed: Vec<Snapshot>,
}

/// Check that the push `undo` can be undone: it did not create the branch,
/// and the server branch is still where the push left it. Errors:
/// `InvalidArgument` for a push that created the branch, `PushRejected`
/// when the branch moved since (someone pushed on top) or was deleted.
pub async fn plan_push_undo(repo: &Repository, undo: &PushUndo) -> Result<PushUndoPlan> {
    let (remote, branch) = (&undo.remote, &undo.branch);
    let Some(restore_to) = undo.old_tip.clone() else {
        return Err(SdkError::InvalidArgument(format!(
            "the push created {remote}/{branch}; there is no earlier server tip to move it back to \
             (delete the branch on the server instead)"
        )));
    };
    let (client, tenant, name) = remote_rest(repo, &undo.tree, remote, true)?;
    let current = client.server_branch_tip(&tenant, &name, branch).await?;
    match current.as_deref() {
        Some(tip) if tip == undo.new_tip => {}
        Some(tip) => return Err(SdkError::PushRejected(moved_message(undo, tip))),
        None => {
            return Err(SdkError::PushRejected(format!(
                "{remote}/{branch} no longer exists on the server; there is no push to undo"
            )))
        }
    }
    let removed = if repo.has_snapshot(&undo.new_tip) && repo.has_snapshot(&restore_to) {
        let kept: std::collections::HashSet<String> = repo
            .ancestry(&restore_to)?
            .into_iter()
            .map(|s| s.id)
            .collect();
        let mut removed: Vec<Snapshot> = repo
            .ancestry(&undo.new_tip)?
            .into_iter()
            .filter(|s| !kept.contains(&s.id))
            .collect();
        removed.reverse();
        removed
    } else {
        Vec::new()
    };
    Ok(PushUndoPlan {
        undo: undo.clone(),
        server: client.server().to_string(),
        server_tree: format!("{tenant}/{name}"),
        restore_to,
        removed,
    })
}

/// Result of [`undo_push`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushUndoReport {
    pub tree: String,
    pub remote: String,
    pub branch: String,
    /// The server tip before (the pushed tip) and after the undo.
    pub old_tip: String,
    pub new_tip: String,
    /// The undo rewound the server branch (always, except for undoing a
    /// force push whose overwritten tip descends from the pushed one).
    pub forced: bool,
    pub bypass: bool,
    /// The server reflog entry recording the restore.
    pub entry: ServerReflogEntry,
}

/// Undo the push of `plan` on the server: restore the branch to the tip it
/// had before the push, with the pushed tip as lease, then move the
/// remote-tracking ref back and record the undo in the reflog and oplog.
/// Local branches keep their snapshots, so `wt push` publishes them again.
/// Errors as [`RestClient::restore_branch`].
pub async fn undo_push(
    repo: &Repository,
    plan: &PushUndoPlan,
    bypass_protection: bool,
) -> Result<PushUndoReport> {
    let undo = &plan.undo;
    let (client, tenant, name) = remote_rest(repo, &undo.tree, &undo.remote, true)?;
    let req = RestoreBranchRequest {
        branch: undo.branch.clone(),
        tip: plan.restore_to.clone(),
        expected_tip: undo.new_tip.clone(),
        bypass: bypass_protection,
    };
    let restored = client
        .restore_branch(&tenant, &name, &req)
        .await
        .map_err(|e| match e {
            // The lease was lost between the plan and the restore.
            SdkError::PushRejected(msg) => SdkError::PushRejected(format!(
                "cannot undo the push of {}/{}: {msg}; undoing would discard the snapshots pushed since. {REVERT_HINT}",
                undo.remote, undo.branch
            )),
            other => other,
        })?;
    repo.record_push_undo(undo)?;
    Ok(PushUndoReport {
        tree: undo.tree.clone(),
        remote: undo.remote.clone(),
        branch: undo.branch.clone(),
        old_tip: restored.old_tip,
        new_tip: restored.new_tip,
        forced: restored.forced,
        bypass: restored.bypass,
        entry: restored.entry,
    })
}

/// How to take pushed changes back without rewinding the server branch.
const REVERT_HINT: &str = "Run `wt pull` and take your changes back with `wt revert` instead";

fn moved_message(undo: &PushUndo, now: &str) -> String {
    format!(
        "{remote}/{branch} has moved since your push: the server tip is now {}, not {} \
         (someone pushed on top); undoing would discard their snapshots. {REVERT_HINT}",
        short(now),
        short(&undo.new_tip),
        remote = undo.remote,
        branch = undo.branch,
    )
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_errors_have_their_own_variants() {
        let body = |code: &str, msg: &str| {
            serde_json::json!({"error": {"code": code, "message": msg}}).to_string()
        };
        assert!(matches!(
            restore_error("s", 403, &body("branch_protection", "no_force_push")),
            SdkError::BranchProtection(m) if m == "no_force_push"
        ));
        assert!(matches!(
            restore_error("s", 403, &body("forbidden", "needs branch:force_push")),
            SdkError::PermissionDenied(m) if m.contains("branch:force_push")
        ));
        assert!(matches!(
            restore_error("s", 409, &body("conflict", "moved")),
            SdkError::PushRejected(m) if m == "moved"
        ));
        assert!(matches!(
            restore_error("s", 400, &body("bad_request", "not in the reflog")),
            SdkError::Remote(m) if m.contains("not in the reflog")
        ));
    }

    #[test]
    fn entries_tolerate_null_lists() {
        let e: ServerReflogEntry = serde_json::from_value(serde_json::json!({
            "branch": "main", "old_tip": "a", "new_tip": "b", "kind": "restore",
            "bypassed": null, "at": "2026-11-01T10:00:00Z"
        }))
        .unwrap();
        assert!(e.bypassed.is_empty() && !e.forced && e.kind == "restore");
    }

    #[test]
    fn requests_omit_a_false_bypass() {
        let req = RestoreBranchRequest {
            branch: "main".into(),
            tip: "a".into(),
            expected_tip: "b".into(),
            bypass: false,
        };
        let v = serde_json::to_value(&req).unwrap();
        assert!(v.get("bypass").is_none(), "{v}");
    }
}
