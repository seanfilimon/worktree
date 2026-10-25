//! `wt tree`

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;

use worktree_sdk::config::{Severity, TreeProblem};

use crate::context::Context;
use crate::error::{CliError, Result};

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
    /// Check tree layout: .wt-tree/ marker, config and ignore files parse,
    /// no nested .wt/ (all trees when NAME is omitted; exit 1 on errors)
    Validate { name: Option<String> },
    /// Scaffold .wt-tree/{config.toml,ignore,hooks/,access/} for a nested
    /// tree (default: the selected tree); existing files are kept
    InitConfig { name: Option<String> },
}

#[derive(Serialize)]
struct Validated {
    tree: String,
    problems: Vec<TreeProblem>,
}

#[derive(Serialize)]
struct Scaffolded {
    tree: String,
    created: Vec<String>,
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
        TreeCommand::Validate { name } => {
            let names = match name {
                Some(name) => vec![repo.tree(&name)?.name],
                None => repo.trees()?.into_iter().map(|t| t.name).collect(),
            };
            let mut report = Vec::new();
            for tree in names {
                let problems = repo.validate_tree(&tree)?;
                report.push(Validated { tree, problems });
            }
            let errors = report
                .iter()
                .flat_map(|r| &r.problems)
                .filter(|p| p.severity == Severity::Error)
                .count();
            ctx.out.emit(&report, |t| {
                for r in &report {
                    if r.problems.is_empty() {
                        t.line(format_args!("{} {}", r.tree.bold(), "ok".green()));
                        continue;
                    }
                    t.line(r.tree.bold());
                    for p in &r.problems {
                        let label = match p.severity {
                            Severity::Error => "error".red(),
                            Severity::Warning => "warning".yellow(),
                        };
                        t.line(format_args!("  {label}: {}: {}", p.path, p.message));
                    }
                }
            })?;
            if errors > 0 {
                return Err(CliError::failure(format!(
                    "tree validation found {errors} error(s)"
                )));
            }
            Ok(())
        }
        TreeCommand::InitConfig { name } => {
            let tree = match name {
                Some(name) => repo.tree(&name)?,
                None => ctx.select_tree(&repo)?,
            };
            let created: Vec<String> = repo
                .init_tree_config(&tree.name)?
                .iter()
                .map(|p| {
                    p.strip_prefix(repo.root())
                        .unwrap_or(p)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            let out = Scaffolded {
                tree: tree.name.clone(),
                created,
            };
            ctx.out.emit(&out, |t| {
                if out.created.is_empty() {
                    t.line(format_args!(
                        "{} already has a complete .wt-tree/",
                        out.tree.bold()
                    ));
                }
                for path in &out.created {
                    t.line(format_args!("created {path}"));
                }
            })
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
