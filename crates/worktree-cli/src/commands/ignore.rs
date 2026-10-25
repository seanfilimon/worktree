//! `wt ignore`
//!
//! Rules: `.wt/`, `.wt-tree/` and `.git/` are always ignored; `.wt/ignore`
//! and the root `.wtignore` are authoritative (nested files cannot
//! re-include what they ignore); `<tree>/.wt-tree/ignore` files are additive;
//! soft defaults (`node_modules/`, `target/`, `dist/`, `*.log`, `.env`, ...)
//! apply unless re-included with `!pattern` in the root rules.

use std::path::PathBuf;

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;

use crate::context::Context;
use crate::error::Result;

#[derive(Debug, Args)]
pub struct IgnoreArgs {
    #[command(subcommand)]
    pub command: IgnoreCommand,
}

#[derive(Debug, Subcommand)]
pub enum IgnoreCommand {
    /// Show the effective ignore patterns, in evaluation order
    List {
        /// Show the file each pattern comes from (hard, soft, .wt/ignore, ...)
        #[arg(long)]
        sources: bool,
    },
    /// Add a pattern (gitignore syntax) to .wt/ignore
    Add {
        pattern: String,
        /// Add it to the selected tree's .wt-tree/ignore instead (additive:
        /// cannot re-include root-ignored paths)
        #[arg(long)]
        in_tree: bool,
    },
    /// Check whether a path is ignored
    Check { path: PathBuf },
}

#[derive(Serialize)]
struct Added<'a> {
    pattern: &'a str,
    added: bool,
    file: &'a str,
}

#[derive(Serialize)]
struct Checked<'a> {
    path: &'a str,
    ignored: bool,
}

pub fn run(ctx: &Context, args: IgnoreArgs) -> Result<()> {
    let repo = ctx.open()?;
    match args.command {
        IgnoreCommand::List { sources: false } => {
            let patterns = repo.ignore_patterns()?;
            ctx.out.emit(&patterns, |t| {
                for pattern in &patterns {
                    t.line(pattern);
                }
            })
        }
        IgnoreCommand::List { sources: true } => {
            let rules = repo.ignore_rules()?;
            ctx.out.emit(&rules, |t| {
                let width = rules.iter().map(|r| r.source.len()).max().unwrap_or(0);
                for rule in &rules {
                    t.line(format_args!(
                        "{}  {}",
                        format!("{:<width$}", rule.source).dimmed(),
                        rule.pattern
                    ));
                }
            })
        }
        IgnoreCommand::Add { pattern, in_tree } => {
            let file = if in_tree {
                let tree = ctx.select_tree(&repo)?;
                repo.add_tree_ignore_pattern(&tree.name, &pattern)?;
                if tree.path.as_os_str().is_empty() {
                    ".wt/ignore".to_string()
                } else {
                    format!(
                        "{}/.wt-tree/ignore",
                        tree.path.to_string_lossy().replace('\\', "/")
                    )
                }
            } else {
                repo.add_ignore_pattern(&pattern)?;
                ".wt/ignore".to_string()
            };
            ctx.out.emit(
                &Added {
                    pattern: &pattern,
                    added: true,
                    file: &file,
                },
                |t| {
                    t.line(format_args!(
                        "Added ignore pattern {} to {file}",
                        pattern.bold()
                    ));
                },
            )
        }
        IgnoreCommand::Check { path } => {
            let rel = ctx.root_path(&repo, &path)?;
            let ignored = repo.is_ignored(&rel)?;
            ctx.out.emit(
                &Checked {
                    path: &rel,
                    ignored,
                },
                |t| {
                    if ignored {
                        t.line(format_args!("{} is {}", rel, "ignored".yellow()));
                    } else {
                        t.line(format_args!("{} is {}", rel, "not ignored".green()));
                    }
                },
            )
        }
    }
}
