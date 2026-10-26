//! `wt owners` — path owners of a server tree (§15.7): who must review
//! changes to which paths.

use std::path::{Path, PathBuf};

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::commands::iam_common::read_input;
use crate::commands::server_common::{
    nullable, render, tree_or_default, when, Api, Cell, Table, TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::plural;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct OwnersArgs {
    #[command(subcommand)]
    pub command: OwnersCommand,
}

#[derive(Debug, Subcommand)]
pub enum OwnersCommand {
    /// Show the owner rules of a tree and where they come from
    ///
    /// Rules come from the server configuration (`wt owners set`), else from
    /// an OWNERS file at the tree root on the default branch.
    Show {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Replace the owner rules of a tree with those of an OWNERS-format file
    ///
    /// One rule per line: a pattern followed by its owners, e.g.
    /// `docs/**  @alice @acme/docs` (also user:NAME / team:SLUG). Lines
    /// starting with `#` and blank lines are ignored. Patterns are globs
    /// anchored at the tree root (`docs/` = everything below docs, `**/*.go`
    /// at any depth, `*.go` top level only); the last matching rule wins.
    /// `--clear` removes the server configuration so an OWNERS file in the
    /// tree applies again (`wt owners set --clear [TREE]`).
    Set {
        /// OWNERS-format file (`-` = stdin)
        #[arg(value_name = "FILE", required_unless_present = "clear")]
        file: Option<PathBuf>,
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        /// Remove the server configuration instead
        #[arg(long)]
        clear: bool,
    },
}

/// `domain.PathOwnerRule` (also the PUT body element).
#[derive(Debug, Default, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default)]
pub struct Rule {
    pub pattern: String,
    #[serde(deserialize_with = "nullable")]
    pub owners: Vec<String>,
}

/// `OwnersView` of `GET|PUT {tree}/owners`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OwnersView {
    #[serde(deserialize_with = "nullable")]
    rules: Vec<Rule>,
    updated_by: String,
    updated_at: String,
    source: String,
    #[serde(deserialize_with = "nullable")]
    errors: Vec<String>,
}

pub fn run(ctx: &Context, args: OwnersArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        OwnersCommand::Show { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&path(&tree))?;
            render(out, &value, |t, v: OwnersView| show(t, &tree, &v))
        }
        OwnersCommand::Set { file, tree, clear } => {
            let (file, tree) = set_target(file, tree, clear)?;
            let tree = tree_or_default(ctx, tree)?;
            let rules = match &file {
                Some(file) => {
                    let name = if file == Path::new("-") {
                        "stdin".to_string()
                    } else {
                        file.display().to_string()
                    };
                    parse_owners(&read_input(file)?, &name)?
                }
                None => Vec::new(),
            };
            let value = api.put(&path(&tree), &set_body(&rules))?;
            render(out, &value, |t, v: OwnersView| {
                if clear {
                    t.line(format_args!(
                        "Cleared the owner configuration of {}",
                        tree.to_string().bold()
                    ));
                } else {
                    t.line(format_args!(
                        "Set {} on {}",
                        plural(rules.len(), "owner rule", "owner rules"),
                        tree.to_string().bold()
                    ));
                }
                show(t, &tree, &v);
            })
        }
    }
}

fn path(tree: &TreeRef) -> String {
    format!("{}/owners", tree.api())
}

/// The PUT body: `{"rules": [...]}` (empty = remove the server configuration).
fn set_body(rules: &[Rule]) -> Value {
    json!({ "rules": rules })
}

/// The file and tree of `wt owners set`: with `--clear` the single
/// positional is the tree (`wt owners set --clear acme/web`).
fn set_target(
    file: Option<PathBuf>,
    tree: Option<TreeRef>,
    clear: bool,
) -> Result<(Option<PathBuf>, Option<TreeRef>)> {
    if !clear {
        return Ok((file, tree));
    }
    let Some(file) = file else {
        return Ok((None, tree));
    };
    let as_tree = file.to_str().and_then(|s| s.parse::<TreeRef>().ok());
    match (tree, as_tree) {
        (None, Some(tree)) => Ok((None, Some(tree))),
        _ => Err(CliError::usage("--clear does not take a FILE")
            .with_hint("run `wt owners set --clear [TENANT/TREE]`")),
    }
}

