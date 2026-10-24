//! `fetch` (download remote history into `refs/remotes/<remote>/*`) and
//! `pull` (fetch + integrate into a local branch).

use std::collections::{BTreeSet, HashSet};

use chrono::Utc;
use worktree_protocol::proto::sync as wire;

use super::client::RemoteBranch;
use super::convert::{branch_id, hash_from_wire, snapshot_id, tenant_id, tree_id};
use super::session::{self, Access, RemoteSession};
use super::{FetchReport, PullReport};
use crate::error::{Result, SdkError};
use crate::model::{MergeOptions, MergeOutcome, ReflogEntry, Snapshot};
use crate::repo::Repository;
use crate::store::ObjectHash;

pub(crate) async fn fetch(repo: &Repository, tree: &str, remote: &str) -> Result<FetchReport> {
    let mut session = session::open(repo, tree, remote, Access::Read).await?;
    Ok(fetch_with(repo, tree, remote, &mut session).await?.0)
}

/// Fetch through an open session. Also returns the server's branch list.
pub(crate) async fn fetch_with(
    repo: &Repository,
    tree: &str,
    remote: &str,
    session: &mut RemoteSession,
) -> Result<(FetchReport, Vec<RemoteBranch>)> {
    let tree_name = repo.tree(tree)?.name;
    let branches = session.client.list_branches(&session.tree.id).await?;
    let mut report = FetchReport {
        tree: tree_name.clone(),
        updated: Vec::new(),
        snapshots_received: 0,
        objects_received: 0,
        tags_received: 0,
    };

    for branch in &branches {
        let old = repo.remote_tip(&tree_name, remote, &branch.name)?;
        if old == branch.tip {
            continue;
        }
        if let Some(tip) = branch.tip.as_deref().filter(|t| !repo.has_snapshot(t)) {
            let have = old.as_deref().filter(|o| repo.has_snapshot(o));
            let (snapshots, objects) = download_history(repo, session, &branch.name, have).await?;
            report.snapshots_received += snapshots;
            report.objects_received += objects;
            if !repo.has_snapshot(tip) {
                return Err(SdkError::Remote(format!(
                    "server did not send snapshot {tip} (tip of '{}')",
                    branch.name
                )));
            }
        }
        repo.set_remote_tip(&tree_name, remote, &branch.name, branch.tip.as_deref())?;
        report
            .updated
            .push((branch.name.clone(), old, branch.tip.clone()));
    }

    // Branches that disappeared from the server: drop their remote refs.
    let on_server: HashSet<&str> = branches.iter().map(|b| b.name.as_str()).collect();
    for local in repo.branches(&tree_name, true)? {
        if on_server.contains(local.name.as_str()) {
            continue;
        }
        if let Some(old) = repo.remote_tip(&tree_name, remote, &local.name)? {
            repo.set_remote_tip(&tree_name, remote, &local.name, None)?;
            report.updated.push((local.name, Some(old), None));
        }
    }
    report.updated.sort_by(|a, b| a.0.cmp(&b.0));

    report.tags_received = fetch_tags(repo, &tree_name, session).await?;
    Ok((report, branches))
}

/// `Pull` one branch's missing snapshots, download their objects, then
/// persist the snapshot records (objects first, so a record never exists
/// without its content). Returns `(snapshots written, objects downloaded)`.
async fn download_history(
    repo: &Repository,
    session: &mut RemoteSession,
    branch: &str,
    have: Option<&str>,
) -> Result<(usize, usize)> {
    let req = wire::PullRequest {
        tenant_id: Some(tenant_id(&session.tree.tenant_id)),
        tree_id: Some(tree_id(&session.tree.id)),
        branch_id: Some(branch_id(branch)),
        current_tip: have.map(snapshot_id),
        account_id: None,
        depth: None,
    };
    let resp = session.client.pull(req).await?;
    let mut seen = HashSet::new();
    let mut fresh = Vec::new();
    for rec in &resp.snapshots {
        let snap = Snapshot::from_record(rec)?;
        if !repo.has_snapshot(&snap.id) && seen.insert(snap.id.clone()) {
            fresh.push(snap);
        }
    }
    check_parents_known(&fresh, |id| repo.has_snapshot(id))?;

    let hinted = resp
        .objects_to_fetch
        .iter()
        .map(|h| hash_from_wire(Some(h)))
        .collect::<Result<Vec<_>>>()?;
    let objects = ensure_objects(repo, session, &fresh, &hinted).await?;
    for snap in &fresh {
        repo.write_snapshot(snap)?;
    }
    Ok((fresh.len(), objects))
}

