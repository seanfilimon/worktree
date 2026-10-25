//! `fetch` (download remote history into `refs/remotes/<remote>/*`) and
//! `pull` (fetch + integrate into a local branch).

use std::collections::{BTreeSet, HashSet};

use chrono::Utc;
use worktree_protocol::proto::sync as wire;

use super::client::RemoteBranch;
use super::convert::{branch_id, hash_from_wire, snapshot_id, tenant_id, tree_id};
use super::session::{self, Access, RemoteSession};
use super::{FetchOptions, FetchReport, PullReport};
use crate::error::{Result, SdkError};
use crate::model::{MergeOptions, MergeOutcome, ReflogEntry, Snapshot};
use crate::repo::Repository;
use crate::store::ObjectHash;

pub(crate) async fn fetch(
    repo: &Repository,
    tree: &str,
    remote: &str,
    opts: FetchOptions,
) -> Result<FetchReport> {
    let mut session = session::open(repo, tree, remote, Access::Read).await?;
    Ok(fetch_with(repo, tree, remote, &mut session, opts).await?.0)
}

/// Stubbed-path bookkeeping across the branches of one fetch.
#[derive(Default)]
struct Stubs {
    /// Paths the server reported as stubbed.
    received: BTreeSet<String>,
    /// Every path of every manifest processed (the server had the chance to
    /// stub each of them).
    mentioned: BTreeSet<String>,
}

