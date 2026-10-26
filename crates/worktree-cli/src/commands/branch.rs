//! `wt branch`, `wt switch`

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use serde_json::json;
use worktree_sdk::config::OnBranchSwitch;
use worktree_sdk::repo::{AutoStashSwitch, StashApplyOutcome, StashEntry};
use worktree_sdk::{BranchInfo, Repository, SwitchOptions};

use crate::commands::config_as_code;
use crate::context::Context;
use crate::error::Result;
use crate::output::format::{plural, short_or, subject};

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
    /// Set the description of a branch on the server (origin); "" clears
    /// it. Branches declared in .wt-tree/branches.toml are changed there.
    Describe { name: String, text: String },
    /// Set the owner of a branch on the server (origin): user:NAME or
    /// team:SLUG (needs tree:admin)
    Owner {
        name: String,
        #[arg(required_unless_present = "clear", value_name = "user:X|team:Y")]
        owner: Option<String>,
        /// Remove the owner
        #[arg(long, conflicts_with = "owner")]
        clear: bool,
    },
}

/// `wt switch NAME [-c] [--force]`
#[derive(Debug, Args)]
#[command(after_long_help = "\
A dirty working tree is refused unless `--force` (discard the changes) or
`auto_snapshot.on_branch_switch` says otherwise: `snapshot` lets the daemon
snapshot the tree first; `stash` stashes the changes (new files included) as
`auto-stash on BRANCH` and, when you switch back to BRANCH, pops that
auto-stash again. `-c` carries the changes to the new branch.")]
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
    /// Auto-stash of the branch left behind (`on_branch_switch = "stash"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    stashed: Option<&'a StashEntry>,
    /// Auto-stash of the target branch that was popped.
    #[serde(skip_serializing_if = "Option::is_none")]
    restored: Option<&'a StashApplyOutcome>,
}

pub fn run(ctx: &Context, args: BranchArgs) -> Result<()> {
    // config-as-code (§16.3): branch metadata lives on the server.
    match &args.action {
        Some(BranchCommand::Describe { name, text }) => {
            return config_as_code::patch_branch(ctx, name, json!({ "description": text }));
        }
        Some(BranchCommand::Owner { name, owner, clear }) => {
            let owner = if *clear {
                String::new()
            } else {
                owner.clone().unwrap_or_default()
            };
            config_as_code::owner_arg(&owner)?;
            return config_as_code::patch_branch(ctx, name, json!({ "owner": owner }));
        }
        _ => {}
    }
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
        Some(BranchCommand::Describe { .. } | BranchCommand::Owner { .. }) => {
            unreachable!("handled above")
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
    let opts = SwitchOptions {
        create: args.create,
        force: args.force,
    };
    // rewrite (§14.5): `auto_snapshot.on_branch_switch = "stash"` stashes
    // dirty changes on leave and pops the target's auto-stash on return.
    let mode = repo
        .effective_config(Some(&tree.name))
        .map(|c| c.auto_snapshot.on_branch_switch)
        .unwrap_or_default();
    let autostash = if mode == OnBranchSwitch::Stash {
        repo.switch_branch_autostash(&tree.name, &args.name, opts)?
    } else {
        // daemon workstream (§13.11): a running daemon snapshots the tree first
        // when auto_snapshot.on_branch_switch = "snapshot". Best effort.
        if !args.force {
            let _ = crate::context::block_on(worktree_sdk::ipc::prepare_branch_switch(
                repo.root(),
                &tree.name,
            ));
        }
        repo.switch_branch(&tree.name, &args.name, opts)?;
        AutoStashSwitch::default()
    };
    let result = SwitchResult {
        tree: &tree.name,
        branch: &args.name,
        created: args.create,
        stashed: autostash.stashed.as_ref(),
        restored: autostash.restored.as_ref(),
    };
    ctx.out.emit(&result, |t| {
        if let Some(entry) = &autostash.stashed {
            t.line(format_args!(
                "Stashed {} as {}: {}",
                plural(entry.changes.len(), "changed file", "changed files"),
                "stash@{0}".yellow(),
                entry.message
            ));
        }
        let what = if args.create {
            "a new branch"
        } else {
            "branch"
        };
        t.line(format_args!(
            "Switched to {what} {}",
            args.name.green().bold()
        ));
        if let Some(restored) = &autostash.restored {
            crate::commands::stash::render_apply(t, restored);
        }
    })?;
    match &autostash.restored {
        Some(restored) => crate::commands::stash::apply_status(restored),
        None => Ok(()),
    }
}