/// Every parent of every snapshot in `fresh` must be stored locally or
/// appear earlier in `fresh` (records arrive oldest first).
pub(crate) fn check_parents_known(
    fresh: &[Snapshot],
    is_local: impl Fn(&str) -> bool,
) -> Result<()> {
    let mut known: HashSet<&str> = HashSet::new();
    for snap in fresh {
        for parent in &snap.parents {
            if !known.contains(parent.as_str()) && !is_local(parent) {
                return Err(SdkError::Remote(format!(
                    "incomplete history from server: snapshot {} references unknown parent {parent}",
                    snap.id
                )));
            }
        }
        known.insert(&snap.id);
    }
    Ok(())
}

/// Download whatever `snapshots` need that the local store lacks: first the
/// server's hint list, then any manifests and blobs still missing (so a
/// partial hint from the server is repaired rather than trusted).
async fn ensure_objects(
    repo: &Repository,
    session: &mut RemoteSession,
    snapshots: &[Snapshot],
    hinted: &[ObjectHash],
) -> Result<usize> {
    let store = repo.store();
    let mut received = 0;

    let wanted: Vec<ObjectHash> = hinted.iter().filter(|h| !store.has(h)).copied().collect();
    if !wanted.is_empty() {
        received += session.client.download_objects(store, &wanted).await?;
    }

    let manifests: BTreeSet<ObjectHash> = snapshots.iter().map(|s| s.manifest).collect();
    let missing: Vec<ObjectHash> = manifests
        .iter()
        .filter(|h| !store.has(h))
        .copied()
        .collect();
    if !missing.is_empty() {
        received += session.client.download_objects(store, &missing).await?;
        require_present(repo, &missing)?;
    }

    let mut blobs = BTreeSet::new();
    for hash in &manifests {
        for entry in repo.read_manifest(hash)?.entries {
            if !store.has(&entry.hash) {
                blobs.insert(entry.hash);
            }
        }
    }
    if !blobs.is_empty() {
        let blobs: Vec<ObjectHash> = blobs.into_iter().collect();
        received += session.client.download_objects(store, &blobs).await?;
        require_present(repo, &blobs)?;
    }
    Ok(received)
}

fn require_present(repo: &Repository, hashes: &[ObjectHash]) -> Result<()> {
    match hashes.iter().find(|h| !repo.store().has(h)) {
        Some(h) => Err(SdkError::Remote(format!(
            "server did not provide object {h}"
        ))),
        None => Ok(()),
    }
}

/// Store server tags that are missing locally and whose target is known.
async fn fetch_tags(repo: &Repository, tree: &str, session: &mut RemoteSession) -> Result<usize> {
    let local: HashSet<String> = repo.tags(tree)?.into_iter().map(|t| t.name).collect();
    let mut received = 0;
    for tag in session.client.list_tags(&session.tree.id).await? {
        if local.contains(&tag.name) {
            continue;
        }
        if !repo.has_snapshot(&tag.target) {
            tracing::warn!(tag = %tag.name, target = %tag.target, "skipping remote tag whose target was not fetched");
            continue;
        }
        repo.put_tag(tree, &tag)?;
        received += 1;
    }
    Ok(received)
}

/// How a local branch relates to its remote counterpart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Integration {
    /// Nothing to do (equal, remote empty, or local ahead).
    UpToDate,
    /// Local is empty or an ancestor of remote.
    FastForward(String),
    /// Both sides have snapshots the other lacks.
    Diverged(String),
}

pub(crate) fn plan_integration(
    local: Option<&str>,
    remote: Option<&str>,
    is_ancestor: impl Fn(&str, &str) -> Result<bool>,
) -> Result<Integration> {
    let Some(remote) = remote else {
        return Ok(Integration::UpToDate);
    };
    let Some(local) = local else {
        return Ok(Integration::FastForward(remote.to_string()));
    };
    if local == remote || is_ancestor(remote, local)? {
        Ok(Integration::UpToDate)
    } else if is_ancestor(local, remote)? {
        Ok(Integration::FastForward(remote.to_string()))
    } else {
        Ok(Integration::Diverged(remote.to_string()))
    }
}

