//! `wt stash` — set working changes aside and restore them later
//! (ENGINEERING_PLAN §14.5).

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::repo::{StashApplyOutcome, StashEntry, StashOptions};
use worktree_sdk::{DiffOptions, FileDiff, MergeOutcome};

use crate::commands::diff::{render_name_status, render_patches};
use crate::commands::merge::render_outcome;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{ago, plural};
use crate::output::Text;

/// `wt stash [push|list|show|apply|pop|drop|clear]`
#[derive(Debug, Args)]
#[command(
    args_conflicts_with_subcommands = true,
    after_long_help = "\
`wt stash` (or `wt stash push`) saves the changes to files of the head
snapshot and resets them; new files are only stashed with
`--include-untracked`. Entries are numbered from 0 (the newest) and can be
named `N` or `stash@{N}`. `apply` / `pop` merge an entry into the working
tree three-way; conflicting text files get conflict markers and `pop` then
keeps the entry. Entries are stored in .wt/trees/<tree>/stash.jsonl."
)]
pub struct StashArgs {
    #[command(subcommand)]
    pub action: Option<StashCommand>,

    #[command(flatten)]
    pub push: PushArgs,
}

#[derive(Debug, Args)]
pub struct PushArgs {
    /// Description of the entry (default: `WIP on BRANCH: ID SUBJECT`)
    #[arg(short, long, value_name = "MSG")]
    pub message: Option<String>,

    /// Also stash (and remove) files that are not in the head snapshot
    #[arg(short = 'u', long)]
    pub include_untracked: bool,
}

#[derive(Debug, Subcommand)]
pub enum StashCommand {
    /// Save the working changes and reset the working tree (the default)
    Push(PushArgs),
    /// List stash entries, newest first
    List,
    /// Show the changes of an entry
    Show {
        /// Entry (`N` or `stash@{N}`; default 0)
        #[arg(value_name = "N", value_parser = parse_index)]
        index: Option<usize>,
        /// Show patches instead of file names
        #[arg(short, long)]
        patch: bool,
    },
    /// Merge an entry into the working tree and keep it
    Apply {
        /// Entry (`N` or `stash@{N}`; default 0)
        #[arg(value_name = "N", value_parser = parse_index)]
        index: Option<usize>,
    },
    /// Merge an entry into the working tree and drop it unless it conflicted
    Pop {
        /// Entry (`N` or `stash@{N}`; default 0)
        #[arg(value_name = "N", value_parser = parse_index)]
        index: Option<usize>,
    },
    /// Delete an entry
    Drop {
        /// Entry (`N` or `stash@{N}`; default 0)
        #[arg(value_name = "N", value_parser = parse_index)]
        index: Option<usize>,
    },
    /// Delete every entry
    Clear,
}

/// `N` or `stash@{N}`.
fn parse_index(text: &str) -> std::result::Result<usize, String> {
    let inner = text
        .strip_prefix("stash@{")
        .and_then(|s| s.strip_suffix('}'))
        .unwrap_or(text);
    inner
        .trim()
        .parse()
        .map_err(|_| format!("expected N or stash@{{N}}, got {text:?}"))
}

#[derive(Serialize)]
struct ShowResult<'a> {
    entry: &'a StashEntry,
    files: &'a [FileDiff],
}

#[derive(Serialize)]
struct ClearResult<'a> {
    tree: &'a str,
    dropped: usize,
}

