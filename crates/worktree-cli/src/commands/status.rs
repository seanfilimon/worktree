//! `wt status`

use chrono::{DateTime, Utc};
use clap::Args;
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::remote::{self, StagedInfo};
use worktree_sdk::repo::{OutboxEntry, SparseStatus};
use worktree_sdk::{ChangeKind, MergeState, Repository, StatusReport};

use crate::commands::{merge, remote::remote_name};
use crate::context::{block_on, Context};
use crate::error::Result;
use crate::output::format::{ago, plural, relative_time, short, subject};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct StatusArgs {
    /// Also show the team's staged work in progress from the remote
    #[arg(long)]
    pub team: bool,
}

#[derive(Serialize)]
struct TeamStatus<'a> {
    status: &'a StatusReport,
    remote: &'a str,
    staged: &'a [StagedInfo],
}

/// `StatusReport` plus the partial-copy state (ENGINEERING_PLAN §13.2).
#[derive(Serialize)]
struct LocalStatus<'a> {
    #[serde(flatten)]
    status: &'a StatusReport,
    /// Files whose content the server withheld (not checked out).
    restricted: &'a [String],
    /// The local history is shallow.
    shallow: bool,
    /// sparse (§15.4): sparse checkout state (absent when off).
    #[serde(skip_serializing_if = "Option::is_none")]
    sparse: Option<SparseStatus>,
    /// server-ops (§16.11): pushes queued offline (`wt sync queue`),
    /// oldest first (absent when none).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    outbox: Vec<OutboxEntry>,
}

pub fn run(ctx: &Context, args: StatusArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let report = repo.status(&tree.name)?;
    let head_line = if ctx.out.is_json() {
        String::new()
    } else {
        head_summary(&repo, &report)?
    };
    let restricted = repo.restricted_paths(&tree.name)?;
    let local = LocalStatus {
        status: &report,
        restricted: &restricted,
        shallow: repo.is_shallow(&tree.name)?,
        sparse: repo.sparse_status(&tree.name)?,
        // An absent outbox file reads as empty.
        outbox: repo.outbox(&tree.name)?,
    };

    // daemon workstream (§13.11): hint when auto-snapshot is on but the
    // daemon is not running / not watching this worktree (text mode only).
    let daemon_hint = if ctx.out.is_json() {
        None
    } else {
        crate::commands::daemon::status_hint(&repo)
    };
    let render = |t: &mut Text, local: &LocalStatus<'_>, head_line: &str| {
        render(t, local, head_line);
        if let Some(hint) = &daemon_hint {
            t.blank();
            t.line(hint);
        }
    };

    if !args.team {
        return ctx.out.emit(&local, |t| render(t, &local, &head_line));
    }

    let remote = remote_name(&repo, None);
    let staged = block_on(remote::list_staged(&repo, &tree.name, &remote, None))??;
    let result = TeamStatus {
        status: &report,
        remote: &remote,
        staged: &staged,
    };
    ctx.out.emit(&result, |t| {
        render(t, &local, &head_line);
        t.blank();
        render_team(t, &remote, &staged);
    })
}

fn head_summary(repo: &Repository, report: &StatusReport) -> Result<String> {
    Ok(match &report.head {
        Some(id) => {
            let snap = repo.get_snapshot(id)?;
            format!(
                "HEAD {} {} ({})",
                snap.short_id().yellow(),
                subject(&snap.message),
                ago(snap.created_at)
            )
        }
        None => "No snapshots yet".to_string(),
    })
}

fn render(t: &mut Text, local: &LocalStatus<'_>, head_line: &str) {
    let report = local.status;
    t.line(format_args!(
        "On tree {}, branch {}",
        report.tree.bold(),
        report.branch.green().bold()
    ));
    t.line(head_line);
    if local.shallow {
        t.line("History is shallow; use `wt fetch --deepen N` for more.");
    }
    if let Some(sparse) = &local.sparse {
        t.line(format_args!(
            "Sparse checkout ({}): {}",
            sparse.patterns.join(" "),
            crate::commands::sparse::summary(sparse)
        ));
    }
    for line in outbox_lines(&local.outbox, Utc::now()) {
        t.line(line);
    }
    if let Some(state) = &report.merge {
        t.blank();
        render_merge(t, state);
    }
    if !local.restricted.is_empty() {
        t.blank();
        t.line(format_args!(
            "Restricted ({}; not checked out, the server withholds their content):",
            plural(local.restricted.len(), "file", "files")
        ));
        for path in local.restricted {
            t.line(format_args!("  {:<10}{}", "restricted:", path.dimmed()));
        }
    }
    t.blank();
    if report.is_clean() {
        t.line("nothing to snapshot, working tree clean");
        return;
    }
    t.line(format_args!(
        "Changes since the last snapshot ({}):",
        plural(report.changes.len(), "file", "files")
    ));
    for change in &report.changes {
        let (label, path) = match change.kind {
            ChangeKind::Added => ("added:", change.path.green()),
            ChangeKind::Modified => ("modified:", change.path.yellow()),
            ChangeKind::Deleted => ("deleted:", change.path.red()),
        };
        t.line(format_args!("  {label:<10}{path}"));
    }
}

/// Queued pushes listed on the summary line before "and N more".
const OUTBOX_SHOWN: usize = 3;