pub(crate) async fn pull(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<PullReport> {
    let tree_name = repo.tree(tree)?.name;
    if repo.merge_state(&tree_name)?.is_some() {
        return Err(SdkError::MergeInProgress);
    }
    let current = repo.current_branch(&tree_name)?;
    let branch = branch.map_or_else(|| current.clone(), str::to_string);

    let mut session = session::open(repo, &tree_name, remote, Access::Read).await?;
    let (fetch, server_branches) = fetch_with(repo, &tree_name, remote, &mut session).await?;
    if !server_branches.iter().any(|b| b.name == branch) {
        return Err(SdkError::BranchNotFound(format!("{remote}/{branch}")));
    }

    let remote_tip = repo.remote_tip(&tree_name, remote, &branch)?;
    let old = repo.branch_tip(&tree_name, &branch)?;
    let is_current = branch == current;
    let plan = plan_integration(old.as_deref(), remote_tip.as_deref(), |a, d| {
        repo.is_ancestor(a, d)
    })?;
    let outcome = match plan {
        Integration::UpToDate => MergeOutcome::UpToDate,
        Integration::FastForward(to) => {
            if is_current {
                // Refuses (DirtyWorkingTree) before touching anything if
                // there are uncommitted changes.
                repo.checkout(&tree_name, &to, false)?;
            }
            repo.set_branch_tip(&tree_name, &branch, Some(&to))?;
            MergeOutcome::FastForward { to }
        }
        Integration::Diverged(to) => {
            if !is_current {
                return Err(SdkError::InvalidArgument(format!(
                    "branch '{branch}' has diverged from {remote}/{branch}; switch to it and run `wt pull` to merge"
                )));
            }
            let opts = MergeOptions {
                message: Some(format!("Merge {remote}/{branch} into {branch}")),
                ..MergeOptions::default()
            };
            repo.merge(&tree_name, &to, opts)?
        }
    };

    if outcome != MergeOutcome::UpToDate {
        let new = repo.branch_tip(&tree_name, &branch)?;
        let summary = match &outcome {
            MergeOutcome::FastForward { .. } => "fast-forward".to_string(),
            MergeOutcome::Merged { .. } => "merged".to_string(),
            MergeOutcome::Conflicts { conflicts } => format!("{} conflict(s)", conflicts.len()),
            MergeOutcome::UpToDate => "up to date".to_string(),
        };
        repo.append_reflog(&ReflogEntry {
            at: Utc::now(),
            tree: tree_name.clone(),
            branch: branch.clone(),
            action: "pull".into(),
            old,
            new,
            message: format!("pull {remote}/{branch}: {summary}"),
        })?;
    }

    Ok(PullReport {
        fetch,
        branch,
        outcome,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::hash_bytes;

    /// Linear history a <- b <- c plus a side branch a <- x.
    fn ancestor(a: &str, d: &str) -> Result<bool> {
        let chain = ["a", "b", "c"];
        let pos = |s: &str| chain.iter().position(|c| *c == s);
        Ok(match (a, d) {
            _ if a == d => true,
            ("a", "x") => true,
            _ => matches!((pos(a), pos(d)), (Some(i), Some(j)) if i < j),
        })
    }

    #[test]
    fn integration_plans() {
        use Integration::*;
        assert_eq!(
            plan_integration(Some("c"), None, ancestor).unwrap(),
            UpToDate
        );
        assert_eq!(plan_integration(None, None, ancestor).unwrap(), UpToDate);
        assert_eq!(
            plan_integration(None, Some("c"), ancestor).unwrap(),
            FastForward("c".into())
        );
        assert_eq!(
            plan_integration(Some("c"), Some("c"), ancestor).unwrap(),
            UpToDate
        );
        assert_eq!(
            plan_integration(Some("c"), Some("a"), ancestor).unwrap(),
            UpToDate
        );
        assert_eq!(
            plan_integration(Some("a"), Some("c"), ancestor).unwrap(),
            FastForward("c".into())
        );
        assert_eq!(
            plan_integration(Some("x"), Some("c"), ancestor).unwrap(),
            Diverged("c".into())
        );
    }

    #[test]
    fn integration_propagates_errors() {
        let failing =
            |_: &str, _: &str| -> Result<bool> { Err(SdkError::SnapshotNotFound("zz".into())) };
        assert!(matches!(
            plan_integration(Some("a"), Some("b"), failing),
            Err(SdkError::SnapshotNotFound(_))
        ));
    }

    fn snap(id: &str, parents: &[&str]) -> Snapshot {
        Snapshot {
            id: id.into(),
            tree_id: "t".into(),
            branch: "main".into(),
            parents: parents.iter().map(|p| p.to_string()).collect(),
            manifest: hash_bytes(id.as_bytes()),
            author: "a".into(),
            message: String::new(),
            created_at: Utc::now(),
            auto_generated: false,
        }
    }

    #[test]
    fn parents_must_be_local_or_earlier_in_batch() {
        let local = |id: &str| id == "base";
        let ok = vec![
            snap("s1", &["base"]),
            snap("s2", &["s1"]),
            snap("m", &["s2", "base"]),
        ];
        check_parents_known(&ok, local).unwrap();
        check_parents_known(&[snap("root", &[])], local).unwrap();

        let out_of_order = vec![snap("s2", &["s1"]), snap("s1", &["base"])];
        let err = check_parents_known(&out_of_order, local).unwrap_err();
        assert!(matches!(err, SdkError::Remote(m) if m.contains("unknown parent s1")));

        let dangling = vec![snap("s1", &["nowhere"])];
        assert!(check_parents_known(&dangling, local).is_err());
    }
}
