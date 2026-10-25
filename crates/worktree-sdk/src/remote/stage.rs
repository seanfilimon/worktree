//! Staged (team-visible WIP) snapshots: `stage` and `list_staged`.

use std::collections::BTreeMap;

use chrono::Utc;
use worktree_protocol::proto::sync as wire;

use super::client::ObjectKind;
use super::convert::{branch_id, snapshot_id, tenant_id, tree_id};
use super::session::{self, Access};
use super::StagedInfo;
use crate::error::Result;
use crate::model::{Manifest, ManifestEntry};
use crate::repo::Repository;
use crate::store::ObjectHash;

/// Paths changed between two manifests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ChangeSummary {
    /// Every added, modified or deleted path, sorted.
    pub files_changed: Vec<String>,
    pub added: u32,
    pub modified: u32,
    pub deleted: u32,
}

/// Compare `base` → `working` by path (content hash or executable bit
/// differences count as modifications).
pub(crate) fn summarize_changes(base: &Manifest, working: &Manifest) -> ChangeSummary {
    let index = |m: &Manifest| -> BTreeMap<String, (ObjectHash, bool)> {
        m.entries
            .iter()
            .map(|e: &ManifestEntry| (e.path.clone(), (e.hash, e.executable)))
            .collect()
    };
    let (old, new) = (index(base), index(working));
    let mut summary = ChangeSummary::default();
    for (path, entry) in &new {
        match old.get(path) {
            None => summary.added += 1,
            Some(prev) if prev != entry => summary.modified += 1,
            Some(_) => continue,
        }
        summary.files_changed.push(path.clone());
    }
    for path in old.keys().filter(|p| !new.contains_key(*p)) {
        summary.deleted += 1;
        summary.files_changed.push(path.clone());
    }
    summary.files_changed.sort();
    summary
}

/// See [`super::stage`]. The change summary is relative to the shared
/// branch head (the server's tip when known locally, else the last fetched
/// `refs/remotes/<remote>/<branch>`), i.e. everything this user has not
/// pushed yet — committed or not.
pub(crate) async fn stage(repo: &Repository, tree: &str, remote: &str) -> Result<StagedInfo> {
    let tree_name = repo.tree(tree)?.name;
    let branch = repo.current_branch(&tree_name)?;
    let mut session = session::open(repo, &tree_name, remote, Access::Write).await?;

    let working = repo.manifest_from_working(&tree_name)?;
    let manifest_hash = repo.write_manifest(&working)?;

    let server_tip = session
        .client
        .list_branches(&session.tree.id)
        .await?
        .into_iter()
        .find(|b| b.name == branch)
        .and_then(|b| b.tip);
    let base_id = match server_tip.filter(|t| repo.has_snapshot(t)) {
        Some(tip) => Some(tip),
        None => repo
            .remote_tip(&tree_name, remote, &branch)?
            .filter(|t| repo.has_snapshot(t)),
    };
    let base = match base_id {
        Some(id) => repo.read_manifest(&repo.get_snapshot(&id)?.manifest)?,
        None => Manifest::default(),
    };
    let summary = summarize_changes(&base, &working);

    let mut objects: Vec<(ObjectHash, ObjectKind)> = working
        .entries
        .iter()
        .map(|e| (e.hash, ObjectKind::Blob))
        .collect();
    objects.push((manifest_hash, ObjectKind::Manifest));
    let hashes: Vec<ObjectHash> = objects.iter().map(|(h, _)| *h).collect();
    let missing: std::collections::HashSet<ObjectHash> = session
        .client
        .missing_objects(&hashes)
        .await?
        .into_iter()
        .collect();
    let to_send: Vec<_> = objects
        .into_iter()
        .filter(|(h, _)| missing.contains(h))
        .collect();
    session
        .client
        .upload_objects(repo.store(), &to_send)
        .await?;

    // A clean working tree stages the head snapshot itself (the daemon
    // stages right after an auto-snapshot); otherwise a fresh id names the WIP.
    let head = repo
        .branch_tip(&tree_name, &branch)?
        .map(|id| repo.get_snapshot(&id))
        .transpose()?;
    let (staged_id, message) = match head {
        Some(h) if h.manifest == manifest_hash => (h.id, Some(h.message).filter(|m| !m.is_empty())),
        _ => (uuid::Uuid::new_v4().to_string(), None),
    };

    let req = wire::StageUploadRequest {
        tenant_id: Some(tenant_id(&session.tree.tenant_id)),
        tree_id: Some(tree_id(&session.tree.id)),
        branch_id: Some(branch_id(&branch)),
        branch_name: branch.clone(),
        snapshot_id: Some(snapshot_id(&staged_id)),
        files_changed: summary.files_changed.clone(),
        files_added: summary.added,
        files_modified: summary.modified,
        files_deleted: summary.deleted,
        message: message.clone(),
        account_id: None,
    };
    session.client.stage(req).await?;

    Ok(StagedInfo {
        snapshot_id: staged_id,
        author: session.username.clone().unwrap_or_else(|| repo.author()),
        branch,
        files_changed: summary.files_changed,
        files_added: summary.added,
        files_modified: summary.modified,
        files_deleted: summary.deleted,
        message,
        staged_at: Utc::now(),
    })
}

pub(crate) async fn list_staged(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<Vec<StagedInfo>> {
    let mut session = session::open(repo, tree, remote, Access::Read).await?;
    session.client.list_staged(&session.tree.id, branch).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::hash_bytes;

    fn entry(path: &str, content: &str, executable: bool) -> ManifestEntry {
        ManifestEntry {
            path: path.into(),
            hash: hash_bytes(content.as_bytes()),
            size: content.len() as u64,
            executable,
            chunked: false,
        }
    }

    fn manifest(entries: Vec<ManifestEntry>) -> Manifest {
        Manifest { entries }
    }

    #[test]
    fn identical_manifests_have_no_changes() {
        let m = manifest(vec![entry("a", "1", false), entry("b/c", "2", true)]);
        assert_eq!(summarize_changes(&m, &m), ChangeSummary::default());
    }

    #[test]
    fn counts_added_modified_deleted() {
        let base = manifest(vec![
            entry("keep", "k", false),
            entry("mod", "old", false),
            entry("gone", "g", false),
            entry("x", "x", false),
        ]);
        let working = manifest(vec![
            entry("keep", "k", false),
            entry("mod", "new", false),
            entry("new", "n", false),
            entry("x", "x", true),
        ]);
        let s = summarize_changes(&base, &working);
        assert_eq!(s.added, 1);
        assert_eq!(s.modified, 2, "content change and executable-bit change");
        assert_eq!(s.deleted, 1);
        assert_eq!(s.files_changed, vec!["gone", "mod", "new", "x"]);
    }

    #[test]
    fn empty_base_means_everything_added() {
        let working = manifest(vec![entry("z", "1", false), entry("a", "2", false)]);
        let s = summarize_changes(&Manifest::default(), &working);
        assert_eq!((s.added, s.modified, s.deleted), (2, 0, 0));
        assert_eq!(s.files_changed, vec!["a", "z"]);
    }

    #[test]
    fn empty_working_means_everything_deleted() {
        let base = manifest(vec![entry("a", "1", false)]);
        let s = summarize_changes(&base, &Manifest::default());
        assert_eq!((s.added, s.modified, s.deleted), (0, 0, 1));
    }
}
