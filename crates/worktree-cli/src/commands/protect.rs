//! `wt protect` — branch and tag protection rules of a server tree.

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::commands::mr::MergeMethod;
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
        /// Merges need passing CI on the source tip (every --check, or every reported check)
        #[arg(long)]
        require_ci: bool,
        /// CI context that must succeed (repeatable; implies --require-ci)
        #[arg(long = "check", value_name = "NAME")]
        checks: Vec<String>,
        /// Pushed snapshots must carry a verified signature
        #[arg(long)]
        require_signed: bool,
        /// No merge snapshots: direct pushes and merged sources must be linear
        #[arg(long)]
        require_linear: bool,
        /// Open blocking TODOs of the source branch block merging
        #[arg(long)]
        block_on_todos: bool,
        /// Merge methods allowed for merge requests into matching branches,
        /// comma-separated (fast-forward, merge, squash; default: all)
        #[arg(long, value_name = "METHODS", value_delimiter = ',')]
        merge_methods: Vec<MergeMethod>,
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
    /// Tag protection rules: who may create tags, and whether they can move or be deleted
    Tag(TagArgs),
}

#[derive(Debug, Args)]
pub struct TagArgs {
    #[command(subcommand)]
    pub command: TagCommand,
}

#[derive(Debug, Subcommand)]
pub enum TagCommand {
    /// List the tag protection rules of a tree
    List {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Create or replace the rule for a tag pattern (unset flags are off)
    #[command(allow_missing_positional = true)]
    Set {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Tag name or glob, e.g. "v*"
        pattern: String,
        /// Matching tags cannot be deleted
        #[arg(long)]
        no_delete: bool,
        /// Matching tags cannot be replaced (moved to another snapshot)
        #[arg(long)]
        no_move: bool,
        /// Account allowed to create, move or delete matching tags (repeatable; default: anyone with write access)
        #[arg(long = "tagger", value_name = "U")]
        taggers: Vec<String>,
        // v0.7 trust (§15.8).
        /// Matching tags must carry a verified signature (`wt tag NAME --sign`)
        #[arg(long)]
        require_signed: bool,
    },
    /// Remove the rule for a tag pattern
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
    require_ci_pass: bool,
    #[serde(deserialize_with = "nullable")]
    required_checks: Vec<String>,
    require_signed: bool,
    require_linear: bool,
    block_on_todos: bool,
    #[serde(deserialize_with = "nullable")]
    merge_methods: Vec<String>,
}

/// `domain.TagProtection` (also the PUT body).
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct TagProtection {
    pattern: String,
    no_delete: bool,
    no_move: bool,
    #[serde(deserialize_with = "nullable")]
    allowed_taggers: Vec<String>,
    // v0.7 trust (§15.8).
    require_signed: bool,
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
            require_ci,
            checks,
            require_signed,
            require_linear,
            block_on_todos,
            merge_methods,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let mut methods: Vec<String> = Vec::new();
            for m in merge_methods {
                if !methods.iter().any(|x| x == m.as_str()) {
                    methods.push(m.as_str().to_string());
                }
            }
            let rule = Protection {
                pattern,
                no_direct_push,
                required_approvals: approvals,
                allowed_pushers: pushers,
                no_delete,
                require_ci_pass: require_ci || !checks.is_empty(),
                required_checks: checks,
                require_signed,
                require_linear,
                block_on_todos,
                merge_methods: methods,
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
        ProtectCommand::Tag(args) => run_tag(ctx, &api, args.command),
    }
}

fn run_tag(ctx: &Context, api: &Api, command: TagCommand) -> Result<()> {
    let out = &ctx.out;
    match command {
        TagCommand::List { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&tag_path(&tree))?;
            render(out, &value, |t, rules: Vec<TagProtection>| {
                tag_table(t, &rules)
            })
        }
        TagCommand::Set {
            tree,
            pattern,
            no_delete,
            no_move,
            taggers,
            require_signed,
        } => {
            let tree = tree_or_default(ctx, tree)?;
            let rule = TagProtection {
                pattern,
                no_delete,
                no_move,
                allowed_taggers: taggers,
                require_signed,
            };
            let value = api.put(&tag_path(&tree), &rule)?;
            render(out, &value, |t, rule: TagProtection| {
                t.line(format_args!(
                    "Protected tags {} on {tree}: {}",
                    rule.pattern.green().bold(),
                    describe_tag(&rule)
                ));
            })
        }
        TagCommand::Remove { tree, pattern, yes } => {
            let tree = tree_or_default(ctx, tree)?;
            confirm(
                yes,
                &format!("remove the tag protection of {pattern} on {tree}"),
            )?;
            api.delete(&tag_path(&tree), &[("pattern", &pattern)])?;
            out.emit(
                &json!({ "tree": tree.to_string(), "pattern": pattern, "removed": true }),
                |t| {
                    t.line(format_args!(
                        "Removed the tag protection of {} on {tree}",
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

fn tag_path(tree: &TreeRef) -> String {
    format!("{}/tag-protections", tree.api())
}

fn pushers(rule: &Protection) -> String {
    if rule.allowed_pushers.is_empty() {
        "anyone with write access".into()
    } else {
        rule.allowed_pushers.join(", ")
    }
}

/// The merge requirements of a rule beyond approvals (CI, signatures,
/// linear history, TODOs).
fn merge_checks(rule: &Protection) -> Vec<String> {
    let mut parts = Vec::new();
    if rule.require_ci_pass {
        if rule.required_checks.is_empty() {
            parts.push("ci".to_string());
        } else {
            parts.push(format!("ci ({})", rule.required_checks.join(", ")));
        }
    }
    if rule.require_signed {
        parts.push("signed".into());
    }
    if rule.require_linear {
        parts.push("linear".into());
    }
    if rule.block_on_todos {
        parts.push("todos".into());
    }
    if !rule.merge_methods.is_empty() {
        parts.push(format!("methods {}", rule.merge_methods.join("/")));
    }
    parts
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
    if rule.require_ci_pass {
        if rule.required_checks.is_empty() {
            parts.push("passing CI required".into());
        } else {
            parts.push(format!("CI {} required", rule.required_checks.join(", ")));
        }
    }
    if rule.require_signed {
        parts.push("signed snapshots only".into());
    }
    if rule.require_linear {
        parts.push("linear history".into());
    }
    if rule.block_on_todos {
        parts.push("blocking TODOs block merges".into());
    }
    if !rule.merge_methods.is_empty() {
        parts.push(format!("merge methods {}", rule.merge_methods.join(", ")));
    }
    if parts.is_empty() {
        "no restrictions".into()
    } else {
        parts.join(", ")
    }
}

fn describe_tag(rule: &TagProtection) -> String {
    let mut parts = Vec::new();
    if rule.no_move {
        parts.push("cannot be moved".to_string());
    }
    if rule.no_delete {
        parts.push("cannot be deleted".into());
    }
    if !rule.allowed_taggers.is_empty() {
        parts.push(format!("taggers {}", rule.allowed_taggers.join(", ")));
    }
    if rule.require_signed {
        parts.push("signed tags only".into()); // v0.7 trust
    }
    if parts.is_empty() {
        "no restrictions".into()
    } else {
        parts.join(", ")
    }
}

fn table(t: &mut Text, rules: &[Protection]) {
    let mut table = Table::new(&[
        "PATTERN",
        "DIRECT PUSH",
        "APPROVALS",
        "DELETE",
        "PUSHERS",
        "CHECKS",
    ]);
    let allowed = |blocked: bool| {
        if blocked {
            Cell::from("blocked").red()
        } else {
            Cell::from("allowed")
        }
    };
    for rule in rules {
        let checks = merge_checks(rule);
        table.row([
            Cell::from(rule.pattern.as_str()).green().bold(),
            allowed(rule.no_direct_push),
            rule.required_approvals.to_string().into(),
            allowed(rule.no_delete),
            pushers(rule).into(),
            if checks.is_empty() {
                "-".into()
            } else {
                checks.join(", ").into()
            },
        ]);
    }
    table.render(t, "No protection rules.");
}

fn tag_table(t: &mut Text, rules: &[TagProtection]) {
    let mut table = Table::new(&["PATTERN", "MOVE", "DELETE", "TAGGERS"]);
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
            allowed(rule.no_move),
            allowed(rule.no_delete),
            if rule.allowed_taggers.is_empty() {
                "anyone with write access".into()
            } else {
                rule.allowed_taggers.join(", ").into()
            },
        ]);
    }
    table.render(t, "No tag protection rules.");
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
            require_ci,
            checks,
            require_signed,
            require_linear,
            block_on_todos,
            merge_methods,
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
            "--require-ci",
            "--check",
            "build",
            "--check",
            "test",
            "--require-signed",
            "--require-linear",
            "--block-on-todos",
            "--merge-methods",
            "squash,fast-forward",
        ])
        else {
            panic!()
        };
        assert_eq!(
            merge_methods,
            [MergeMethod::Squash, MergeMethod::FastForward]
        );
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!(pattern, "release/*");
        assert!(no_direct_push && no_delete);
        assert!(require_ci && require_signed && require_linear && block_on_todos);
        assert_eq!(approvals, 2);
        assert_eq!(pushers, ["ada", "bob"]);
        assert_eq!(checks, ["build", "test"]);
        assert!(matches!(
            protect(&["set", "main"]),
            ProtectCommand::Set {
                tree: None,
                approvals: 0,
                no_direct_push: false,
                require_ci: false,
                require_linear: false,
                ..
            }
        ));
        let ProtectCommand::Set { merge_methods, .. } = protect(&["set", "main"]) else {
            panic!()
        };
        assert!(merge_methods.is_empty());
        assert_eq!(
            parse_err(&["protect", "set", "main", "--merge-methods", "squash,rebase"]),
            ErrorKind::InvalidValue
        );
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
    fn parses_tag_subcommands() {
        let tag = |args: &[&str]| match protect(args) {
            ProtectCommand::Tag(a) => a.command,
            other => panic!("{other:?}"),
        };
        assert!(matches!(
            tag(&["tag", "list", "acme/web"]),
            TagCommand::List { tree: Some(_) }
        ));
        let TagCommand::Set {
            tree,
            pattern,
            no_delete,
            no_move,
            taggers,
            require_signed,
        } = tag(&[
            "tag",
            "set",
            "v*",
            "--no-delete",
            "--no-move",
            "--tagger",
            "ada",
            "--require-signed",
        ])
        else {
            panic!()
        };
        assert!(tree.is_none() && no_delete && no_move && require_signed);
        assert_eq!((pattern.as_str(), taggers), ("v*", vec!["ada".to_string()]));
        assert!(matches!(
            tag(&["tag", "remove", "acme/web", "v*", "--yes"]),
            TagCommand::Remove {
                tree: Some(_),
                yes: true,
                ..
            }
        ));
        assert_eq!(
            parse_err(&["protect", "tag", "set"]),
            ErrorKind::MissingRequiredArgument
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
            "PATTERN  DIRECT PUSH  APPROVALS  DELETE   PUSHERS  CHECKS\nmain     blocked      1          blocked  ada      -\n"
        );

        let rule = Protection {
            pattern: "main".into(),
            require_ci_pass: true,
            required_checks: vec!["build".into()],
            require_linear: true,
            block_on_todos: true,
            require_signed: true,
            ..Protection::default()
        };
        assert_eq!(
            describe(&rule),
            "CI build required, signed snapshots only, linear history, blocking TODOs block merges"
        );
        assert_eq!(
            merge_checks(&rule),
            ["ci (build)", "signed", "linear", "todos"]
        );
        let body = serde_json::to_value(&rule).expect("json");
        assert_eq!(body["require_ci_pass"], true);
        assert_eq!(body["required_checks"], json!(["build"]));

        // v0.6 (§14.1): merge-method restrictions.
        let rule: Protection = serde_json::from_value(json!({
            "pattern": "main", "merge_methods": ["fast-forward", "squash"]
        }))
        .expect("rule");
        assert_eq!(describe(&rule), "merge methods fast-forward, squash");
        assert_eq!(merge_checks(&rule), ["methods fast-forward/squash"]);
        let body = serde_json::to_value(&rule).expect("json");
        assert_eq!(body["merge_methods"], json!(["fast-forward", "squash"]));
        let rule: Protection =
            serde_json::from_value(json!({"pattern": "main", "merge_methods": null}))
                .expect("rule");
        assert!(rule.merge_methods.is_empty());
    }

    #[test]
    fn describes_tag_rules() {
        let rule: TagProtection = serde_json::from_value(json!({
            "pattern": "v*", "no_move": true, "no_delete": false, "allowed_taggers": null
        }))
        .expect("rule");
        assert_eq!(describe_tag(&rule), "cannot be moved");
        assert_eq!(describe_tag(&TagProtection::default()), "no restrictions");
        colored::control::set_override(false);
        let mut t = Text::default();
        tag_table(&mut t, &[rule]);
        assert_eq!(
            t.as_str(),
            "PATTERN  MOVE     DELETE   TAGGERS\nv*       blocked  allowed  anyone with write access\n"
        );
    }
}
