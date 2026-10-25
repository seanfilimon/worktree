//! `wt init`

use std::path::{Path, PathBuf};

use clap::Args;
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::{InitOptions, Repository, TreeInfo};

use crate::context::Context;
use crate::error::Result;

#[derive(Debug, Args)]
pub struct InitArgs {
    /// Directory to initialize (default: the current directory)
    pub path: Option<PathBuf>,

    /// Name of the worktree and its root tree (default: directory name)
    #[arg(long, value_name = "N")]
    pub name: Option<String>,

    /// Default branch (default: main)
    #[arg(long, value_name = "B")]
    pub branch: Option<String>,

    /// Start the background daemon (if needed) and auto-snapshot the new worktree
    #[arg(long)]
    pub watch: bool,
}

#[derive(Serialize)]
struct InitResult<'a> {
    root: &'a Path,
    tree: TreeInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    watching: Option<String>,
}

pub fn run(ctx: &Context, args: InitArgs) -> Result<()> {
    let path = args
        .path
        .map_or_else(|| ctx.cwd().to_path_buf(), |p| ctx.absolute(&p));
    let repo = Repository::init(
        &path,
        InitOptions {
            name: args.name,
            default_branch: args.branch,
        },
    )?;
    let watching = if args.watch {
        Some(crate::commands::daemon::start_and_watch(repo.root())?)
    } else {
        None
    };
    let result = InitResult {
        root: repo.root(),
        tree: repo.root_tree()?,
        watching,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Initialized empty worktree {} in {}",
            result.tree.name.bold(),
            repo.wt_dir().display()
        ));
        t.line(format_args!(
            "Default branch: {}",
            result.tree.default_branch.green()
        ));
        if let Some(msg) = &result.watching {
            t.line(format_args!("Daemon: {msg}"));
        }
    })
}
