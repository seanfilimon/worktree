//! One-step sync and the offline outbox (ENGINEERING_PLAN §15.5).
//!
//! [`sync`] runs fetch → integrate → push for the current branch of one
//! tree:
//!
//! 1. **Fetch** every remote branch (`refs/remotes/<remote>/*`). A tree the
//!    server does not have yet is skipped (the push creates it).
//! 2. **Integrate** `<remote>/<branch>` into the local branch when the
//!    remote has snapshots the local branch lacks: fast-forward, else a
//!    three-way merge (`sync.strategy = "merge"`, the default) or a replay
//!    of the local snapshots on top of the remote tip
//!    (`sync.strategy = "rebase"`). Conflicts stop exactly like `wt merge`
//!    / `wt rebase` (conflict markers, `MERGE_STATE.json`, ...); after
//!    resolving them [`sync_continue`] finishes the integration and pushes.
//! 3. **Push** the branch when it is ahead of the remote.
//!
//! When the server cannot be reached (connection refused, `Unavailable`,
//! timeouts — see [`is_unreachable`]) and the branch has unpushed
//! snapshots, the push is recorded in the tree's outbox
//! (`.wt/trees/<tree>/outbox.jsonl`, [`crate::repo::OutboxEntry`]) instead
//! of failing; [`flush_outbox`] delivers queued pushes later (`wt sync
//! flush`, or the daemon with `sync.auto_flush`).

use serde::{Deserialize, Serialize};

use super::fetch::{fetch_with, plan_integration, Integration};
use super::session::{self, Access};
use super::{push, FetchOptions, FetchReport, PushReport};
use crate::config::SyncStrategy;
use crate::error::{Result, SdkError};
use crate::model::{ConflictInfo, MergeOptions, MergeOutcome};
use crate::repo::{OutboxEntry, RebaseOptions, Repository, RewriteOp, RewriteStatus};

/// Options of [`sync`] / [`sync_continue`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SyncOptions {
    /// Overrides `sync.strategy` (`--merge` / `--rebase`).
    pub strategy: Option<SyncStrategy>,
    /// Fetch and integrate only (`--no-push`).
    pub no_push: bool,
    /// Undo a conflicting integration instead of leaving conflicts in the
    /// working tree (the daemon's unattended auto-sync).
    pub abort_on_conflict: bool,
}

/// Overall result of a sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    /// Nothing to fetch, integrate or push.
    UpToDate,
    /// Something was fetched, integrated and/or pushed.
    Synced,
    /// The integration stopped with conflicts (resolve, then continue).
    Conflicts,
    /// The integration conflicted and was undone (`abort_on_conflict`).
    Aborted,
    /// The server was unreachable; the push was queued in the outbox.
    Queued,
    /// The server was unreachable and there was nothing to queue.
    Offline,
}

/// What happened to the local branch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncIntegration {
    /// Not attempted (offline).
    None,
    /// The local branch already contains the remote branch.
    UpToDate,
    FastForward,
    /// A merge snapshot was created.
    Merged,
    /// Local snapshots were replayed on top of the remote branch.
    Rebased,
    /// Stopped with conflicts.
    Conflicts,
    /// Conflicted and was undone.
    Aborted,
}

/// Result of [`sync`] for one tree (`wt sync --json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncReport {
    pub tree: String,
    pub branch: String,
    pub remote: String,
    pub strategy: SyncStrategy,
    pub status: SyncStatus,
    /// `None` when the fetch did not happen (offline) or the tree is not
    /// on the server yet.
    pub fetch: Option<FetchReport>,
    pub integration: SyncIntegration,
    /// Local snapshots replayed by a rebase.
    #[serde(default)]
    pub replayed: usize,
    /// Branch tip before / after the sync.
    pub old_tip: Option<String>,
    pub new_tip: Option<String>,
    pub push: Option<PushReport>,
    /// The outbox entry when the push was queued.
    pub queued: Option<OutboxEntry>,
    #[serde(default)]
    pub conflicts: Vec<ConflictInfo>,
    /// Why the server could not be reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_error: Option<String>,
}

/// Result of [`sync_all`] for one tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeSync {
    pub tree: String,
    pub report: Option<SyncReport>,
    pub error: Option<String>,
}

/// One delivered outbox entry.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeliveredPush {
    pub entry: OutboxEntry,
    pub push: PushReport,
}

