//! `wt merge`, `wt revert`, `wt restore`

use std::path::PathBuf;

use clap::{ArgGroup, Args};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::{ConflictInfo, ConflictKind, MergeOptions, MergeOutcome};

use crate::commands::snapshot::snapshot_line;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{plural, short};
use crate::output::Text;

#[derive(Debug, Args)]
#[command(group(ArgGroup::new("operation").required(true).args(["source", "resume", "abort"])))]
pub struct MergeArgs {
    /// Branch or revision to merge into the current branch
    pub source: Option<String>,

    /// Message for the merge snapshot
    #[arg(short, long, value_name = "MSG", conflicts_with = "abort")]
    pub message: Option<String>,

    /// Always create a merge snapshot, even when a fast-forward is possible
    #[arg(long, conflicts_with_all = ["resume", "abort"])]
    pub no_ff: bool,

    /// Finish a conflicted merge after resolving the conflicts
    #[arg(long = "continue")]
    pub resume: bool,

    /// Abandon a conflicted merge and restore the pre-merge state
    #[arg(long)]
    pub abort: bool,
}

#[derive(Debug, Args)]
pub struct RevertArgs {
    /// Snapshot whose changes to undo
    #[arg(value_name = "REV")]
    pub rev: String,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// Paths to restore (default: everything)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Revision to restore from (default: HEAD)
    #[arg(long, value_name = "REV")]
    pub from: Option<String>,
}

#[derive(Serialize)]
struct AbortResult<'a> {
    tree: &'a str,
    aborted: bool,
}

pub fn run(ctx: &Context, args: MergeArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    if args.abort {
        repo.abort_merge(&tree.name)?;
        let result = AbortResult {
            tree: &tree.name,
            aborted: true,
        };
        return ctx.out.emit(&result, |t| {
            t.line("Merge aborted; the pre-merge state was restored.")
        });
    }
    if args.resume {
        let snapshot = repo.continue_merge(&tree.name, args.message.as_deref())?;
        return ctx.out.emit(&snapshot, |t| snapshot_line(t, &snapshot));
    }
    let source = args
        .source
        .ok_or_else(|| CliError::usage("nothing to merge"))?;
    let opts = MergeOptions {
        message: args.message,
        no_fast_forward: args.no_ff,
        author: None,
    };
    let outcome = repo.merge(&tree.name, &source, opts)?;
    ctx.out.emit(&outcome, |t| render_outcome(t, &outcome))?;
    conflict_status(&outcome)
}

/// Human rendering of a merge (shared with `wt pull`).
pub fn render_outcome(t: &mut Text, outcome: &MergeOutcome) {
    match outcome {
        MergeOutcome::UpToDate => t.line("Already up to date."),
        MergeOutcome::FastForward { to } => {
            t.line(format_args!("Fast-forward to {}", short(to).yellow()))
        }
        MergeOutcome::Merged { snapshot } => {
            t.line("Merge made by the three-way strategy.");
            snapshot_line(t, snapshot);
        }
        MergeOutcome::Conflicts { conflicts } => render_conflicts(t, conflicts),
    }
}

fn render_conflicts(t: &mut Text, conflicts: &[ConflictInfo]) {
    for conflict in conflicts {
        t.line(format_args!(
            "{} ({}): {}",
            "CONFLICT".red().bold(),
            conflict_label(conflict.kind),
            conflict.path
        ));
    }
}

/// Exit code 3 when a merge stopped with conflicts.
pub fn conflict_status(outcome: &MergeOutcome) -> Result<()> {
    match outcome {
        MergeOutcome::Conflicts { conflicts } => Err(CliError::conflicts(format!(
            "automatic merge failed with {}",
            plural(conflicts.len(), "conflict", "conflicts")
        ))
        .with_hint("fix the conflicts, then run `wt merge --continue` (or `wt merge --abort`)")),
        _ => Ok(()),
    }
}

pub fn conflict_label(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::Content => "content",
        ConflictKind::ModifyDelete => "modify/delete",
        ConflictKind::AddAdd => "add/add",
        ConflictKind::Binary => "binary",
    }
}

pub fn revert(ctx: &Context, args: RevertArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let snapshot = repo.revert(&tree.name, &args.rev)?;
    ctx.out.emit(&snapshot, |t| snapshot_line(t, &snapshot))
}

pub fn restore(ctx: &Context, args: RestoreArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let paths = ctx.tree_paths(&repo, &tree, &args.paths)?;
    let restored = repo.restore(&tree.name, &paths, args.from.as_deref())?;
    ctx.out.emit(&restored, |t| {
        if restored.is_empty() {
            t.line("Nothing to restore.");
            return;
        }
        for path in &restored {
            t.line(format_args!("restored: {path}"));
        }
        t.line(format_args!(
            "Restored {}.",
            plural(restored.len(), "file", "files")
        ));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conflicts_exit_with_code_3() {
        let outcome = MergeOutcome::Conflicts {
            conflicts: vec![
                ConflictInfo {
                    path: "a.txt".into(),
                    kind: ConflictKind::Content,
                },
                ConflictInfo {
                    path: "b.bin".into(),
                    kind: ConflictKind::Binary,
                },
            ],
        };
        let err = conflict_status(&outcome).expect_err("conflicts");
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains("2 conflicts"), "{err}");
    }

    #[test]
    fn clean_outcomes_succeed() {
        assert!(conflict_status(&MergeOutcome::UpToDate).is_ok());
        assert!(conflict_status(&MergeOutcome::FastForward { to: "abc".into() }).is_ok());
    }

    #[test]
    fn renders_conflicts() {
        colored::control::set_override(false);
        let outcome = MergeOutcome::Conflicts {
            conflicts: vec![ConflictInfo {
                path: "a.txt".into(),
                kind: ConflictKind::ModifyDelete,
            }],
        };
        let mut t = Text::default();
        render_outcome(&mut t, &outcome);
        assert_eq!(t.as_str(), "CONFLICT (modify/delete): a.txt\n");

        let mut t = Text::default();
        render_outcome(
            &mut t,
            &MergeOutcome::FastForward {
                to: "0123456789abcdef".into(),
            },
        );
        assert_eq!(t.as_str(), "Fast-forward to 01234567\n");
    }

    #[test]
    fn labels() {
        assert_eq!(conflict_label(ConflictKind::Content), "content");
        assert_eq!(conflict_label(ConflictKind::AddAdd), "add/add");
    }
}
