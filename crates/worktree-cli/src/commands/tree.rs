//! `wt tree`

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;

use crate::context::Context;
use crate::error::Result;

#[derive(Debug, Args)]
pub struct TreeArgs {
    #[command(subcommand)]
    pub command: TreeCommand,
}

#[derive(Debug, Subcommand)]
pub enum TreeCommand {
    /// List the trees of this worktree
    List,
    /// Register a directory as a nested tree
    Add {
        name: String,
        /// Directory of the tree (created if missing)
        path: PathBuf,
    },
    /// Unregister a nested tree (its files are kept)
    Remove { name: String },
}

#[derive(Serialize)]
struct Removed<'a> {
    tree: &'a str,
    removed: bool,
}

pub fn run(ctx: &Context, args: TreeArgs) -> Result<()> {
    let repo = ctx.open()?;
    match args.command {
        TreeCommand::List => {
            let trees = repo.trees()?;
            let current = ctx.select_tree(&repo)?.name;
            ctx.out.emit(&trees, |t| {
                let width = trees.iter().map(|tree| tree.name.len()).max().unwrap_or(0);
                for tree in &trees {
                    let name = format!("{:<width$}", tree.name);
                    let (marker, name) = if tree.name == current {
                        ("*", name.green().bold())
                    } else {
                        (" ", name.normal())
                    };
                    t.line(format_args!(
                        "{marker} {name}  {}  [{}]",
                        display_path(&tree.path),
                        tree.current_branch.cyan()
                    ));
                }
            })
        }
        TreeCommand::Add { name, path } => {
            let rel = ctx.root_path(&repo, &path)?;
            let tree = repo.add_tree(&name, Path::new(&rel))?;
            ctx.out.emit(&tree, |t| {
                t.line(format_args!(
                    "Added tree {} at {}",
                    tree.name.bold(),
                    display_path(&tree.path)
                ));
            })
        }
        TreeCommand::Remove { name } => {
            repo.remove_tree(&name)?;
            ctx.out.emit(
                &Removed {
                    tree: &name,
                    removed: true,
                },
                |t| {
                    t.line(format_args!(
                        "Removed tree {} (files were kept)",
                        name.bold()
                    ));
                },
            )
        }
    }
}

/// Tree paths are root-relative; the root tree's is empty.
fn display_path(path: &Path) -> String {
    if path.as_os_str().is_empty() {
        ".".into()
    } else {
        path.to_string_lossy().replace('\\', "/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_tree_path_displays_as_dot() {
        assert_eq!(display_path(Path::new("")), ".");
        assert_eq!(display_path(Path::new("apps/web")), "apps/web");
    }
}