/// Fetch through an open session. Also returns the server's branch list.
///
/// With `opts.depth` each branch's missing history is cut to its newest
/// `depth` snapshots; with `opts.deepen` a shallow history is extended by
/// that many snapshots. Either way the shallow boundary
/// (`.wt/trees/<tree>/shallow`) and the stubbed path list are updated.
pub(crate) async fn fetch_with(
    repo: &Repository,
    tree: &str,
    remote: &str,
    session: &mut RemoteSession,
    opts: FetchOptions,
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
    let depth = opts.depth.filter(|d| *d > 0);
    let deepen = opts.deepen.filter(|d| *d > 0);
    let mut fresh_ids: Vec<String> = Vec::new();
    let mut stubs = Stubs::default();

    for branch in &branches {
        let old = repo.remote_tip(&tree_name, remote, &branch.name)?;
        if old == branch.tip && deepen.is_none() {
            continue;
        }
        if let Some(tip) = branch.tip.as_deref() {
            let request = match deepen {
                // Deepen: ask for the newest (local depth + N) snapshots of
                // the branch; those already stored are skipped.
                Some(n) => {
                    let base = [Some(tip), old.as_deref()]
                        .into_iter()
                        .flatten()
                        .find(|id| repo.has_snapshot(id));
                    let local = match base {
                        Some(id) => repo.ancestry(id)?.len(),
                        None => 0,
                    };
                    let total = u32::try_from(local).unwrap_or(u32::MAX).saturating_add(n);
                    Some((None, Some(total)))
                }
                None if repo.has_snapshot(tip) => None,
                None => Some((old.as_deref().filter(|o| repo.has_snapshot(o)), depth)),
            };
            if let Some((have, depth)) = request {
                let (ids, objects) =
                    download_history(repo, session, &branch.name, have, depth, &mut stubs).await?;
                report.snapshots_received += ids.len();
                report.objects_received += objects;
                fresh_ids.extend(ids);
                if !repo.has_snapshot(tip) {
                    return Err(SdkError::Remote(format!(
                        "server did not send snapshot {tip} (tip of '{}')",
                        branch.name
                    )));
                }
            }
        }
        if old != branch.tip {
            repo.set_remote_tip(&tree_name, remote, &branch.name, branch.tip.as_deref())?;
            report
                .updated
                .push((branch.name.clone(), old, branch.tip.clone()));
        }
    }

    if !fresh_ids.is_empty() || deepen.is_some() {
        repo.update_shallow_boundary(&tree_name, fresh_ids.iter().map(String::as_str))?;
    }
    if !stubs.mentioned.is_empty() {
        let old = repo.stubbed_paths(&tree_name)?;
        let mut new: BTreeSet<String> = old
            .iter()
            .filter(|p| !stubs.mentioned.contains(*p))
            .cloned()
            .collect();
        new.extend(stubs.received);
        if new != old {
            repo.set_stubbed_paths(&tree_name, &new)?;
        }
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

/// `Pull` one branch's missing snapshots (at most `depth`), download their
/// objects, then persist the snapshot records (objects first, so a record
/// never exists without its content). Returns `(ids of the snapshots
/// written, objects downloaded)`.
async fn download_history(
    repo: &Repository,
    session: &mut RemoteSession,
    branch: &str,
    have: Option<&str>,
    depth: Option<u32>,
    stubs: &mut Stubs,
) -> Result<(Vec<String>, usize)> {
    let req = wire::PullRequest {
        tenant_id: Some(tenant_id(&session.tree.tenant_id)),
        tree_id: Some(tree_id(&session.tree.id)),
        branch_id: Some(branch_id(branch)),
        current_tip: have.map(snapshot_id),
        account_id: None,
        depth,
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
    // A shallow request cuts history on purpose: the oldest snapshots
    // become the shallow boundary instead of an error.
    if depth.is_none() {
        check_parents_known(&fresh, |id| repo.has_snapshot(id))?;
    }

    let hinted = resp
        .objects_to_fetch
        .iter()
        .map(|h| hash_from_wire(Some(h)))
        .collect::<Result<Vec<_>>>()?;
    let stubbed: BTreeSet<String> = resp.stubbed_paths.iter().cloned().collect();
    let objects = ensure_objects(repo, session, &fresh, &hinted, &stubbed, stubs).await?;
    stubs.received.extend(stubbed);
    for snap in &fresh {
        repo.write_snapshot(snap)?;
    }
    Ok((fresh.into_iter().map(|s| s.id).collect(), objects))
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
/// server's hint list, then any manifests, blobs / chunk lists and chunks
/// still missing (so a partial hint from the server is repaired rather than
/// trusted). Entries at `stubbed` paths are skipped: the server withholds
/// their content (ENGINEERING_PLAN §13.2).
async fn ensure_objects(
    repo: &Repository,
    session: &mut RemoteSession,
    snapshots: &[Snapshot],
    hinted: &[ObjectHash],
    stubbed: &BTreeSet<String>,
    stubs: &mut Stubs,
) -> Result<usize> {
    let store = repo.store();
    let tree = session.tree.id.clone();
    let mut received = 0;

    let wanted: Vec<ObjectHash> = hinted.iter().filter(|h| !store.has(h)).copied().collect();
    if !wanted.is_empty() {
        received += session
            .client
            .download_objects(store, &tree, &wanted)
            .await?;
    }

    let manifests: BTreeSet<ObjectHash> = snapshots.iter().map(|s| s.manifest).collect();
    let missing: Vec<ObjectHash> = manifests
        .iter()
        .filter(|h| !store.has(h))
        .copied()
        .collect();
    if !missing.is_empty() {
        received += session
            .client
            .download_objects(store, &tree, &missing)
            .await?;
        require_present(repo, &missing)?;
    }

    // Blobs and chunk lists named by the manifests.
    let mut blobs = BTreeSet::new();
    let mut chunk_lists = BTreeSet::new();
    for hash in &manifests {
        for entry in repo.read_manifest(hash)?.entries {
            stubs.mentioned.insert(entry.path.clone());
            if stubbed.contains(&entry.path) {
                continue;
            }
            if entry.chunked {
                chunk_lists.insert(entry.hash);
            }
            if !store.has(&entry.hash) {
                blobs.insert(entry.hash);
            }
        }
    }
    if !blobs.is_empty() {
        let blobs: Vec<ObjectHash> = blobs.into_iter().collect();
        received += session
            .client
            .download_objects(store, &tree, &blobs)
            .await?;
        require_present(repo, &blobs)?;
    }

    // Chunks named by the chunk lists (closure expansion).
    let mut chunks = BTreeSet::new();
    for list in &chunk_lists {
        for chunk in store.read_chunk_list(list)?.chunks {
            if !store.has(&chunk.hash) {
                chunks.insert(chunk.hash);
            }
        }
    }
    if !chunks.is_empty() {
        let chunks: Vec<ObjectHash> = chunks.into_iter().collect();
        received += session
            .client
            .download_objects(store, &tree, &chunks)
            .await?;
        require_present(repo, &chunks)?;
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
    opts: FetchOptions,
) -> Result<PullReport> {
    let tree_name = repo.tree(tree)?.name;
    if repo.merge_state(&tree_name)?.is_some() {
        return Err(SdkError::MergeInProgress);
    }
    let current = repo.current_branch(&tree_name)?;
    let branch = branch.map_or_else(|| current.clone(), str::to_string);

    let mut session = session::open(repo, &tree_name, remote, Access::Read).await?;
    let (fetch, server_branches) = fetch_with(repo, &tree_name, remote, &mut session, opts).await?;
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
            dependencies: Vec::new(),
            reverts: None,
            signature: None,
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