/// Parse an OWNERS file: `pattern owner...` per line, `#` comments (at the
/// start of a line or of a word) and blank lines ignored. Owner tokens are
/// kept as written (the server normalizes `@alice`, `@tenant/team`,
/// `user:x`, `team:y`). Errors are `NAME:LINE: message`.
pub fn parse_owners(text: &str, name: &str) -> Result<Vec<Rule>> {
    let mut rules = Vec::new();
    let mut errors = Vec::new();
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    for (i, line) in text.lines().enumerate() {
        let mut words = line.split_whitespace().take_while(|w| !w.starts_with('#'));
        let Some(pattern) = words.next() else {
            continue;
        };
        let owners: Vec<String> = words.map(str::to_string).collect();
        if owners.is_empty() {
            errors.push(format!("{name}:{}: no owners for '{pattern}'", i + 1));
            continue;
        }
        rules.push(Rule {
            pattern: pattern.to_string(),
            owners,
        });
    }
    if errors.is_empty() {
        Ok(rules)
    } else {
        Err(CliError::usage(format!(
            "cannot parse {name}:\n  {}",
            errors.join("\n  ")
        )))
    }
}

fn show(t: &mut Text, tree: &TreeRef, v: &OwnersView) {
    let source = match v.source.as_str() {
        "config" => {
            let mut s = "server configuration".to_string();
            if !v.updated_by.is_empty() {
                s.push_str(&format!(
                    ", updated by {} {}",
                    v.updated_by,
                    when(&v.updated_at)
                ));
            }
            s
        }
        "file" => "OWNERS file on the default branch".to_string(),
        "" | "none" => String::new(),
        other => other.to_string(),
    };
    if source.is_empty() && v.rules.is_empty() {
        t.line(format_args!(
            "No path owners for {tree} (no server configuration and no OWNERS file)"
        ));
    } else {
        t.line(format_args!("Owners of {tree}: {source}"));
        let mut table = Table::new(&["PATTERN", "OWNERS"]);
        for rule in &v.rules {
            table.row([
                Cell::from(rule.pattern.as_str()).green(),
                if rule.owners.is_empty() {
                    "(unowned)".into()
                } else {
                    rule.owners.join(", ").into()
                },
            ]);
        }
        table.render(t, "No owner rules.");
    }
    if !v.errors.is_empty() {
        t.line(format_args!(
            "{}:",
            plural(v.errors.len(), "OWNERS file error", "OWNERS file errors").red()
        ));
        for e in &v.errors {
            t.line(format_args!("  {} {e}", "x".red()));
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn owners(args: &[&str]) -> OwnersCommand {
        let mut full = vec!["owners"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Owners(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    fn tree(spec: &str) -> TreeRef {
        spec.parse().expect("tree")
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(
            owners(&["show"]),
            OwnersCommand::Show { tree: None }
        ));
        assert!(matches!(
            owners(&["show", "acme/web"]),
            OwnersCommand::Show { tree: Some(_) }
        ));
        let OwnersCommand::Set { file, tree, clear } = owners(&["set", "OWNERS", "acme/web"])
        else {
            panic!()
        };
        assert_eq!(file, Some(PathBuf::from("OWNERS")));
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert!(!clear);
        assert!(matches!(
            owners(&["set", "-"]),
            OwnersCommand::Set {
                file: Some(_),
                tree: None,
                clear: false
            }
        ));
        assert!(matches!(
            owners(&["set", "--clear"]),
            OwnersCommand::Set {
                file: None,
                tree: None,
                clear: true
            }
        ));
        assert!(matches!(
            owners(&["set", "--clear", "acme/web"]),
            OwnersCommand::Set { clear: true, .. }
        ));
        assert_eq!(
            parse_err(&["owners", "set"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["owners", "set", "OWNERS", "not-a-tree"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn resolves_set_targets() {
        let (file, t) =
            set_target(Some("OWNERS".into()), Some(tree("acme/web")), false).expect("plain");
        assert_eq!(file, Some(PathBuf::from("OWNERS")));
        assert_eq!(t, Some(tree("acme/web")));
        // --clear: the single positional is the tree.
        let (file, t) = set_target(Some("acme/web".into()), None, true).expect("clear tree");
        assert_eq!((file, t), (None, Some(tree("acme/web"))));
        assert_eq!(set_target(None, None, true).expect("clear"), (None, None));
        assert_eq!(
            set_target(Some("OWNERS".into()), None, true)
                .expect_err("file")
                .exit_code(),
            2
        );
        assert!(set_target(Some("OWNERS".into()), Some(tree("acme/web")), true).is_err());
    }

    #[test]
    fn parses_owners_files() {
        let text = "\u{feff}# Owners of acme/web\n\n*  @ada\ndocs/**   @alice @acme/docs  # writers\n**/*.go user:bob team:go\n  # indented comment\nsrc/#x @carol\n";
        let rules = parse_owners(text, "OWNERS").expect("parse");
        let pairs: Vec<(&str, Vec<&str>)> = rules
            .iter()
            .map(|r| {
                (
                    r.pattern.as_str(),
                    r.owners.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(
            pairs,
            [
                ("*", vec!["@ada"]),
                ("docs/**", vec!["@alice", "@acme/docs"]),
                ("**/*.go", vec!["user:bob", "team:go"]),
                ("src/#x", vec!["@carol"]),
            ]
        );
        assert!(parse_owners("", "OWNERS").expect("empty").is_empty());
        assert!(parse_owners("# only\r\n\r\n", "OWNERS")
            .expect("comments")
            .is_empty());
        assert_eq!(
            parse_owners("a @x\r\nb @y\r\n", "OWNERS").expect("crlf")[1],
            Rule {
                pattern: "b".into(),
                owners: vec!["@y".into()]
            }
        );

        let err = parse_owners("a @x\ndocs/\nsrc # @y\n", "OWNERS").expect_err("errors");
        assert_eq!(err.exit_code(), 2);
        assert_eq!(
            err.to_string(),
            "cannot parse OWNERS:\n  OWNERS:2: no owners for 'docs/'\n  OWNERS:3: no owners for 'src'"
        );

        assert_eq!(
            set_body(&rules[..1]),
            json!({"rules": [{"pattern": "*", "owners": ["@ada"]}]})
        );
        assert_eq!(set_body(&[]), json!({"rules": []}));
    }

    #[test]
    fn renders_views() {
        colored::control::set_override(false);
        let web = tree("acme/web");
        let v: OwnersView = serde_json::from_value(json!({
            "tree_id": "t1", "source": "config", "updated_by": "ada", "updated_at": "",
            "rules": [
                {"pattern": "docs/**", "owners": ["user:alice", "team:docs"]},
                {"pattern": "docs/legal", "owners": null}
            ],
            "errors": []
        }))
        .expect("view");
        let mut t = Text::default();
        show(&mut t, &web, &v);
        assert_eq!(
            t.as_str(),
            "Owners of acme/web: server configuration, updated by ada -\nPATTERN     OWNERS\ndocs/**     user:alice, team:docs\ndocs/legal  (unowned)\n"
        );

        let v: OwnersView = serde_json::from_value(json!({
            "source": "file", "rules": [{"pattern": "*", "owners": ["user:ada"]}],
            "errors": ["OWNERS:3: no owners"]
        }))
        .expect("view");
        let mut t = Text::default();
        show(&mut t, &web, &v);
        assert_eq!(
            t.as_str(),
            "Owners of acme/web: OWNERS file on the default branch\nPATTERN  OWNERS\n*        user:ada\n1 OWNERS file error:\n  x OWNERS:3: no owners\n"
        );

        let v: OwnersView =
            serde_json::from_value(json!({"source": "none", "rules": null, "errors": null}))
                .expect("view");
        let mut t = Text::default();
        show(&mut t, &web, &v);
        assert_eq!(
            t.as_str(),
            "No path owners for acme/web (no server configuration and no OWNERS file)\n"
        );
    }
}
