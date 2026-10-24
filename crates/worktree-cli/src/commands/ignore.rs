//! `wt ignore`

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
    /// Show the effective ignore patterns
    List,
    /// Add a pattern (gitignore syntax) to .wt/ignore
    Add { pattern: String },
    /// Check whether a path is ignored
    Check { path: PathBuf },
}

#[derive(Serialize)]
struct Added<'a> {
    pattern: &'a str,
    added: bool,
}

#[derive(Serialize)]
struct Checked<'a> {
    path: &'a str,
    ignored: bool,
}

pub fn run(ctx: &Context, args: IgnoreArgs) -> Result<()> {
    let repo = ctx.open()?;
    match args.command {
        IgnoreCommand::List => {
            let patterns = repo.ignore_patterns()?;
            ctx.out.emit(&patterns, |t| {
                for pattern in &patterns {
                    t.line(pattern);
                }
            })
        }
        IgnoreCommand::Add { pattern } => {
            repo.add_ignore_pattern(&pattern)?;
            ctx.out.emit(
                &Added {
                    pattern: &pattern,
                    added: true,
                },
                |t| {
                    t.line(format_args!("Added ignore pattern {}", pattern.bold()));
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
