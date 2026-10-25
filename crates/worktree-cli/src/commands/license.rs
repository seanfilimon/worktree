//! `wt license` — path license rules of a server tree.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::commands::server_common::{confirm, render, tree_or_default, Api, Cell, Table, TreeRef};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

/// `wt license detect` (git-export, §13.8).
mod detect;

#[derive(Debug, Args)]
pub struct LicenseArgs {
    #[command(subcommand)]
    pub command: LicenseCommand,
}

#[derive(Debug, Subcommand)]
pub enum LicenseCommand {
    /// List the license rules of a tree
    List {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Assign an SPDX license to the paths matching a glob
    #[command(override_usage = "wt license set [OPTIONS] [TREE] <GLOB> <SPDX>")]
    Set {
        /// [TREE] GLOB SPDX: an optional TENANT/TREE, the path glob and the SPDX license id
        #[arg(value_name = "ARG", num_args = 2..=3, required = true)]
        args: Vec<String>,
        /// What other tenants may do with the matching files
        #[arg(long, value_name = "GRANT", default_value = "read-only")]
        grant: Grant,
    },
    /// Remove the rule for a glob
    #[command(allow_missing_positional = true)]
    Remove {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
        glob: String,
        /// Do not ask for confirmation
        #[arg(long)]
        yes: bool,
    },
    /// Guess the SPDX licenses of the tree's files (LICENSE files and
    /// SPDX-License-Identifier headers) and propose license rules
    ///
    /// A LICENSE/LICENCE/COPYING file proposes a rule for its directory
    /// (several license files in one directory combine with OR); a file
    /// whose SPDX header differs from its directory's license proposes a
    /// rule for that file. `--apply` creates the proposed rules on the
    /// tree's server (rules that already exist are skipped).
    Detect {
        /// File or directory to scan (default: the whole tree)
        path: Option<std::path::PathBuf>,
        /// Create the proposed rules on the server
        #[arg(long)]
        apply: bool,
        /// Grant for the created rules
        #[arg(
            long,
            value_name = "GRANT",
            default_value = "read-only",
            requires = "apply"
        )]
        grant: Grant,
        /// Do not ask for confirmation
        #[arg(long, requires = "apply")]
        yes: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Grant {
    ReadOnly,
    Modify,
    Redistribute,
}

/// `domain.LicenseRule` (also the PUT body; the server fills tree_id).
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
struct LicenseRule {
    path_glob: String,
    spdx: String,
    grant: String,
}

pub fn run(ctx: &Context, args: LicenseArgs) -> Result<()> {
    // git-export (§13.8): `detect` works offline unless --apply is given.
    if let LicenseCommand::Detect {
        path,
        apply,
        grant,
        yes,
    } = args.command
    {
        return detect::run(ctx, path, apply, grant, yes);
    }
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        LicenseCommand::List { tree } => {
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&path(&tree))?;
            render(out, &value, |t, rules: Vec<LicenseRule>| table(t, &rules))
        }
        LicenseCommand::Set { args, grant } => {
            let (tree, glob, spdx) = split_set_args(args)?;
            let tree = tree_or_default(ctx, tree)?;
            let body = json!({ "path_glob": glob, "spdx": spdx, "grant": grant });
            let value = api.put(&path(&tree), &body)?;
            render(out, &value, |t, rule: LicenseRule| {
                t.line(format_args!(
                    "Licensed {} on {tree} as {} ({})",
                    rule.path_glob.bold(),
                    rule.spdx.green().bold(),
                    rule.grant
                ));
            })
        }
        // Handled before connecting (git-export, §13.8).
        LicenseCommand::Detect { .. } => Ok(()),
        LicenseCommand::Remove { tree, glob, yes } => {
            let tree = tree_or_default(ctx, tree)?;
            confirm(
                yes,
                &format!("remove the license rule for {glob} on {tree}"),
            )?;
            api.delete(&path(&tree), &[("path_glob", &glob)])?;
            out.emit(
                &json!({ "tree": tree.to_string(), "path_glob": glob, "removed": true }),
                |t| {
                    t.line(format_args!(
                        "Removed the license rule for {} on {tree}",
                        glob.bold()
                    ));
                },
            )
        }
    }
}

/// `[TREE] GLOB SPDX` → its parts (clap guarantees two or three values).
fn split_set_args(args: Vec<String>) -> Result<(Option<TreeRef>, String, String)> {
    let mut args = args.into_iter();
    let (tree, glob, spdx) = match (args.next(), args.next(), args.next()) {
        (Some(glob), Some(spdx), None) => (None, glob, spdx),
        (Some(tree), Some(glob), Some(spdx)) => {
            let tree = tree.parse::<TreeRef>().map_err(CliError::usage)?;
            (Some(tree), glob, spdx)
        }
        _ => return Err(CliError::usage("expected [TREE] GLOB SPDX")),
    };
    Ok((tree, glob, spdx))
}

fn path(tree: &TreeRef) -> String {
    format!("{}/licenses", tree.api())
}

fn table(t: &mut Text, rules: &[LicenseRule]) {
    let mut table = Table::new(&["GLOB", "SPDX", "GRANT"]);
    for rule in rules {
        table.row([
            Cell::from(rule.path_glob.as_str()).bold(),
            Cell::from(rule.spdx.as_str()).green(),
            rule.grant.as_str().into(),
        ]);
    }
    table.render(t, "No license rules.");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn license(args: &[&str]) -> LicenseCommand {
        let mut full = vec!["license"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::License(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(
            license(&["list"]),
            LicenseCommand::List { tree: None }
        ));
        let LicenseCommand::Set { args, grant } =
            license(&["set", "acme/web", "vendor/**", "MIT", "--grant", "modify"])
        else {
            panic!()
        };
        assert_eq!(grant, Grant::Modify);
        let (tree, glob, spdx) = split_set_args(args).expect("split");
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!((glob.as_str(), spdx.as_str()), ("vendor/**", "MIT"));

        let LicenseCommand::Set { args, grant } = license(&["set", "**", "Apache-2.0"]) else {
            panic!()
        };
        assert_eq!(grant, Grant::ReadOnly);
        let (tree, glob, spdx) = split_set_args(args).expect("split");
        assert!(tree.is_none());
        assert_eq!((glob.as_str(), spdx.as_str()), ("**", "Apache-2.0"));

        let LicenseCommand::Set { args, .. } = license(&["set", "web", "**", "MIT"]) else {
            panic!()
        };
        assert_eq!(split_set_args(args).expect_err("bad tree").exit_code(), 2);

        assert_eq!(
            parse_err(&["license", "set", "MIT"]),
            ErrorKind::TooFewValues
        );
        assert_eq!(
            parse_err(&["license", "set", "a", "b", "c", "d"]),
            ErrorKind::TooManyValues
        );
        assert_eq!(
            parse_err(&["license", "set", "**", "MIT", "--grant", "all"]),
            ErrorKind::InvalidValue
        );
        assert!(matches!(
            license(&["remove", "vendor/**", "--yes"]),
            LicenseCommand::Remove {
                tree: None,
                yes: true,
                ..
            }
        ));
        assert!(matches!(
            license(&["remove", "acme/web", "vendor/**"]),
            LicenseCommand::Remove { tree: Some(_), .. }
        ));
    }

    #[test]
    fn grants_serialize_kebab_case() {
        assert_eq!(json!(Grant::ReadOnly), json!("read-only"));
    }
}
