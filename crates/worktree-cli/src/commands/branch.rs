//! `wt branch`, `wt switch`

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::{BranchInfo, Repository, SwitchOptions};

use crate::context::Context;
use crate::error::Result;
use crate::output::format::{short_or, subject};

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct BranchArgs {
    #[command(subcommand)]
    pub action: Option<BranchCommand>,

    /// Include archived branches
    #[arg(short, long)]
    pub all: bool,
}

#[derive(Debug, Subcommand)]
pub enum BranchCommand {
    /// List branches (the default)
    List {
        /// Include archived branches
        #[arg(short, long)]
        all: bool,
    },
    /// Create a branch
    Create {
        name: String,
        /// Revision to start from (default: HEAD)
        #[arg(long, value_name = "REV")]
        from: Option<String>,
    },
    /// Archive (soft-delete) a branch; its history stays reachable
    Archive { name: String },
    /// Restore an archived branch
    Restore { name: String },
}

#[derive(Debug, Args)]
pub struct SwitchArgs {
    /// Branch to switch to
    pub name: String,

    /// Create the branch at the current head first
    #[arg(short = 'c', long)]
    pub create: bool,

    /// Discard uncommitted changes
    #[arg(long)]
    pub force: bool,
}

#[derive(Serialize)]
struct BranchState<'a> {
    tree: &'a str,
    branch: &'a str,
    archived: bool,
}

#[derive(Serialize)]
struct SwitchResult<'a> {
    tree: &'a str,
    branch: &'a str,
    created: bool,
}

pub fn run(ctx: &Context, args: BranchArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    match args.action {
        None => list(ctx, &repo, &tree.name, args.all),
        Some(BranchCommand::List { all }) => list(ctx, &repo, &tree.name, all || args.all),
        Some(BranchCommand::Create { name, from }) => {
            let branch = repo.create_branch(&tree.name, &name, from.as_deref())?;
            ctx.out.emit(&branch, |t| {
                t.line(format_args!(
                    "Created branch {} at {}",
                    branch.name.green().bold(),
                    short_or(branch.tip.as_deref(), "(no snapshots)").yellow()
                ));
            })
        }
        Some(BranchCommand::Archive { name }) => {
            repo.archive_branch(&tree.name, &name)?;
            let state = BranchState {
                tree: &tree.name,
                branch: &name,
                archived: true,
            };
            ctx.out.emit(&state, |t| {
                t.line(format_args!(
                    "Archived branch {} (restore it with `wt branch restore {name}`)",
                    name.bold()
                ));
            })
        }
        Some(BranchCommand::Restore { name }) => {
            repo.restore_branch(&tree.name, &name)?;
            let state = BranchState {
                tree: &tree.name,
                branch: &name,
                archived: false,
            };
            ctx.out.emit(&state, |t| {
                t.line(format_args!("Restored branch {}", name.green().bold()))
            })
        }
    }
}

fn list(ctx: &Context, repo: &Repository, tree: &str, all: bool) -> Result<()> {
    let branches = repo.branches(tree, all)?;
    let subjects = if ctx.out.is_json() {
        Vec::new()
    } else {
        tip_subjects(repo, &branches)?
    };
    ctx.out.emit(&branches, |t| {
        let width = branches.iter().map(|b| b.name.len()).max().unwrap_or(0);
        for (branch, subject) in branches.iter().zip(&subjects) {
            let name = format!("{:<width$}", branch.name);
            let (marker, name) = if branch.is_current {
                ("*", name.green().bold())
            } else if branch.archived {
                (" ", name.dimmed())
            } else {
                (" ", name.normal())
            };
            let archived = if branch.archived {
                " [archived]".dimmed().to_string()
            } else {
                String::new()
            };
            t.line(format_args!(
                "{marker} {name} {} {subject}{archived}",
                short_or(branch.tip.as_deref(), "--------").yellow()
            ));
        }
    })
}

fn tip_subjects(repo: &Repository, branches: &[BranchInfo]) -> Result<Vec<String>> {
    branches
        .iter()
        .map(|b| match &b.tip {
            Some(tip) => Ok(subject(&repo.get_snapshot(tip)?.message).to_string()),
            None => Ok(String::new()),
        })
        .collect()
}

pub fn switch(ctx: &Context, args: SwitchArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    repo.switch_branch(
        &tree.name,
        &args.name,
        SwitchOptions {
            create: args.create,
            force: args.force,
        },
    )?;
    let result = SwitchResult {
        tree: &tree.name,
        branch: &args.name,
        created: args.create,
    };
    ctx.out.emit(&result, |t| {
        let what = if args.create {
            "a new branch"
        } else {
            "branch"
        };
        t.line(format_args!(
            "Switched to {what} {}",
            args.name.green().bold()
        ));
    })
}