pub fn run(ctx: &Context, args: StashArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let tree = tree.name.as_str();
    match args.action.unwrap_or(StashCommand::Push(args.push)) {
        StashCommand::Push(push) => {
            let entry = repo.stash_push(
                tree,
                &StashOptions {
                    message: push.message,
                    include_untracked: push.include_untracked,
                    auto: false,
                },
            )?;
            ctx.out.emit(&entry, |t| {
                t.line(format_args!(
                    "Saved {} as {}: {}",
                    plural(entry.changes.len(), "changed file", "changed files"),
                    "stash@{0}".yellow(),
                    entry.message
                ))
            })
        }
        StashCommand::List => {
            let entries = repo.stash_list(tree)?;
            ctx.out.emit(&entries, |t| render_list(t, &entries))
        }
        StashCommand::Show { index, patch } => {
            let opts = DiffOptions {
                name_only: !patch,
                ..DiffOptions::default()
            };
            let (entry, files) = repo.stash_show(tree, index.unwrap_or(0), &opts)?;
            let result = ShowResult {
                entry: &entry,
                files: &files,
            };
            ctx.out.emit(&result, |t| {
                t.line(format_args!(
                    "{}: {} (on {})",
                    format!("stash@{{{}}}", index.unwrap_or(0)).yellow(),
                    entry.message,
                    entry.branch.green()
                ));
                if patch {
                    render_patches(t, &files);
                } else {
                    render_name_status(t, &files);
                }
            })
        }
        StashCommand::Apply { index } => {
            let outcome = repo.stash_apply(tree, index.unwrap_or(0))?;
            emit_apply(ctx, &outcome)
        }
        StashCommand::Pop { index } => {
            let outcome = repo.stash_pop(tree, index.unwrap_or(0))?;
            emit_apply(ctx, &outcome)
        }
        StashCommand::Drop { index } => {
            let index = index.unwrap_or(0);
            let entry = repo.stash_drop(tree, index)?;
            ctx.out.emit(&entry, |t| {
                t.line(format_args!(
                    "Dropped {} ({})",
                    format!("stash@{{{index}}}").yellow(),
                    entry.message
                ))
            })
        }
        StashCommand::Clear => {
            let dropped = repo.stash_clear(tree)?;
            let result = ClearResult { tree, dropped };
            ctx.out.emit(&result, |t| {
                t.line(format_args!(
                    "Dropped {}.",
                    plural(dropped, "stash entry", "stash entries")
                ))
            })
        }
    }
}

fn render_list(t: &mut Text, entries: &[StashEntry]) {
    if entries.is_empty() {
        t.line("No stash entries.");
    }
    for (i, entry) in entries.iter().enumerate() {
        t.line(format_args!(
            "{}: {} ({}, {}, {})",
            format!("stash@{{{i}}}").yellow(),
            entry.message,
            entry.branch.green(),
            plural(entry.changes.len(), "file", "files"),
            ago(entry.created_at)
        ));
    }
}

/// Print the result of `apply` / `pop` (shared with `wt switch`) and exit
/// 3 when the merge conflicted.
pub fn emit_apply(ctx: &Context, outcome: &StashApplyOutcome) -> Result<()> {
    ctx.out.emit(outcome, |t| render_apply(t, outcome))?;
    apply_status(outcome)
}

pub fn render_apply(t: &mut Text, outcome: &StashApplyOutcome) {
    t.line(format_args!(
        "Applied {}: {} ({})",
        format!("stash@{{{}}}", outcome.index).yellow(),
        outcome.entry.message,
        plural(outcome.changed.len(), "file changed", "files changed")
    ));
    if !outcome.conflicts.is_empty() {
        render_outcome(
            t,
            &MergeOutcome::Conflicts {
                conflicts: outcome.conflicts.clone(),
            },
        );
    }
    if outcome.dropped {
        t.line(format_args!(
            "Dropped {}.",
            format!("stash@{{{}}}", outcome.index).yellow()
        ));
    }
}

pub fn apply_status(outcome: &StashApplyOutcome) -> Result<()> {
    if outcome.conflicts.is_empty() {
        return Ok(());
    }
    Err(CliError::conflicts(format!(
        "applying stash@{{{}}} conflicted in {}",
        outcome.index,
        plural(outcome.conflicts.len(), "file", "files")
    ))
    .with_hint(
        "edit the files with conflict markers (the stash entry was kept; drop it with \
         `wt stash drop` once done)",
    ))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        args: StashArgs,
    }

    fn parse(args: &[&str]) -> std::result::Result<StashArgs, clap::Error> {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        Harness::try_parse_from(argv).map(|h| h.args)
    }

    #[test]
    fn index_syntax() {
        assert_eq!(parse_index("2"), Ok(2));
        assert_eq!(parse_index("stash@{3}"), Ok(3));
        assert!(parse_index("stash@{x}").is_err());
        assert!(parse_index("-1").is_err());
    }

    #[test]
    fn parses_subcommands_and_default_push() {
        let a = parse(&["-m", "wip", "-u"]).unwrap();
        assert!(a.action.is_none());
        assert_eq!(a.push.message.as_deref(), Some("wip"));
        assert!(a.push.include_untracked);
        assert!(matches!(
            parse(&["pop", "stash@{1}"]).unwrap().action,
            Some(StashCommand::Pop { index: Some(1) })
        ));
        assert!(matches!(
            parse(&["show", "-p"]).unwrap().action,
            Some(StashCommand::Show {
                index: None,
                patch: true
            })
        ));
        assert!(matches!(
            parse(&["push", "--include-untracked"]).unwrap().action,
            Some(StashCommand::Push(PushArgs {
                include_untracked: true,
                ..
            }))
        ));
        assert!(parse(&["-m", "x", "list"]).is_err());
    }
}