/// One outbox entry that could not be delivered.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailedPush {
    pub entry: OutboxEntry,
    pub error: String,
    /// The server was unreachable (the entry is retried later).
    pub offline: bool,
}

/// Result of [`flush_outbox`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlushReport {
    pub tree: String,
    pub delivered: Vec<DeliveredPush>,
    pub failed: Vec<FailedPush>,
    /// Entries still queued afterwards.
    pub remaining: usize,
}

impl FlushReport {
    /// Whether some remote was unreachable.
    pub fn offline(&self) -> bool {
        self.failed.iter().any(|f| f.offline)
    }
}

/// Result of [`push_or_queue`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PushOrQueued {
    pub push: Option<PushReport>,
    pub queued: Option<OutboxEntry>,
    /// Why the push was queued.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_error: Option<String>,
}

/// Whether `err` means the server could not be reached at all (as opposed
/// to an answer such as a rejection or an auth failure).
pub fn is_unreachable(err: &SdkError) -> bool {
    let SdkError::Remote(m) = err else {
        return false;
    };
    let m = m.to_ascii_lowercase();
    m.starts_with("cannot connect to")
        || m.contains(" is unavailable")
        || m.contains("timed out")
        || m.contains("transport error")
        || m.contains("connection refused")
        || m.contains("connection reset")
        || m.contains("broken pipe")
}

/// Fetch, integrate and push the current branch of `tree` (see the module
/// docs). Refused while a merge, cherry-pick or rebase is in progress.
pub async fn sync(
    repo: &Repository,
    tree: &str,
    remote: &str,
    opts: SyncOptions,
) -> Result<SyncReport> {
    let tree_name = repo.tree(tree)?.name;
    if repo.rewrite_state(&tree_name)?.is_some() || repo.merge_state(&tree_name)?.is_some() {
        return Err(SdkError::Other(
            "a merge or rebase is in progress; resolve the conflicts and run \
             `wt sync --continue` (or `wt sync --abort`)"
                .into(),
        ));
    }
    let strategy = opts.strategy.unwrap_or(repo.config().sync.strategy);
    let branch = repo.current_branch(&tree_name)?;
    let old_tip = repo.branch_tip(&tree_name, &branch)?;
    let mut report = SyncReport {
        tree: tree_name.clone(),
        branch: branch.clone(),
        remote: remote.to_string(),
        strategy,
        status: SyncStatus::UpToDate,
        fetch: None,
        integration: SyncIntegration::None,
        replayed: 0,
        old_tip: old_tip.clone(),
        new_tip: old_tip.clone(),
        push: None,
        queued: None,
        conflicts: Vec::new(),
        offline_error: None,
    };

    // 1. Fetch.
    let fetched = async {
        let mut session = session::open(repo, &tree_name, remote, Access::Read).await?;
        fetch_with(
            repo,
            &tree_name,
            remote,
            &mut session,
            FetchOptions::default(),
        )
        .await
        .map(|(f, _)| f)
    }
    .await;
    match fetched {
        Ok(f) => report.fetch = Some(f),
        // Not on the server yet: the push below creates it.
        Err(SdkError::TreeNotFound(_)) => {}
        Err(e) if is_unreachable(&e) => {
            go_offline(repo, &mut report, &e, opts)?;
            return Ok(report);
        }
        Err(e) => return Err(e),
    }

    // 2. Integrate.
    let remote_tip = repo.remote_tip(&tree_name, remote, &branch)?;
    let plan = plan_integration(old_tip.as_deref(), remote_tip.as_deref(), |a, d| {
        repo.is_ancestor(a, d)
    })?;
    report.integration = SyncIntegration::UpToDate;
    if plan != Integration::UpToDate {
        let source = format!("{remote}/{branch}");
        let rebase = strategy == SyncStrategy::Rebase && old_tip.is_some();
        if rebase {
            let outcome = repo.rebase(&tree_name, &source, &RebaseOptions::default())?;
            report.replayed = outcome.picked.len();
            report.integration = match outcome.status {
                RewriteStatus::UpToDate => SyncIntegration::UpToDate,
                RewriteStatus::FastForward => SyncIntegration::FastForward,
                RewriteStatus::Completed => SyncIntegration::Rebased,
                RewriteStatus::Conflicts => {
                    report.conflicts = outcome.conflicts;
                    SyncIntegration::Conflicts
                }
            };
        } else {
            let merge = MergeOptions {
                message: Some(format!("Merge {source} into {branch}")),
                ..MergeOptions::default()
            };
            report.integration = match repo.merge(&tree_name, &source, merge)? {
                MergeOutcome::UpToDate => SyncIntegration::UpToDate,
                MergeOutcome::FastForward { .. } => SyncIntegration::FastForward,
                MergeOutcome::Merged { .. } => SyncIntegration::Merged,
                MergeOutcome::Conflicts { conflicts } => {
                    report.conflicts = conflicts;
                    SyncIntegration::Conflicts
                }
            };
        }
        if report.integration == SyncIntegration::Conflicts {
            if opts.abort_on_conflict {
                if rebase {
                    repo.rebase_abort(&tree_name)?;
                } else {
                    repo.abort_merge(&tree_name)?;
                }
                report.integration = SyncIntegration::Aborted;
                report.status = SyncStatus::Aborted;
            } else {
                report.status = SyncStatus::Conflicts;
            }
            report.new_tip = repo.branch_tip(&tree_name, &branch)?;
            return Ok(report);
        }
    }

    // 3. Push.
    finish_push(repo, &mut report, opts).await?;
    Ok(report)
}

