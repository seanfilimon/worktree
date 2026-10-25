//! Status, snapshot creation and checkout.

use std::cmp::Ordering;
use std::collections::HashSet;

use chrono::Utc;

use super::trees::TreeRecord;
use super::Repository;
use crate::error::{Result, SdkError};
use crate::model::*;

/// Maximum number of secret findings listed in a `SecretsDetected` error.
const MAX_LISTED_FINDINGS: usize = 20;

/// Where the parents and bookkeeping of a new snapshot come from.
pub(crate) enum CommitKind<'a> {
    /// Ordinary snapshot on top of the current head.
    Normal,
    /// Completes a conflicted merge (second parent = merge source).
    MergeCompletion(&'a MergeState),
}

impl Repository {
    // ----------------------------------------------------------------
    // Status & snapshots
    // ----------------------------------------------------------------

    /// Compare the working directory with the tree's head snapshot.
    pub fn status(&self, tree: &str) -> Result<StatusReport> {
        let rec = self.load_tree(tree)?;
        let (branch, head) = self.head_tip(tree)?;
        let head_manifest = self.manifest_of_tip(head.as_deref())?;
        let working = self.build_working(&rec, &head_manifest, false, false)?;
        // The index is only a cache; failing to refresh it must not fail status.
        if let Err(err) = self.save_index(tree, &working.index) {
            tracing::warn!(tree, %err, "could not refresh index");
        }
        Ok(StatusReport {
            tree: tree.to_string(),
            branch,
            head,
            changes: compute_changes(&head_manifest, &working.manifest),
            merge: self.merge_state(tree)?,
        })
    }

    /// Snapshot the tree's working state onto its current branch. Runs the
    /// secret scan on added/modified files unless `skip_secret_scan`
    /// (`SecretsDetected`). `NoChanges` if identical to head and not
    /// `allow_empty`. Refuses during a conflicted merge unless all conflict
    /// markers are gone (then it completes the merge with two parents).
    pub fn snapshot(&self, tree: &str, opts: SnapshotOptions) -> Result<Snapshot> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        match self.merge_state(tree)? {
            Some(state) => {
                if !self.unresolved_conflicts(&rec, &state)?.is_empty() {
                    return Err(SdkError::MergeInProgress);
                }
                self.commit_working(&rec, &opts, CommitKind::MergeCompletion(&state))
            }
            None => self.commit_working(&rec, &opts, CommitKind::Normal),
        }
    }

    /// Rewrite the tree's working directory to `snapshot` (removing tracked
    /// files absent from it, never touching ignored or nested-tree paths) and
    /// refresh the index. `DirtyWorkingTree` unless clean or `force`.
    /// Untracked files are kept. Branch refs are not moved.
    pub fn checkout(&self, tree: &str, snapshot: &str, force: bool) -> Result<()> {
        let _lock = self.lock_tree(tree)?;
        let rec = self.load_tree(tree)?;
        let target = self.resolve(tree, snapshot)?;
        let target_manifest = self.manifest_of(&target)?;
        let head = self.manifest_of_tip(self.head_tip(tree)?.1.as_deref())?;
        let mut working = self.build_working(&rec, &head, false, false)?;
        let changes = compute_changes(&head, &working.manifest);
        if !changes.is_empty() && !force {
            return Err(SdkError::DirtyWorkingTree(changes.len()));
        }
        let tracked: HashSet<String> = head.entries.iter().map(|e| e.path.clone()).collect();
        self.apply_manifest(
            &rec,
            &target_manifest,
            &tracked,
            &working.manifest,
            &mut working.index,
        )?;
        self.save_index(tree, &working.index)
    }

    /// Create a snapshot from the working directory (caller holds the lock).
    pub(crate) fn commit_working(
        &self,
        rec: &TreeRecord,
        opts: &SnapshotOptions,
        kind: CommitKind<'_>,
    ) -> Result<Snapshot> {
        let tree = rec.name.as_str();
        let (branch, head) = self.head_tip(tree)?;
        // local-config (§13.9): `skip_hooks` = `wt snapshot --no-verify`.
        if !opts.skip_hooks {
            crate::hooks::run(self, rec, crate::hooks::Phase::PreSnapshot, &branch, None)?;
        }
        let head_manifest = self.manifest_of_tip(head.as_deref())?;
        let working = self.build_working(rec, &head_manifest, true, !opts.skip_secret_scan)?;
        if !working.findings.is_empty() {
            self.save_index(tree, &working.index)?;
            return Err(SdkError::SecretsDetected(describe_findings(
                &working.findings,
            )));
        }

        let (parents, message, action) = match kind {
            CommitKind::Normal => {
                if working.manifest == head_manifest && !opts.allow_empty {
                    self.save_index(tree, &working.index)?;
                    return Err(SdkError::NoChanges);
                }
                let action = if opts.auto_generated {
                    "auto-snapshot"
                } else {
                    "snapshot"
                };
                (
                    head.iter().cloned().collect::<Vec<_>>(),
                    opts.message.clone(),
                    action,
                )
            }
            CommitKind::MergeCompletion(state) => {
                let mut parents: Vec<SnapshotId> = state.target_tip.iter().cloned().collect();
                parents.push(state.source_tip.clone());
                let message = if opts.message.trim().is_empty() {
                    state.message.clone()
                } else {
                    opts.message.clone()
                };
                (parents, message, "merge")
            }
        };

        let manifest_hash = self.write_manifest(&working.manifest)?;
        let mut snapshot = Snapshot {
            id: uuid::Uuid::new_v4().to_string(),
            tree_id: rec.id.clone(),
            branch: branch.clone(),
            parents,
            manifest: manifest_hash,
            author: opts.author.clone().unwrap_or_else(|| self.author()),
            message,
            created_at: Utc::now(),
            auto_generated: opts.auto_generated,
            dependencies: opts.dependencies.clone(),
            reverts: None,
            signature: None,
        };
        crate::signing::sign_snapshot(self, &mut snapshot)?;
        self.write_snapshot(&snapshot)?;
        self.write_tip(tree, &branch, Some(&snapshot.id))?;
        self.save_index(tree, &working.index)?;
        if matches!(kind, CommitKind::MergeCompletion(_)) {
            self.clear_merge_state(tree)?;
        }
        self.append_reflog(&ReflogEntry {
            at: snapshot.created_at,
            tree: tree.to_string(),
            branch,
            action: action.into(),
            old: head,
            new: Some(snapshot.id.clone()),
            message: snapshot
                .message
                .lines()
                .next()
                .unwrap_or_default()
                .to_string(),
        })?;
        if !opts.skip_hooks {
            crate::hooks::run(
                self,
                rec,
                crate::hooks::Phase::PostSnapshot,
                &snapshot.branch,
                Some(&snapshot),
            )?;
        }
        Ok(snapshot)
    }
}

