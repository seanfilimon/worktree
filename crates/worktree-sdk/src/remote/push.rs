//! `push`: send a local branch (snapshots, manifests, blobs, tags) to the server.

use std::collections::{BTreeSet, HashSet};

use chrono::Utc;
use worktree_protocol::proto::sync as wire;
use worktree_protocol::proto::sync::push_rejection::Reason;

use super::client::{entry_upload_objects, ObjectKind, RemoteBranch};
use super::convert::{branch_id, snapshot_id, tenant_id, tree_id};
use super::pack::{self, TransferStats};
use super::session::{self, Access, RemoteSession};
use super::PushReport;
use crate::error::{Result, SdkError};
use crate::model::{ReflogEntry, Snapshot};
use crate::repo::Repository;
use crate::store::ObjectHash;

pub(crate) async fn push(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<PushReport> {
    Ok(push_with(repo, tree, remote, branch).await?.0)
}

/// [`push`] that also reports how the objects travelled (pack-delta,
/// §16.1).
pub(crate) async fn push_with(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<(PushReport, TransferStats)> {
    let mut transfer = TransferStats::default();
    let tree_name = repo.tree(tree)?.name;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => repo.current_branch(&tree_name)?,
    };
    let new_tip = repo.branch_tip(&tree_name, &branch)?.ok_or_else(|| {
        SdkError::InvalidArgument(format!("branch '{branch}' has no snapshots to push"))
    })?;

    let mut session = session::open(repo, &tree_name, remote, Access::Write).await?;
    let server_branches = session.client.list_branches(&session.tree.id).await?;
    let server_tip = server_branches
        .iter()
        .find(|b| b.name == branch)
        .and_then(|b| b.tip.clone());

    let mut report = PushReport {
        tree: tree_name.clone(),
        branch: branch.clone(),
        old_tip: server_tip.clone(),
        new_tip: new_tip.clone(),
        snapshots_sent: 0,
        objects_sent: 0,
        up_to_date: false,
    };

    if server_tip.as_deref() == Some(new_tip.as_str()) {
        repo.set_remote_tip(&tree_name, remote, &branch, Some(&new_tip))?;
        let known = server_known_snapshots(repo, &server_branches)?;
        push_tags(repo, &tree_name, &mut session, &known).await?;
        report.up_to_date = true;
        return Ok((report, transfer));
    }

    // Refuse early (without uploading anything) when the server has history
    // this branch does not contain; the server would answer ConflictDetected.
    if let Some(tip) = &server_tip {
        if !repo.has_snapshot(tip) || !repo.is_ancestor(tip, &new_tip)? {
            return Err(SdkError::PushRejected(behind_message(remote, &branch, tip)));
        }
    }

    let mut known = server_known_snapshots(repo, &server_branches)?;
    let chain: Vec<Snapshot> = repo
        .ancestry(&new_tip)?
        .into_iter()
        .filter(|s| !known.contains(&s.id))
        .collect();

    let objects = objects_for(repo, &chain, &known)?;
    let hashes: Vec<ObjectHash> = objects.iter().map(|(h, _)| *h).collect();
    let missing: HashSet<ObjectHash> = session
        .client
        .missing_objects(&hashes)
        .await?
        .into_iter()
        .collect();
    let to_send: Vec<(ObjectHash, ObjectKind)> = objects
        .into_iter()
        .filter(|(h, _)| missing.contains(h))
        .collect();
    // ---- pack-delta (§16.1): one pack with deltas against the
    // server-known parent; whole objects when the server has no packs ----
    let packed = if to_send.is_empty() {
        None
    } else {
        let items = pack::upload_items(repo, &chain, &to_send);
        // Only objects of server-known snapshots: objects that are merely
        // stored on the server (a rejected push) are not valid bases.
        let server_has = pack::server_known_objects(repo, &chain, &known);
        pack::upload(
            &mut session.client,
            repo.store(),
            &session.tree.id,
            items,
            server_has,
        )
        .await?
    };
    report.objects_sent = match packed {
        Some(up) => {
            transfer = up.stats;
            if !up.retry.is_empty() {
                session
                    .client
                    .upload_objects(repo.store(), &up.retry)
                    .await?;
            }
            transfer.objects
        }
        None => {
            let sent = session
                .client
                .upload_objects(repo.store(), &to_send)
                .await?;
            transfer.objects = sent;
            sent
        }
    };
    // ---- end pack-delta ----

    let req = push_request(&session, &branch, server_tip.as_deref(), &new_tip, &chain);
    let resp = session.client.push(req).await?;
    if !resp.accepted {
        return Err(SdkError::PushRejected(rejection_message(
            remote,
            &branch,
            resp.rejection_reason.as_ref(),
        )));
    }
    report.snapshots_sent = chain.len();

    repo.set_remote_tip(&tree_name, remote, &branch, Some(&new_tip))?;
    repo.append_reflog(&ReflogEntry {
        at: Utc::now(),
        tree: tree_name.clone(),
        branch: branch.clone(),
        action: "push".into(),
        old: server_tip,
        new: Some(new_tip.clone()),
        message: format!(
            "push {branch} to {remote}/{branch} ({} snapshot(s))",
            chain.len()
        ),
    })?;

    known.extend(chain.into_iter().map(|s| s.id));
    push_tags(repo, &tree_name, &mut session, &known).await?;
    Ok((report, transfer))
}

/// Ids of every snapshot reachable from a server branch tip that is also
/// present locally — history the server is known to have.
fn server_known_snapshots(repo: &Repository, branches: &[RemoteBranch]) -> Result<HashSet<String>> {
    let mut known = HashSet::new();
    for tip in branches.iter().filter_map(|b| b.tip.as_deref()) {
        if known.contains(tip) || !repo.has_snapshot(tip) {
            continue;
        }
        known.extend(repo.ancestry(tip)?.into_iter().map(|s| s.id));
    }
    Ok(known)
}

/// Blobs (including the chunks of large files), chunk lists, then
/// manifests referenced by `chain`, each once — except those of the
/// server-known (`known`) snapshots the chain builds on.
fn objects_for(
    repo: &Repository,
    chain: &[Snapshot],
    known: &HashSet<String>,
) -> Result<Vec<(ObjectHash, ObjectKind)>> {
    let manifests: BTreeSet<ObjectHash> = chain.iter().map(|s| s.manifest).collect();
    let mut seen = HashSet::new();
    // hardening (§15.10): the server stores every object of the snapshots
    // reachable from its branch tips (a push is only accepted once they
    // are all present), so the entries of a known parent's manifest are
    // neither negotiated nor uploaded again. For an incremental push of a
    // large tree this shrinks the Negotiate request from every file of the
    // tree to the changed files.
    let parents: BTreeSet<&str> = chain
        .iter()
        .flat_map(|s| s.parents.iter())
        .filter(|p| known.contains(p.as_str()))
        .map(String::as_str)
        .collect();
    for parent in parents {
        let Ok(snap) = repo.get_snapshot(parent) else {
            continue;
        };
        let Ok(manifest) = repo.read_manifest(&snap.manifest) else {
            continue;
        };
        seen.insert(snap.manifest);
        seen.extend(manifest.entries.iter().map(|e| e.hash));
    }
    let mut objects = Vec::new();
    for hash in &manifests {
        for entry in repo.read_manifest(hash)?.entries {
            if seen.contains(&entry.hash) {
                continue;
            }
            for (h, kind) in entry_upload_objects(repo.store(), &entry)? {
                if seen.insert(h) {
                    objects.push((h, kind));
                }
            }
        }
    }
    objects.extend(
        manifests
            .into_iter()
            .filter(|h| seen.insert(*h))
            .map(|h| (h, ObjectKind::Manifest)),
    );
    Ok(objects)
}

fn push_request(
    session: &RemoteSession,
    branch: &str,
    old_tip: Option<&str>,
    new_tip: &str,
    chain: &[Snapshot],
) -> wire::PushRequest {
    let snapshots = chain
        .iter()
        .map(|s| {
            let mut rec = s.to_record();
            // Records are filed under the server's tree id.
            rec.tree_id = Some(tree_id(&session.tree.id));
            rec
        })
        .collect();
    wire::PushRequest {
        tenant_id: Some(tenant_id(&session.tree.tenant_id)),
        tree_id: Some(tree_id(&session.tree.id)),
        branch_id: Some(branch_id(branch)),
        branch_name: branch.to_string(),
        old_tip: old_tip.map(snapshot_id),
        new_tip: Some(snapshot_id(new_tip)),
        snapshot_chain: chain.iter().map(|s| snapshot_id(&s.id)).collect(),
        account_id: None,
        snapshots,
        ..Default::default()
    }
}

/// Push local tags the server lacks whose target the server has. Tag
/// failures never fail the (already successful) branch push; they are
/// logged instead.
async fn push_tags(
    repo: &Repository,
    tree: &str,
    session: &mut RemoteSession,
    server_has: &HashSet<String>,
) -> Result<()> {
    let remote: HashSet<String> = session
        .client
        .list_tags(&session.tree.id)
        .await?
        .into_iter()
        .map(|t| t.name)
        .collect();
    for tag in repo.tags(tree)? {
        if remote.contains(&tag.name) || !server_has.contains(&tag.target) {
            continue;
        }
        if let Err(e) = session.client.push_tag(&session.tree.id, &tag).await {
            tracing::warn!(tag = %tag.name, error = %e, "failed to push tag");
        }
    }
    Ok(())
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn behind_message(remote: &str, branch: &str, server_tip: &str) -> String {
    format!(
        "{remote}/{branch} has snapshots that are not in your local '{branch}' (remote tip {}); \
         run `wt pull` to integrate them, then push again",
        short(server_tip)
    )
}

/// Human-readable explanation of a `PushRejection` (`None` = no reason given).
pub fn rejection_message(
    remote: &str,
    branch: &str,
    rejection: Option<&wire::PushRejection>,
) -> String {
    let Some(reason) = rejection.and_then(|r| r.reason.as_ref()) else {
        return format!(
            "the server rejected the push to {remote}/{branch} without giving a reason"
        );
    };
    match reason {
        Reason::ConflictDetected(c) => match c.server_tip.as_ref().map(|t| t.value.as_str()).filter(|t| !t.is_empty()) {
            Some(tip) => behind_message(remote, branch, tip),
            None => format!("{remote}/{branch} changed since your last pull; run `wt pull`, then push again"),
        },
        Reason::BranchProtection(p) => {
            let rule = if p.rule.is_empty() { "direct pushes are not allowed".to_string() } else { p.rule.clone() };
            format!("branch '{branch}' is protected ({rule}); open a merge request instead")
        }
        Reason::AccessDenied(a) => {
            let why = if a.reason.is_empty() { "you lack write access to this tree" } else { a.reason.as_str() };
            format!("access denied: {why}")
        }
        Reason::LicenseViolation(l) => {
            format!("license policy violation: '{}' is under {} and may not be pushed here", l.path, l.license)
        }
        Reason::CiChecksFailed(c) => {
            if c.checks.is_empty() {
                "required CI checks failed".to_string()
            } else {
                format!("required CI checks failed: {}", c.checks.join(", "))
            }
        }
        Reason::ReviewRequired(r) => format!(
            "branch '{branch}' requires {} approval(s) (has {}); open a merge request and get it reviewed",
            r.required, r.current
        ),
        Reason::QuotaExceeded(q) => {
            let limit = if q.limit.is_empty() { "storage" } else { q.limit.as_str() };
            format!("tenant quota exceeded ({limit})")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_protocol::proto::sync::push_rejection as pr;

    fn rejection(reason: Reason) -> wire::PushRejection {
        wire::PushRejection {
            reason: Some(reason),
        }
    }

    fn msg(reason: Reason) -> String {
        rejection_message("origin", "main", Some(&rejection(reason)))
    }

    #[test]
    fn conflict_tells_user_to_pull() {
        let m = msg(Reason::ConflictDetected(pr::ConflictDetected {
            server_tip: Some(snapshot_id("0123456789abcdef")),
        }));
        assert!(m.contains("wt pull"), "{m}");
        assert!(m.contains("origin/main"), "{m}");
        assert!(m.contains("01234567") && !m.contains("0123456789"), "{m}");

        let m = msg(Reason::ConflictDetected(pr::ConflictDetected {
            server_tip: None,
        }));
        assert!(m.contains("wt pull"), "{m}");
    }

    #[test]
    fn every_rejection_variant_has_a_specific_message() {
        let m = msg(Reason::BranchProtection(pr::BranchProtection {
            rule: "no direct push to main".into(),
        }));
        assert!(
            m.contains("protected")
                && m.contains("no direct push to main")
                && m.contains("merge request"),
            "{m}"
        );
        let m = msg(Reason::BranchProtection(pr::BranchProtection {
            rule: String::new(),
        }));
        assert!(m.contains("direct pushes are not allowed"), "{m}");

        let m = msg(Reason::AccessDenied(pr::AccessDenied {
            reason: "viewer role".into(),
        }));
        assert!(
            m.contains("access denied") && m.contains("viewer role"),
            "{m}"
        );
        let m = msg(Reason::AccessDenied(pr::AccessDenied {
            reason: String::new(),
        }));
        assert!(m.contains("write access"), "{m}");

        let m = msg(Reason::LicenseViolation(pr::LicenseViolation {
            path: "lib/gpl.c".into(),
            license: "GPL-3.0".into(),
        }));
        assert!(m.contains("lib/gpl.c") && m.contains("GPL-3.0"), "{m}");

        let m = msg(Reason::CiChecksFailed(pr::CiChecksFailed {
            checks: vec!["build".into(), "lint".into()],
        }));
        assert!(m.contains("build, lint"), "{m}");
        let m = msg(Reason::CiChecksFailed(pr::CiChecksFailed {
            checks: vec![],
        }));
        assert_eq!(m, "required CI checks failed");

        let m = msg(Reason::ReviewRequired(pr::ReviewRequired {
            required: 2,
            current: 1,
        }));
        assert!(m.contains("2 approval") && m.contains("has 1"), "{m}");

        let m = msg(Reason::QuotaExceeded(pr::QuotaExceeded {
            limit: "10 GiB".into(),
        }));
        assert!(m.contains("quota") && m.contains("10 GiB"), "{m}");
        let m = msg(Reason::QuotaExceeded(pr::QuotaExceeded {
            limit: String::new(),
        }));
        assert!(m.contains("storage"), "{m}");
    }

    #[test]
    fn missing_reason_is_explained() {
        let m = rejection_message("origin", "dev", None);
        assert!(
            m.contains("origin/dev") && m.contains("without giving a reason"),
            "{m}"
        );
        let m = rejection_message("origin", "dev", Some(&wire::PushRejection { reason: None }));
        assert!(m.contains("without giving a reason"), "{m}");
    }

    #[test]
    fn short_ids() {
        assert_eq!(short("abc"), "abc");
        assert_eq!(short("0123456789"), "01234567");
    }
}
