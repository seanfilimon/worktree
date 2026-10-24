//! `wt tag`

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;

use crate::context::Context;
use crate::error::Result;
use crate::output::format::{short, subject};

#[derive(Debug, Args)]
pub struct TagArgs {
    #[command(subcommand)]
    pub action: Option<TagCommand>,
}

#[derive(Debug, Subcommand)]
pub enum TagCommand {
    /// List tags (the default)
    List,
    /// Tag a snapshot
    Create {
        name: String,
        /// Revision to tag (default: HEAD)
        #[arg(value_name = "REV")]
        rev: Option<String>,
        /// Tag message (makes an annotated tag)
        #[arg(short, long, value_name = "MSG")]
        message: Option<String>,
    },
    /// Delete a tag
    Delete { name: String },
}

#[derive(Serialize)]
struct Deleted<'a> {
    tag: &'a str,
    deleted: bool,
}

pub fn run(ctx: &Context, args: TagArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    match args.action.unwrap_or(TagCommand::List) {
        TagCommand::List => {
            let tags = repo.tags(&tree.name)?;
            ctx.out.emit(&tags, |t| {
                let width = tags.iter().map(|tag| tag.name.len()).max().unwrap_or(0);
                for tag in &tags {
                    let message = tag.message.as_deref().map(subject).unwrap_or_default();
                    t.line(format_args!(
                        "{} {} {message}",
                        format!("{:<width$}", tag.name).yellow().bold(),
                        short(&tag.target)
                    ));
                }
            })
        }
        TagCommand::Create { name, rev, message } => {
            let tag = repo.create_tag(&tree.name, &name, rev.as_deref(), message.as_deref())?;
            ctx.out.emit(&tag, |t| {
                t.line(format_args!(
                    "Tagged {} as {}",
                    short(&tag.target).yellow(),
                    tag.name.bold()
                ));
            })
        }
        TagCommand::Delete { name } => {
            repo.delete_tag(&tree.name, &name)?;
            ctx.out.emit(
                &Deleted {
                    tag: &name,
                    deleted: true,
                },
                |t| {
                    t.line(format_args!("Deleted tag {}", name.bold()));
                },
            )
        }
    }
}
