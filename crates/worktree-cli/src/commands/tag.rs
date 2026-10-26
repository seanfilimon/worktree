//! `wt tag` — local tags; v0.7 (ENGINEERING_PLAN §15.8): signed tags
//! (`--sign`, `[signing] auto`), `wt tag verify` and `wt tag push`.

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::remote;
use worktree_sdk::signing::{self, Verification};
use worktree_sdk::TagInfo;

use crate::commands::identity::{account_keys, describe};
use crate::commands::remote::remote_name;
use crate::context::{block_on, Context};
use crate::error::{CliError, Result};
use crate::output::format::{short, subject};

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct TagArgs {
    #[command(subcommand)]
    pub action: Option<TagCommand>,
    /// `wt tag NAME [REV] [-m MSG] [--sign]` is short for `wt tag create`
    #[command(flatten)]
    pub create: CreateArgs,
}

/// Arguments of `wt tag create` (and of the `wt tag NAME` shorthand).
#[derive(Debug, Default, Args)]
pub struct CreateArgs {
    /// Tag name
    #[arg(value_name = "NAME")]
    pub name: Option<String>,
    /// Revision to tag (default: HEAD)
    #[arg(value_name = "REV")]
    pub rev: Option<String>,
    /// Tag message (makes an annotated tag)
    #[arg(short, long, value_name = "MSG")]
    pub message: Option<String>,
    /// Sign the tag with your signing key ([signing] key, else "default");
    /// `[signing] auto = true` signs every tag
    #[arg(long)]
    pub sign: bool,
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
        /// Sign the tag with your signing key ([signing] key, else "default")
        #[arg(long)]
        sign: bool,
    },
    /// Delete a tag
    Delete { name: String },
    /// Verify a tag's signature against local (and optionally an account's registered) keys
    Verify {
        name: String,
        /// Also trust the registered keys of this server account
        #[arg(long, value_name = "USERNAME")]
        account: Option<String>,
        /// Verify the server's copy of the tag instead of the local one
        #[arg(long)]
        remote: bool,
        /// Remote for --remote (default: [sync].remote, usually origin)
        #[arg(long = "from", value_name = "R", requires = "remote")]
        from: Option<String>,
    },
    /// Push one tag to the server (reports a refusal, e.g. an unsigned tag under require_signed)
    Push {
        name: String,
        /// Remote (default: [sync].remote, usually origin)
        #[arg(long, value_name = "R")]
        remote: Option<String>,
    },
}

#[derive(Serialize)]
struct Deleted<'a> {
    tag: &'a str,
    deleted: bool,
}

/// `wt tag verify` result (JSON: the tag plus the flattened verification).
#[derive(Serialize)]
struct TagVerifyReport<'a> {
    tag: &'a str,
    target: &'a str,
    #[serde(flatten)]
    verification: &'a Verification,
}

