//! `wt status`

use clap::Args;
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::remote::{self, StagedInfo};
use worktree_sdk::{ChangeKind, MergeState, Repository, StatusReport};

use crate::commands::{merge, remote::remote_name};
use crate::context::{block_on, Context};
use crate::error::Result;
use crate::output::format::{ago, plural, short, subject};
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

pub fn run(ctx: &Context, args: StatusArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let report = repo.status(&tree.name)?;
    let head_line = if ctx.out.is_json() {
        String::new()
    } else {
        head_summary(&repo, &report)?
    };

    // daemon workstream (§13.11): hint when auto-snapshot is on but the
    // daemon is not running / not watching this worktree (text mode only).
    let daemon_hint = if ctx.out.is_json() {
        None
    } else {
        crate::commands::daemon::status_hint(&repo)
    };
    let render = |t: &mut Text, report: &StatusReport, head_line: &str| {
        render(t, report, head_line);
        if let Some(hint) = &daemon_hint {
            t.blank();
            t.line(hint);
        }
    };

    if !args.team {
        return ctx.out.emit(&report, |t| render(t, &report, &head_line));
    }

    let remote = remote_name(&repo, None);
    let staged = block_on(remote::list_staged(&repo, &tree.name, &remote, None))??;
    let result = TeamStatus {
        status: &report,
        remote: &remote,
        staged: &staged,
    };
    ctx.out.emit(&result, |t| {
        render(t, &report, &head_line);
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

fn render(t: &mut Text, report: &StatusReport, head_line: &str) {
    t.line(format_args!(
        "On tree {}, branch {}",
        report.tree.bold(),
        report.branch.green().bold()
    ));
    t.line(head_line);
    if let Some(state) = &report.merge {
        t.blank();
        render_merge(t, state);
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
