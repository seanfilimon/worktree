//! v0.5 (ENGINEERING_PLAN §13.11): `WatchBranches` (server push of branch
//! changes, used by the daemon's sync loop) and `Unstage` (`wt unstage`,
//! `wt stage --discard`).

use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use worktree_protocol::proto::sync as wire;

use super::client::SyncClient;
use super::convert::{opt_snapshot_id, tree_id};
use super::session::{self, Access};
use crate::error::{Result, SdkError};
use crate::repo::Repository;

/// One branch change on the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchEvent {
    /// Server tree id.
    pub tree_id: String,
    pub branch: String,
    pub old_tip: Option<String>,
    /// `None` when the branch was deleted (or created empty).
    pub new_tip: Option<String>,
    pub actor: String,
    pub at: Option<DateTime<Utc>>,
}

impl BranchEvent {
    fn from_wire(ev: wire::BranchEvent) -> Self {
        Self {
            tree_id: ev.tree_id.map(|t| t.value).unwrap_or_default(),
            branch: ev.branch_name,
            old_tip: opt_snapshot_id(ev.old_tip.as_ref()),
            new_tip: opt_snapshot_id(ev.new_tip.as_ref()),
            actor: ev.actor,
            at: ev
                .at
                .and_then(|ts| DateTime::from_timestamp(ts.seconds, ts.nanos.max(0) as u32)),
        }
    }
}

/// An open `WatchBranches` stream.
#[derive(Debug)]
pub struct BranchEventStream {
    server: String,
    inner: tonic::Streaming<wire::BranchEvent>,
}

impl BranchEventStream {
    /// The next event; `Ok(None)` when the server ended the stream cleanly.
    pub async fn next(&mut self) -> Result<Option<BranchEvent>> {
        match self.inner.message().await {
            Ok(ev) => Ok(ev.map(BranchEvent::from_wire)),
            Err(status) => Err(super::client::status_to_error(&self.server, &status)),
        }
    }
}

// Separate impl block (daemon workstream) so client.rs stays owned by `chunks`.
impl SyncClient {
    /// Open a `WatchBranches` stream for server trees `tree_ids`. Returns
    /// once the server has the subscription in place (it sends response
    /// headers then), so no later change is missed.
    pub async fn watch_branches(&mut self, tree_ids: &[String]) -> Result<BranchEventStream> {
        let req = wire::WatchBranchesRequest {
            tree_ids: tree_ids.iter().map(|id| tree_id(id)).collect(),
        };
        let inner = self
            .inner
            .watch_branches(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        Ok(BranchEventStream {
            server: self.server().to_string(),
            inner,
        })
    }

    /// Remove the caller's staged snapshot of server tree `tree` on
    /// `branch` (every branch when `None`); `true` if one existed.
    pub async fn unstage(&mut self, tree: &str, branch: Option<&str>) -> Result<bool> {
        let req = wire::UnstageRequest {
            tree_id: Some(tree_id(tree)),
            branch_name: branch.unwrap_or_default().to_string(),
        };
        let resp = self
            .inner
            .unstage(req)
            .await
            .map_err(|s| self.status(s))?
            .into_inner();
        Ok(resp.removed)
    }
}

/// A `WatchBranches` stream over the trees of a worktree.
#[derive(Debug)]
pub struct TreeWatch {
    pub stream: BranchEventStream,
    /// Server tree id -> local tree name, for every watched tree.
    pub trees: BTreeMap<String, String>,
}

/// Resolve local `trees` on `remote` and watch the ones that exist there
/// (trees never pushed are skipped). `TreeNotFound` when none exists.
/// Anonymous access works for public trees.
pub async fn watch_trees(repo: &Repository, trees: &[String], remote: &str) -> Result<TreeWatch> {
    let mut map = BTreeMap::new();
    let mut client = None;
    for tree in trees {
        match session::open(repo, tree, remote, Access::Read).await {
            Ok(s) => {
                map.insert(s.tree.id.clone(), tree.clone());
                client = Some(s.client);
            }
            Err(SdkError::TreeNotFound(name)) => {
                tracing::debug!(tree, %name, "not on the server; not watching it");
            }
            Err(e) => return Err(e),
        }
    }
    let Some(mut client) = client else {
        return Err(SdkError::TreeNotFound(format!(
            "no tree of this worktree exists on {remote}"
        )));
    };
    let ids: Vec<String> = map.keys().cloned().collect();
    let stream = client.watch_branches(&ids).await?;
    Ok(TreeWatch { stream, trees: map })
}

/// Remove the caller's staged snapshot of `tree` on `remote` for `branch`
/// (every branch when `None`). Requires a login. `true` if one existed.
pub async fn unstage(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<bool> {
    let mut session = session::open(repo, tree, remote, Access::Read).await?;
    if session.username.is_none() {
        return Err(SdkError::NotLoggedIn(session.client.server().to_string()));
    }
    let id = session.tree.id.clone();
    session.client.unstage(&id, branch).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_convert_from_wire() {
        let ev = BranchEvent::from_wire(wire::BranchEvent {
            tree_id: Some(tree_id("t1")),
            branch_name: "main".into(),
            old_tip: None,
            new_tip: Some(wire::SnapshotId { value: "s2".into() }),
            actor: "alice".into(),
            at: Some(prost_types::Timestamp {
                seconds: 1_700_000_000,
                nanos: 5,
            }),
        });
        assert_eq!(ev.tree_id, "t1");
        assert_eq!(ev.branch, "main");
        assert_eq!(ev.old_tip, None);
        assert_eq!(ev.new_tip.as_deref(), Some("s2"));
        assert_eq!(ev.at.map(|t| t.timestamp()), Some(1_700_000_000));
        let empty = BranchEvent::from_wire(wire::BranchEvent::default());
        assert_eq!(empty.tree_id, "");
        assert_eq!(empty.at, None);
    }
}