pub fn run(ctx: &Context, args: TagArgs) -> Result<()> {
    let action = match (args.action, args.create.name) {
        (Some(action), _) => action,
        (None, Some(name)) => TagCommand::Create {
            name,
            rev: args.create.rev,
            message: args.create.message,
            sign: args.create.sign,
        },
        (None, None) => TagCommand::List,
    };
    let (repo, tree) = ctx.open_tree()?;
    match action {
        TagCommand::List => {
            let tags = repo.tags(&tree.name)?;
            ctx.out.emit(&tags, |t| {
                let width = tags.iter().map(|tag| tag.name.len()).max().unwrap_or(0);
                for tag in &tags {
                    let message = tag.message.as_deref().map(subject).unwrap_or_default();
                    let signed = if tag.signature.is_some() {
                        format!(" {}", "[signed]".green())
                    } else {
                        String::new()
                    };
                    t.line(format_args!(
                        "{} {}{signed} {message}",
                        format!("{:<width$}", tag.name).yellow().bold(),
                        short(&tag.target)
                    ));
                }
            })
        }
        TagCommand::Create {
            name,
            rev,
            message,
            sign,
        } => {
            let tag = signing::create_tag(
                &repo,
                &tree.name,
                &name,
                rev.as_deref(),
                message.as_deref(),
                sign,
            )?;
            ctx.out.emit(&tag, |t| {
                t.line(format_args!(
                    "Tagged {} as {}{}",
                    short(&tag.target).yellow(),
                    tag.name.bold(),
                    match &tag.signature {
                        Some(sig) => format!(" (signed with key {})", sig.key_id),
                        None => String::new(),
                    }
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
        TagCommand::Verify {
            name,
            account,
            remote: from_remote,
            from,
        } => {
            let tag = if from_remote {
                let remote = remote_name(&repo, from);
                block_on(remote::remote_tags(&repo, &tree.name, &remote))??
                    .into_iter()
                    .find(|t| t.name == name)
                    .ok_or_else(|| {
                        CliError::failure(format!("tag '{name}' does not exist on {remote}"))
                    })?
            } else {
                find_local(&repo.tags(&tree.name)?, &name)?
            };
            let mut trusted = signing::KeyStore::default_location().trusted()?;
            if let Some(account) = account {
                trusted.extend(account_keys(ctx, &account)?);
            }
            let result = signing::verify_tag_with_keys(&tag, &trusted);
            let report = TagVerifyReport {
                tag: &tag.name,
                target: &tag.target,
                verification: &result,
            };
            ctx.out.emit(&report, |t| {
                t.line(format_args!(
                    "tag {}: {}",
                    tag.name.bold(),
                    describe(&result)
                ));
            })?;
            if result.is_valid() {
                Ok(())
            } else {
                Err(
                    CliError::failure(format!("tag '{}' is not verified", tag.name))
                        .with_hint(verify_hint(&result)),
                )
            }
        }
        TagCommand::Push { name, remote } => {
            let remote = remote_name(&repo, remote);
            let tag = block_on(remote::push_tag(&repo, &tree.name, &remote, &name))??;
            ctx.out.emit(&tag, |t| {
                t.line(format_args!(
                    "Pushed tag {} ({}){} to {remote}",
                    tag.name.bold(),
                    short(&tag.target).yellow(),
                    if tag.signature.is_some() {
                        " [signed]"
                    } else {
                        ""
                    }
                ));
            })
        }
    }
}

fn find_local(tags: &[TagInfo], name: &str) -> Result<TagInfo> {
    tags.iter()
        .find(|t| t.name == name)
        .cloned()
        .ok_or_else(|| CliError::failure(format!("tag '{name}' not found")))
}

fn verify_hint(v: &Verification) -> &'static str {
    match v {
        Verification::Unsigned => "sign tags with `wt tag NAME --sign` (or [signing] auto = true)",
        Verification::UnknownKey { .. } => {
            "pass --account USERNAME to trust that account's registered keys"
        }
        _ => "the tag or its signature was altered after signing",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use clap::Parser;

    fn tag(args: &[&str]) -> TagArgs {
        let mut argv = vec!["wt", "tag"];
        argv.extend_from_slice(args);
        match Cli::try_parse_from(argv).expect("parse").command {
            Command::Tag(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_shorthand_and_subcommands() {
        let a = tag(&["v1.0", "HEAD~1", "-m", "one", "--sign"]);
        assert!(a.action.is_none());
        assert_eq!(a.create.name.as_deref(), Some("v1.0"));
        assert_eq!(a.create.rev.as_deref(), Some("HEAD~1"));
        assert_eq!(a.create.message.as_deref(), Some("one"));
        assert!(a.create.sign);
        assert!(matches!(
            tag(&["create", "v2", "--sign"]).action,
            Some(TagCommand::Create { sign: true, .. })
        ));
        assert!(matches!(
            tag(&["verify", "v2", "--account", "ada"]).action,
            Some(TagCommand::Verify { name, account: Some(a), remote: false, .. }) if name == "v2" && a == "ada"
        ));
        assert!(matches!(
            tag(&["verify", "v2", "--remote", "--from", "up"]).action,
            Some(TagCommand::Verify { remote: true, from: Some(r), .. }) if r == "up"
        ));
        assert!(matches!(
            tag(&["push", "v2"]).action,
            Some(TagCommand::Push { name, remote: None }) if name == "v2"
        ));
        let none = tag(&[]);
        assert!(none.action.is_none() && none.create.name.is_none());
    }

    #[test]
    fn verify_report_is_flat() {
        let v = Verification::Unsigned;
        let report = TagVerifyReport {
            tag: "v1",
            target: "s1",
            verification: &v,
        };
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            serde_json::json!({"tag": "v1", "target": "s1", "status": "unsigned"})
        );
        assert!(verify_hint(&v).contains("--sign"));
    }
}