/// Finish a sync that stopped with conflicts: complete the merge (`wt merge
/// --continue`) or the rebase (`wt rebase --continue`) once the conflicts
/// are resolved, then push. A rebase may stop again at a later snapshot.
pub async fn sync_continue(
    repo: &Repository,
    tree: &str,
    remote: &str,
    opts: SyncOptions,
) -> Result<SyncReport> {
    let tree_name = repo.tree(tree)?.name;
    let branch = repo.current_branch(&tree_name)?;
    let mut report = SyncReport {
        tree: tree_name.clone(),
        branch: branch.clone(),
        remote: remote.to_string(),
        strategy: SyncStrategy::Merge,
        status: SyncStatus::Synced,
        fetch: None,
        integration: SyncIntegration::Merged,
        replayed: 0,
        old_tip: None,
        new_tip: None,
        push: None,
        queued: None,
        conflicts: Vec::new(),
        offline_error: None,
    };
    if let Some(state) = repo.rewrite_state(&tree_name)? {
        if state.op != RewriteOp::Rebase {
            return Err(SdkError::Other(format!(
                "a {0} is in progress; finish it with `wt {0} --continue`",
                state.op.as_str()
            )));
        }
        report.strategy = SyncStrategy::Rebase;
        report.old_tip = state.orig_tip;
        let outcome = repo.rebase_continue(&tree_name)?;
        report.replayed = outcome.picked.len();
        report.integration = SyncIntegration::Rebased;
        if outcome.status == RewriteStatus::Conflicts {
            report.integration = SyncIntegration::Conflicts;
            report.status = SyncStatus::Conflicts;
            report.conflicts = outcome.conflicts;
            report.new_tip = repo.branch_tip(&tree_name, &branch)?;
            return Ok(report);
        }
    } else if let Some(state) = repo.merge_state(&tree_name)? {
        report.old_tip = state.target_tip;
        repo.continue_merge(&tree_name, None)?;
    } else {
        return Err(SdkError::InvalidArgument(
            "no sync to continue (no merge or rebase is in progress); run `wt sync`".into(),
        ));
    }
    finish_push(repo, &mut report, opts).await?;
    Ok(report)
}

/// Abandon a sync that stopped with conflicts (the merge or rebase is
/// aborted; the branch and working tree return to their state before it).
/// Returns which operation was aborted.
pub fn sync_abort(repo: &Repository, tree: &str) -> Result<SyncStrategy> {
    let tree_name = repo.tree(tree)?.name;
    if let Some(state) = repo.rewrite_state(&tree_name)? {
        if state.op != RewriteOp::Rebase {
            return Err(SdkError::Other(format!(
                "a {0} is in progress; abort it with `wt {0} --abort`",
                state.op.as_str()
            )));
        }
        repo.rebase_abort(&tree_name)?;
        return Ok(SyncStrategy::Rebase);
    }
    if repo.merge_state(&tree_name)?.is_some() {
        repo.abort_merge(&tree_name)?;
        return Ok(SyncStrategy::Merge);
    }
    Err(SdkError::InvalidArgument(
        "no sync to abort (no merge or rebase is in progress)".into(),
    ))
}

