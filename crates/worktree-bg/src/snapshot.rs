//! The real auto-snapshot pass: snapshot every dirty tree, then stage.

use std::future::Future;
use std::path::PathBuf;

use worktree_sdk::config::WorktreeConfig;
use worktree_sdk::{remote, Repository, SdkError, Snapshot, SnapshotOptions};

use crate::worker::{SnapshotAction, TriggerReport};

/// Snapshots (and optionally stages) all trees of the worktree at `root`.
/// The repository is reopened on every pass so config edits apply.
#[derive(Debug, Clone)]
pub struct RepoSnapshotter {
    root: PathBuf,
}

impl RepoSnapshotter {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

impl SnapshotAction for RepoSnapshotter {
    fn run(&self) -> impl Future<Output = TriggerReport> + Send {
        let root = self.root.clone();
        async move {
            let pass = tokio::task::spawn_blocking(move || snapshot_all(&root)).await;
            let (repo, mut report, trees) = match pass {
                Ok(Ok(pass)) => pass,
                Ok(Err(e)) => return TriggerReport::failed(e.to_string()),
                Err(e) => return TriggerReport::failed(format!("snapshot task failed: {e}")),
            };
            if !trees.is_empty() {
                stage_trees(&repo, &trees, &mut report).await;
            }
            report
        }
    }
}

/// The auto-snapshot message for a tree with `changed` changed files.
pub fn auto_message(changed: usize) -> String {
    format!("auto: {changed} files changed")
}

/// Snapshot every tree with working changes. Returns the open repository,
/// the report so far, and the names of the trees that got a snapshot.
fn snapshot_all(
    root: &std::path::Path,
) -> worktree_sdk::Result<(Repository, TriggerReport, Vec<String>)> {
    let repo = Repository::open(root)?;
    let mut report = TriggerReport::default();
    let mut snapshotted = Vec::new();
    for tree in repo.trees()? {
        match snapshot_tree(&repo, &tree.name) {
            Ok(Some(snapshot)) => {
                tracing::info!(tree = %tree.name, id = %snapshot.short_id(), message = %snapshot.message, "auto-snapshot created");
                report.snapshots.push(snapshot.id);
                snapshotted.push(tree.name);
            }
            Ok(None) => {}
            Err(e) => report.errors.push(format!("{}: {e}", tree.name)),
        }
    }
    Ok((repo, report, snapshotted))
}

/// Snapshot one tree if it has changes and no merge is in progress.
fn snapshot_tree(repo: &Repository, tree: &str) -> worktree_sdk::Result<Option<Snapshot>> {
    let status = repo.status(tree)?;
    if status.merge.is_some() {
        tracing::debug!(tree, "merge in progress; skipping auto-snapshot");
        return Ok(None);
    }
    if status.is_clean() {
        return Ok(None);
    }
    let opts = SnapshotOptions {
        message: auto_message(status.changes.len()),
        auto_generated: true,
        ..SnapshotOptions::default()
    };
    match repo.snapshot(tree, opts) {
        Ok(snapshot) => Ok(Some(snapshot)),
        Err(SdkError::NoChanges) => Ok(None),
        Err(e) => Err(e),
    }
}

/// Remote name and server URL to stage to, if auto-staging is configured.
pub fn stage_target(config: &WorktreeConfig) -> Option<(String, String)> {
    if !config.sync.auto_stage {
        return None;
    }
    let remote = config.remote.get(&config.sync.remote)?;
    Some((
        config.sync.remote.clone(),
        remote.url.trim_end_matches('/').to_string(),
    ))
}

async fn stage_trees(repo: &Repository, trees: &[String], report: &mut TriggerReport) {
    let Some((remote_name, url)) = stage_target(repo.config()) else {
        return;
    };
    match remote::load_credential(&url) {
        Ok(Some(_)) => {}
        Ok(None) => {
            tracing::debug!(%url, "not logged in; skipping auto-stage");
            return;
        }
        Err(e) => {
            report
                .errors
                .push(format!("loading credentials for {url}: {e}"));
            return;
        }
    }
    for tree in trees {
        match remote::stage(repo, tree, &remote_name).await {
            Ok(staged) => {
                tracing::info!(%tree, snapshot = %staged.snapshot_id, "staged on {remote_name}");
                report.staged += 1;
            }
            Err(e) => report
                .errors
                .push(format!("stage {tree} to {remote_name}: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use worktree_sdk::config::{
        AutoSnapshotSection, RemoteSection, SyncSection, UserSection, WorktreeSection,
    };

    fn config(auto_stage: bool, remote: Option<&str>) -> WorktreeConfig {
        let mut remotes = std::collections::BTreeMap::new();
        if let Some(url) = remote {
            remotes.insert(
                "origin".to_string(),
                RemoteSection {
                    url: url.into(),
                    tenant: "acme".into(),
                },
            );
        }
        WorktreeConfig {
            worktree: WorktreeSection {
                name: "w".into(),
                id: "id".into(),
                created_at: "now".into(),
                default_branch: "main".into(),
            },
            user: UserSection::default(),
            auto_snapshot: AutoSnapshotSection::default(),
            sync: SyncSection {
                auto_stage,
                remote: "origin".into(),
                ..SyncSection::default()
            },
            remote: remotes,
            secret_scanning: Default::default(),
            hooks: Default::default(),
            signing: Default::default(),
        }
    }

    #[test]
    fn message_format_matches_contract() {
        assert_eq!(auto_message(3), "auto: 3 files changed");
    }

    #[test]
    fn stage_target_requires_auto_stage_and_remote() {
        assert_eq!(stage_target(&config(false, Some("http://h:7420"))), None);
        assert_eq!(stage_target(&config(true, None)), None);
        assert_eq!(
            stage_target(&config(true, Some("http://h:7420/"))),
            Some(("origin".to_string(), "http://h:7420".to_string()))
        );
    }

    #[test]
    fn stage_target_uses_configured_remote_name() {
        let mut cfg = config(true, Some("http://h:7420"));
        cfg.sync.remote = "backup".into();
        assert_eq!(stage_target(&cfg), None);
        cfg.remote.insert(
            "backup".into(),
            RemoteSection {
                url: "http://b".into(),
                tenant: "t".into(),
            },
        );
        assert_eq!(
            stage_target(&cfg),
            Some(("backup".to_string(), "http://b".to_string()))
        );
    }
}