/// server-ops (§16.11): the pending offline pushes, e.g.
/// "2 pushes queued offline (oldest 3 hours ago): main -> origin, feature
/// -> origin; run `wt sync flush` to deliver them", plus the oldest entry's
/// last error (dimmed). Nothing when the outbox is empty.
fn outbox_lines(entries: &[OutboxEntry], now: DateTime<Utc>) -> Vec<String> {
    let Some(oldest) = entries.iter().min_by_key(|e| e.queued_at) else {
        return Vec::new();
    };
    let mut targets: Vec<String> = entries
        .iter()
        .take(OUTBOX_SHOWN)
        .map(|e| format!("{} -> {}", e.branch, e.remote))
        .collect();
    if entries.len() > OUTBOX_SHOWN {
        targets.push(format!("and {} more", entries.len() - OUTBOX_SHOWN));
    }
    let (age, them) = if entries.len() == 1 {
        ("", "it")
    } else {
        ("oldest ", "them")
    };
    let mut lines = vec![format!(
        "{} queued offline ({age}{}): {}; run `wt sync flush` to deliver {them}",
        plural(entries.len(), "push", "pushes").yellow(),
        relative_time(oldest.queued_at, now),
        targets.join(", ")
    )];
    if let Some(err) = &oldest.last_error {
        let attempts = plural(oldest.attempts as usize, "attempt", "attempts");
        lines.push(
            format!("  last error ({attempts}): {}", subject(err))
                .dimmed()
                .to_string(),
        );
    }
    lines
}

fn render_merge(t: &mut Text, state: &MergeState) {
    t.line(format_args!(
        "Merging {} ({} into this branch)",
        state.source_branch.bold(),
        short(&state.source_tip).yellow()
    ));
    if state.conflicts.is_empty() {
        t.line("All conflicts resolved; run `wt merge --continue` to finish the merge.");
        return;
    }
    t.line(format_args!(
        "Unresolved conflicts ({}):",
        state.conflicts.len()
    ));
    for conflict in &state.conflicts {
        t.line(format_args!(
            "  {:<18}{}",
            merge::conflict_label(conflict.kind),
            conflict.path.red()
        ));
    }
    t.line("Fix them, then run `wt merge --continue` (or `wt merge --abort`).");
}

fn render_team(t: &mut Text, remote: &str, staged: &[StagedInfo]) {
    if staged.is_empty() {
        t.line(format_args!("No staged work on {remote}."));
        return;
    }
    t.line(format_args!("Staged work on {remote}:"));
    let author_width = staged.iter().map(|s| s.author.len()).max().unwrap_or(0);
    let branch_width = staged.iter().map(|s| s.branch.len()).max().unwrap_or(0);
    for s in staged {
        let files = plural(s.files_changed.len(), "file", "files");
        let message = s.message.as_deref().map(subject).unwrap_or_default();
        t.line(format_args!(
            "  {}  {}  {:<9}  {:<16}  {}",
            format!("{:<author_width$}", s.author).cyan(),
            format!("{:<branch_width$}", s.branch).green(),
            files,
            ago(s.staged_at),
            message
        ));
        t.line(format_args!(
            "      {} {}  {} {}  {} {}",
            "+".green(),
            s.files_added,
            "~".yellow(),
            s.files_modified,
            "-".red(),
            s.files_deleted
        ));
    }
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    fn entry(branch: &str, hours_ago: i64, now: DateTime<Utc>) -> OutboxEntry {
        OutboxEntry {
            id: format!("id-{branch}"),
            remote: "origin".into(),
            branch: branch.into(),
            tip: "eeeeeeee55".into(),
            queued_at: now - Duration::hours(hours_ago),
            attempts: 0,
            last_attempt_at: None,
            last_error: None,
        }
    }

    #[test]
    fn outbox_summary_lines() {
        colored::control::set_override(false);
        let now = Utc::now();
        assert!(outbox_lines(&[], now).is_empty());

        assert_eq!(
            outbox_lines(&[entry("main", 1, now)], now),
            ["1 push queued offline (1 hour ago): main -> origin; run `wt sync flush` to deliver it"]
        );

        let mut oldest = entry("main", 3, now);
        oldest.attempts = 2;
        oldest.last_error = Some("cannot reach http://s: connection refused\nmore".into());
        let lines = outbox_lines(&[oldest, entry("feature", 1, now)], now);
        assert_eq!(
            lines,
            [
                "2 pushes queued offline (oldest 3 hours ago): main -> origin, feature -> origin; run `wt sync flush` to deliver them",
                "  last error (2 attempts): cannot reach http://s: connection refused",
            ]
        );

        let many: Vec<_> = ["a", "b", "c", "d", "e"]
            .iter()
            .map(|b| entry(b, 2, now))
            .collect();
        let lines = outbox_lines(&many, now);
        assert_eq!(lines.len(), 1);
        assert!(
            lines[0].contains(": a -> origin, b -> origin, c -> origin, and 2 more;"),
            "{}",
            lines[0]
        );
    }

    #[test]
    fn outbox_is_omitted_from_json_when_empty() {
        let report = StatusReport {
            tree: "web".into(),
            branch: "main".into(),
            head: None,
            changes: Vec::new(),
            merge: None,
        };
        let local = LocalStatus {
            status: &report,
            restricted: &[],
            shallow: false,
            sparse: None,
            outbox: Vec::new(),
        };
        let json = serde_json::to_value(&local).expect("json");
        assert!(json.get("outbox").is_none(), "{json}");
        let now = Utc::now();
        let local = LocalStatus {
            outbox: vec![entry("main", 1, now)],
            ..local
        };
        let json = serde_json::to_value(&local).expect("json");
        assert_eq!(json["outbox"][0]["branch"], "main", "{json}");
        assert_eq!(json["outbox"][0]["remote"], "origin", "{json}");
    }
}