/// [`sync`] every tree of the worktree (`wt sync --all-trees`), each on its
/// current branch. Errors are reported per tree; the others still sync.
pub async fn sync_all(repo: &Repository, remote: &str, opts: SyncOptions) -> Result<Vec<TreeSync>> {
    let mut out = Vec::new();
    for info in repo.trees()? {
        let result = sync(repo, &info.name, remote, opts).await;
        out.push(match result {
            Ok(report) => TreeSync {
                tree: info.name,
                report: Some(report),
                error: None,
            },
            Err(e) => TreeSync {
                tree: info.name,
                report: None,
                error: Some(e.to_string()),
            },
        });
    }
    Ok(out)
}

/// Push `branch` (default: current); when the server is unreachable, queue
/// the push in the outbox instead of failing (`wt push --queue`).
pub async fn push_or_queue(
    repo: &Repository,
    tree: &str,
    remote: &str,
    branch: Option<&str>,
) -> Result<PushOrQueued> {
    let tree_name = repo.tree(tree)?.name;
    let branch = match branch {
        Some(b) => b.to_string(),
        None => repo.current_branch(&tree_name)?,
    };
    match push::push(repo, &tree_name, remote, Some(&branch)).await {
        Ok(report) => {
            clear_delivered(repo, &tree_name, remote, &branch)?;
            Ok(PushOrQueued {
                push: Some(report),
                queued: None,
                offline_error: None,
            })
        }
        Err(e) if is_unreachable(&e) => Ok(PushOrQueued {
            push: None,
            queued: Some(repo.outbox_queue(&tree_name, remote, &branch)?),
            offline_error: Some(e.to_string()),
        }),
        Err(e) => Err(e),
    }
}

/// Retry every queued push of `tree`, oldest first (`wt sync flush`).
/// Delivered entries are removed; failed ones stay with the attempt
/// recorded. Once a remote is found unreachable its other entries are not
/// tried again in this pass.
pub async fn flush_outbox(repo: &Repository, tree: &str) -> Result<FlushReport> {
    let tree_name = repo.tree(tree)?.name;
    let mut report = FlushReport {
        tree: tree_name.clone(),
        delivered: Vec::new(),
        failed: Vec::new(),
        remaining: 0,
    };
    let mut unreachable: Vec<String> = Vec::new();
    for entry in repo.outbox(&tree_name)? {
        if unreachable.contains(&entry.remote) {
            report.failed.push(FailedPush {
                error: format!("{} is unreachable", entry.remote),
                entry,
                offline: true,
            });
            continue;
        }
        match push::push(repo, &tree_name, &entry.remote, Some(&entry.branch)).await {
            Ok(push) => {
                repo.outbox_remove(&tree_name, &entry.id)?;
                report.delivered.push(DeliveredPush { entry, push });
            }
            Err(e) => {
                let offline = is_unreachable(&e);
                if offline {
                    unreachable.push(entry.remote.clone());
                }
                let error = e.to_string();
                repo.outbox_record_attempt(&tree_name, &entry.id, Some(&error))?;
                report.failed.push(FailedPush {
                    entry,
                    error,
                    offline,
                });
            }
        }
    }
    report.remaining = repo.outbox(&tree_name)?.len();
    Ok(report)
}

/// The server could not be reached before anything was fetched: queue the
/// push when the branch has unpushed snapshots.
fn go_offline(
    repo: &Repository,
    report: &mut SyncReport,
    err: &SdkError,
    opts: SyncOptions,
) -> Result<()> {
    report.offline_error = Some(err.to_string());
    report.integration = SyncIntegration::None;
    let remote_tip = repo.remote_tip(&report.tree, &report.remote, &report.branch)?;
    let ahead = report.old_tip.is_some() && report.old_tip != remote_tip;
    if ahead && !opts.no_push {
        report.queued = Some(repo.outbox_queue(&report.tree, &report.remote, &report.branch)?);
        report.status = SyncStatus::Queued;
    } else {
        report.status = SyncStatus::Offline;
    }
    Ok(())
}

