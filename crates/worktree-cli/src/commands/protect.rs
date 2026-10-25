//! `wt protect` — branch protection rules of a server tree.

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::commands::server_common::{
    confirm, nullable, render, tree_or_default, Api, Cell, Table, TreeRef,
};
use crate::context::Context;
use crate::error::Result;
use crate::output::format::plural;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct ProtectArgs {
    #[command(subcommand)]
    pub command: ProtectCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProtectCommand {
    /// List the protection rules of a tree
    List {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Create or replace the rule for a branch pattern (unset flags are off)
    #[command(allow_missing_positional = true)]
    Set {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Branch name or glob, e.g. "main" or "release/*"
        pattern: String,
        /// Changes reach matching branches only through merge requests
        #[arg(long)]
        no_direct_push: bool,
        /// Approvals required to merge a merge request
        #[arg(long, value_name = "N", default_value_t = 0,
              value_parser = clap::value_parser!(u32).range(0..=100))]
        approvals: u32,
        /// Account allowed to push (repeatable; default: anyone with write access)
        #[arg(long = "pusher", value_name = "U")]
        pushers: Vec<String>,
        /// Matching branches cannot be deleted
        #[arg(long)]
        no_delete: bool,
    },
    /// Remove the rule for a branch pattern
    #[command(allow_missing_positional = true)]
    Remove {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        pattern: String,
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
    },
}

/// `domain.BranchProtection` (also the PUT body; the server fills tree_id).
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct Protection {
    pattern: String,
    no_direct_push: bool,
    required_approvals: u32,
    #[serde(deserialize_with = "nullable")]
    allowed_pushers: Vec<String>,
    no_delete: bool,
}

pub fn run(ctx: &Context, args: ProtectArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        ProtectCommand::List { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&path(&tree))?;
            render(out, &value, |t, rules: Vec<Protection>| table(t, &rules))
        }
        ProtectCommand::Set {
            tree,
            pattern,
            no_direct_push,
            approvals,
            pushers,
            no_delete,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let rule = Protection {
                pattern,
                no_direct_push,
                required_approvals: approvals,
                allowed_pushers: pushers,
                no_delete,
            };
            let value = api.put(&path(&tree), &rule)?;
            render(out, &value, |t, rule: Protection| {
                t.line(format_args!(
                    "Protected {} on {tree}: {}",
                    rule.pattern.green().bold(),
                    describe(&rule)
                ));
            })
        }
        ProtectCommand::Remove { tree, pattern, yes } => {
            let tree = tree_or_default(ctx, tree)?;
            confirm(
                yes,
                &format!("remove the protection of {pattern} on {tree}"),
            )?;
            api.delete(&path(&tree), &[("pattern", &pattern)])?;
            out.emit(
                &json!({ "tree": tree.to_string(), "pattern": pattern, "removed": true }),
                |t| {
                    t.line(format_args!(
                        "Removed the protection of {} on {tree}",
                        pattern.bold()
                    ));
                },
            )
        }
    }
}

fn path(tree: &TreeRef) -> String {
    format!("{}/protections", tree.api())
}

fn pushers(rule: &Protection) -> String {
    if rule.allowed_pushers.is_empty() {
        "anyone with write access".into()
    } else {
        rule.allowed_pushers.join(", ")
    }
}

/// One-line summary of a rule.
fn describe(rule: &Protection) -> String {
    let mut parts = Vec::new();
    if rule.no_direct_push {
        parts.push("merge requests only".to_string());
    }
    if rule.required_approvals > 0 {
        parts.push(format!(
            "{} required",
            plural(rule.required_approvals as usize, "approval", "approvals")
        ));
    }
    if !rule.allowed_pushers.is_empty() {
        parts.push(format!("pushers {}", pushers(rule)));
    }
    if rule.no_delete {
        parts.push("cannot be deleted".into());
    }
    if parts.is_empty() {
        "no restrictions".into()
    } else {
        parts.join(", ")
    }
}

fn table(t: &mut Text, rules: &[Protection]) {
    let mut table = Table::new(&["PATTERN", "DIRECT PUSH", "APPROVALS", "DELETE", "PUSHERS"]);
    let allowed = |blocked: bool| {
        if blocked {
            Cell::from("blocked").red()
        } else {
            Cell::from("allowed")
        }
    };
    for rule in rules {
        table.row([
            Cell::from(rule.pattern.as_str()).green().bold(),
            allowed(rule.no_direct_push),
            rule.required_approvals.to_string().into(),
            allowed(rule.no_delete),
            pushers(rule).into(),
        ]);
    }
    table.render(t, "No protection rules.");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn protect(args: &[&str]) -> ProtectCommand {
        let mut full = vec!["protect"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Protect(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(
            protect(&["list"]),
            ProtectCommand::List { tree: None }
        ));
        assert!(matches!(
            protect(&["list", "acme/web"]),
            ProtectCommand::List { tree: Some(_) }
        ));
        let ProtectCommand::Set {
            tree,
            pattern,
            no_direct_push,
            approvals,
            pushers,
            no_delete,
        } = protect(&[
            "set",
            "acme/web",
            "release/*",
            "--no-direct-push",
            "--approvals",
            "2",
            "--pusher",
            "ada",
            "--pusher",
            "bob",
            "--no-delete",
        ])
        else {
            panic!()
        };
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!(pattern, "release/*");
        assert!(no_direct_push && no_delete);
        assert_eq!(approvals, 2);
        assert_eq!(pushers, ["ada", "bob"]);
        assert!(matches!(
            protect(&["set", "main"]),
            ProtectCommand::Set {
                tree: None,
                approvals: 0,
                no_direct_push: false,
                ..
            }
        ));
        assert!(matches!(
            protect(&["remove", "main", "--yes"]),
            ProtectCommand::Remove {
                tree: None,
                yes: true,
                ..
            }
        ));
        assert!(matches!(
            protect(&["remove", "acme/web", "main"]),
            ProtectCommand::Remove {
                tree: Some(_),
                yes: false,
                ..
            }
        ));
        assert_eq!(
            parse_err(&["protect", "set"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["protect", "set", "main", "--approvals", "101"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn describes_rules() {
        let mut rule = Protection {
            pattern: "main".into(),
            ..Protection::default()
        };
        assert_eq!(describe(&rule), "no restrictions");
        rule.no_direct_push = true;
        rule.required_approvals = 1;
        rule.allowed_pushers = vec!["ada".into()];
        rule.no_delete = true;
        assert_eq!(
            describe(&rule),
            "merge requests only, 1 approval required, pushers ada, cannot be deleted"
        );
        colored::control::set_override(false);
        let mut t = Text::default();
        table(&mut t, &[rule]);
        assert_eq!(
            t.as_str(),
            "PATTERN  DIRECT PUSH  APPROVALS  DELETE   PUSHERS\nmain     blocked      1          blocked  ada\n"
        );
    }
}
