//! `wt git import|export` over `worktree_git::bridge`.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_git::bridge::{export_repository, import_repository, ExportOptions, ImportOptions};

use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::plural;

#[derive(Debug, Args)]
pub struct GitArgs {
    #[command(subcommand)]
    pub command: GitCommand,
}

#[derive(Debug, Subcommand)]
pub enum GitCommand {
    /// Create a new worktree from a Git repository (all branches and tags)
    Import {
        /// Git repository to import
        git_path: PathBuf,
        /// Destination directory (default: ./<repository name>)
        dest: Option<PathBuf>,
    },
    /// Export this tree's history into a Git repository (created if missing)
    Export {
        /// Git repository to write; a missing path ending in `.git` is created bare
        git_path: PathBuf,
        /// Branch to export (default: all branches)
        #[arg(long, value_name = "REV")]
        rev: Option<String>,
    },
}

#[derive(Serialize)]
struct ImportResult<'a> {
    dest: &'a Path,
    branches: &'a [String],
    snapshots: usize,
    blobs: usize,
    tags: usize,
    checked_out: Option<&'a str>,
}

#[derive(Serialize)]
struct ExportResult<'a> {
    git_path: &'a Path,
    branches: &'a [String],
    commits: usize,
    tags: usize,
}

pub fn run(ctx: &Context, args: GitArgs) -> Result<()> {
    match args.command {
        GitCommand::Import { git_path, dest } => import(ctx, &git_path, dest),
        GitCommand::Export { git_path, rev } => export(ctx, &git_path, rev),
    }
}

fn import(ctx: &Context, git_path: &Path, dest: Option<PathBuf>) -> Result<()> {
    let source = ctx.absolute(git_path);
    if !source.is_dir() {
        return Err(CliError::failure(format!(
            "no Git repository at '{}'",
            git_path.display()
        )));
    }
    let dest = match dest {
        Some(dest) => ctx.absolute(&dest),
        None => {
            let name = repository_name(&source).ok_or_else(|| {
                CliError::usage(format!(
                    "cannot derive a destination from '{}'",
                    git_path.display()
                ))
                .with_hint("pass a destination: `wt git import GIT_PATH DEST`")
            })?;
            ctx.cwd().join(name)
        }
    };
    let opts = ImportOptions {
        include_tags: true,
        ..ImportOptions::default()
    };
    let report = import_repository(&source, &dest, &opts)?;
    let result = ImportResult {
        dest: &report.dest,
        branches: &report.branches,
        snapshots: report.snapshots,
        blobs: report.blobs,
        tags: report.tags,
        checked_out: report.checked_out.as_deref(),
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Imported {} ({}, {}, {}) into {}",
            plural(report.branches.len(), "branch", "branches"),
            plural(report.snapshots, "snapshot", "snapshots"),
            plural(report.blobs, "blob", "blobs"),
            plural(report.tags, "tag", "tags"),
            report.dest.display()
        ));
        if let Some(branch) = &report.checked_out {
            t.line(format_args!("Checked out {}", branch.green().bold()));
        }
    })
}

fn export(ctx: &Context, git_path: &Path, rev: Option<String>) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    if let Some(rev) = &rev {
        if !repo
            .branches(&tree.name, true)?
            .iter()
            .any(|b| &b.name == rev)
        {
            return Err(CliError::failure(format!(
                "'{rev}' is not a branch of tree '{}'",
                tree.name
            ))
            .with_hint("`--rev` names the branch to export; list them with `wt branch --all`"));
        }
    }
    let target = ctx.absolute(git_path);
    let bare = !target.exists() && target.extension().is_some_and(|ext| ext == "git");
    let opts = ExportOptions {
        tree: Some(tree.name.clone()),
        branches: rev.into_iter().collect(),
        include_tags: true,
        bare,
    };
    let report = export_repository(&repo, &target, &opts)?;
    let result = ExportResult {
        git_path: &report.git_path,
        branches: &report.branches,
        commits: report.commits,
        tags: report.tags,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Exported {} ({} new, {}) to {}",
            plural(report.branches.len(), "branch", "branches"),
            plural(report.commits, "commit", "commits"),
            plural(report.tags, "tag", "tags"),
            report.git_path.display()
        ));
    })
}

/// `../proj` → `proj`, `../proj.git` → `proj`, `../proj/.git` → `proj`.
fn repository_name(path: &Path) -> Option<String> {
    let path = crate::paths::normalize(path);
    let mut name = path.file_name()?.to_string_lossy().into_owned();
    if name == ".git" {
        name = path.parent()?.file_name()?.to_string_lossy().into_owned();
    }
    let name = name.strip_suffix(".git").unwrap_or(&name);
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_names() {
        assert_eq!(
            repository_name(Path::new("/src/proj")).as_deref(),
            Some("proj")
        );
        assert_eq!(
            repository_name(Path::new("/src/proj.git")).as_deref(),
            Some("proj")
        );
        assert_eq!(
            repository_name(Path::new("/src/proj/.git")).as_deref(),
            Some("proj")
        );
        assert_eq!(
            repository_name(Path::new("/src/proj/")).as_deref(),
            Some("proj")
        );
        assert_eq!(
            repository_name(Path::new("/src/proj/sub/..")).as_deref(),
            Some("proj")
        );
    }
}