/// Path-sorted changes turning `old` into `new`. An executable-bit flip
/// counts as a modification.
pub(crate) fn compute_changes(old: &Manifest, new: &Manifest) -> Vec<FileChange> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    let (a, b) = (&old.entries, &new.entries);
    while i < a.len() || j < b.len() {
        let order = match (a.get(i), b.get(j)) {
            (Some(x), Some(y)) => x.path.cmp(&y.path),
            (Some(_), None) => Ordering::Less,
            (None, _) => Ordering::Greater,
        };
        match order {
            Ordering::Less => {
                out.push(FileChange {
                    path: a[i].path.clone(),
                    kind: ChangeKind::Deleted,
                });
                i += 1;
            }
            Ordering::Greater => {
                out.push(FileChange {
                    path: b[j].path.clone(),
                    kind: ChangeKind::Added,
                });
                j += 1;
            }
            Ordering::Equal => {
                if a[i].hash != b[j].hash || a[i].executable != b[j].executable {
                    out.push(FileChange {
                        path: a[i].path.clone(),
                        kind: ChangeKind::Modified,
                    });
                }
                i += 1;
                j += 1;
            }
        }
    }
    out
}

fn describe_findings(findings: &[crate::scan::SecretFinding]) -> String {
    let mut parts: Vec<String> = findings
        .iter()
        .take(MAX_LISTED_FINDINGS)
        .map(|f| format!("{}:{} [{}] {}", f.path, f.line, f.rule, f.excerpt))
        .collect();
    if findings.len() > MAX_LISTED_FINDINGS {
        parts.push(format!(
            "... and {} more",
            findings.len() - MAX_LISTED_FINDINGS
        ));
    }
    format!(
        "{} finding(s): {} (add `wt:allow-secret` on the line or use --no-scan to override)",
        findings.len(),
        parts.join("; ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::hash_bytes;

    fn m(entries: &[(&str, &[u8], bool)]) -> Manifest {
        Manifest::from_entries(
            entries
                .iter()
                .map(|(p, c, x)| ManifestEntry {
                    path: (*p).into(),
                    hash: hash_bytes(c),
                    size: c.len() as u64,
                    executable: *x,
                    chunked: false,
                })
                .collect(),
        )
    }

    #[test]
    fn changes_between_manifests() {
        let old = m(&[("a", b"1", false), ("b", b"2", false), ("d", b"4", false)]);
        let new = m(&[("b", b"2x", false), ("c", b"3", false), ("d", b"4", true)]);
        let changes = compute_changes(&old, &new);
        assert_eq!(
            changes,
            vec![
                FileChange {
                    path: "a".into(),
                    kind: ChangeKind::Deleted
                },
                FileChange {
                    path: "b".into(),
                    kind: ChangeKind::Modified
                },
                FileChange {
                    path: "c".into(),
                    kind: ChangeKind::Added
                },
                FileChange {
                    path: "d".into(),
                    kind: ChangeKind::Modified
                },
            ]
        );
        assert!(compute_changes(&old, &old).is_empty());
    }

    #[test]
    fn findings_are_summarized() {
        let f = crate::scan::SecretFinding {
            rule: "aws-access-key".into(),
            path: "cfg.env".into(),
            line: 3,
            excerpt: "AKIA****".into(),
        };
        let many = vec![f.clone(); MAX_LISTED_FINDINGS + 2];
        let text = describe_findings(&many);
        assert!(text.starts_with("22 finding(s): cfg.env:3 [aws-access-key]"));
        assert!(text.contains("and 2 more"));
    }
}