/// Push the branch when it is ahead of the remote, queueing it when the
/// server went away, then settle `report.status`.
async fn finish_push(repo: &Repository, report: &mut SyncReport, opts: SyncOptions) -> Result<()> {
    let tip = repo.branch_tip(&report.tree, &report.branch)?;
    report.new_tip = tip.clone();
    let remote_tip = repo.remote_tip(&report.tree, &report.remote, &report.branch)?;
    if !opts.no_push && tip.is_some() && tip != remote_tip {
        match push::push(repo, &report.tree, &report.remote, Some(&report.branch)).await {
            Ok(p) => {
                clear_delivered(repo, &report.tree, &report.remote, &report.branch)?;
                report.push = Some(p);
            }
            Err(e) if is_unreachable(&e) => {
                report.offline_error = Some(e.to_string());
                report.queued =
                    Some(repo.outbox_queue(&report.tree, &report.remote, &report.branch)?);
            }
            Err(e) => return Err(e),
        }
    } else if !opts.no_push && tip.is_some() {
        // Already on the server (e.g. delivered by another client).
        clear_delivered(repo, &report.tree, &report.remote, &report.branch)?;
    }
    report.status = settle(report);
    Ok(())
}

fn settle(report: &SyncReport) -> SyncStatus {
    if report.queued.is_some() {
        return SyncStatus::Queued;
    }
    let fetched = report.fetch.as_ref().is_some_and(|f| !f.updated.is_empty());
    let integrated = !matches!(
        report.integration,
        SyncIntegration::None | SyncIntegration::UpToDate
    );
    let pushed = report.push.as_ref().is_some_and(|p| !p.up_to_date);
    if fetched || integrated || pushed {
        SyncStatus::Synced
    } else {
        SyncStatus::UpToDate
    }
}

/// Drop outbox entries for `remote`/`branch`: the branch was just pushed.
fn clear_delivered(repo: &Repository, tree: &str, remote: &str, branch: &str) -> Result<()> {
    for entry in repo.outbox(tree)? {
        if entry.remote == remote && entry.branch == branch {
            repo.outbox_remove(tree, &entry.id)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unreachable_errors_are_recognized() {
        for m in [
            "cannot connect to http://127.0.0.1:1: transport error: connection refused",
            "server http://x is unavailable: fake server is offline",
            "request to http://x timed out: deadline",
            "h2 protocol error: connection reset by peer",
        ] {
            assert!(is_unreachable(&SdkError::Remote(m.into())), "{m}");
        }
        for e in [
            SdkError::Remote("snapshot not found (NotFound)".into()),
            SdkError::PushRejected("behind".into()),
            SdkError::NotLoggedIn("http://x".into()),
            SdkError::PermissionDenied("nope".into()),
        ] {
            assert!(!is_unreachable(&e), "{e}");
        }
    }

    fn report() -> SyncReport {
        SyncReport {
            tree: "t".into(),
            branch: "main".into(),
            remote: "origin".into(),
            strategy: SyncStrategy::Merge,
            status: SyncStatus::UpToDate,
            fetch: None,
            integration: SyncIntegration::UpToDate,
            replayed: 0,
            old_tip: None,
            new_tip: None,
            push: None,
            queued: None,
            conflicts: Vec::new(),
            offline_error: None,
        }
    }

    #[test]
    fn status_reflects_what_happened() {
        let r = report();
        assert_eq!(settle(&r), SyncStatus::UpToDate);
        let mut r2 = report();
        r2.integration = SyncIntegration::FastForward;
        assert_eq!(settle(&r2), SyncStatus::Synced);
        let mut r3 = report();
        r3.fetch = Some(FetchReport {
            tree: "t".into(),
            updated: vec![("dev".into(), None, Some("x".into()))],
            snapshots_received: 1,
            objects_received: 0,
            tags_received: 0,
        });
        assert_eq!(settle(&r3), SyncStatus::Synced);
        let mut r4 = report();
        r4.push = Some(PushReport {
            tree: "t".into(),
            branch: "main".into(),
            old_tip: None,
            new_tip: "x".into(),
            snapshots_sent: 1,
            objects_sent: 1,
            up_to_date: false,
            forced: false,
        });
        assert_eq!(settle(&r4), SyncStatus::Synced);
        let json = serde_json::to_value(&r4).unwrap();
        assert_eq!(json["status"], "up_to_date");
        assert_eq!(json["strategy"], "merge");
        assert_eq!(json["integration"], "up_to_date");
    }
}
