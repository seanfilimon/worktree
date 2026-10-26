//! `wt attr` — ABAC attributes of tenants, accounts and trees: show, set,
//! unset (ENGINEERING_PLAN §16.5).
//!
//! Attributes are tested by the attribute conditions of access policies
//! (`principal.<key>` = the caller's account, `tree.<key>`, `tenant.<key>`).
//! Setting tenant attributes needs `tenant:admin`, tree attributes
//! `tree:admin`, account attributes a server admin.

use std::collections::BTreeMap;

use clap::{Args, Subcommand, ValueEnum};
use serde::Deserialize;
use serde_json::{json, Value};
use worktree_sdk::remote::access::attributes;
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{call, show, tenant_path, Table, TreeRef};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct AttrArgs {
    #[command(subcommand)]
    pub command: AttrCommand,
}

/// What carries the attributes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AttrKind {
    /// A tenant (TARGET = tenant slug)
    Tenant,
    /// An account (TARGET = username, or `me` for `show`)
    Account,
    /// A server tree (TARGET = TENANT/TREE)
    Tree,
}

#[derive(Debug, Subcommand)]
pub enum AttrCommand {
    /// Show the attributes of a tenant, account or tree
    ///
    /// `wt attr show account me` shows your own account's attributes;
    /// other accounts need a server admin.
    Show {
        #[arg(value_enum)]
        kind: AttrKind,
        /// Tenant slug, username (or `me`), or TENANT/TREE
        target: String,
    },
    /// Set attributes (KEY=VALUE...), keeping the others
    ///
    /// Keys are `[a-z][a-z0-9_.-]{0,63}`, values 1-256 bytes; at most 64
    /// attributes per object.
    Set {
        #[arg(value_enum)]
        kind: AttrKind,
        /// Tenant slug, username, or TENANT/TREE
        target: String,
        /// `KEY=VALUE` pairs
        #[arg(required = true, value_name = "KEY=VALUE")]
        pairs: Vec<String>,
    },
    /// Remove attributes by key
    Unset {
        #[arg(value_enum)]
        kind: AttrKind,
        /// Tenant slug, username, or TENANT/TREE
        target: String,
        /// Keys to remove
        #[arg(required = true, value_name = "KEY")]
        keys: Vec<String>,
    },
}

#[derive(Debug, Deserialize)]
struct AttributesBody {
    #[serde(default)]
    attributes: BTreeMap<String, String>,
}

/// A resolved attribute target: its REST path and a display label.
#[derive(Debug, PartialEq, Eq)]
struct Target {
    path: String,
    label: String,
}

fn target(kind: AttrKind, target: &str, writing: bool) -> Result<Target> {
    match kind {
        AttrKind::Tenant => Ok(Target {
            path: tenant_path(target, "/attributes"),
            label: format!("tenant {target}"),
        }),
        AttrKind::Account if target == "me" => {
            if writing {
                return Err(CliError::usage(
                    "accounts cannot change their own attributes: they feed access decisions",
                )
                .with_hint("a server admin can run `wt attr set account <username> KEY=VALUE`"));
            }
            Ok(Target {
                path: "/api/v1/me/attributes".into(),
                label: "your account".into(),
            })
        }
        AttrKind::Account => Ok(Target {
            path: format!("/api/v1/admin/accounts/{}/attributes", seg(target)),
            label: format!("account {target}"),
        }),
        AttrKind::Tree => {
            let tree: TreeRef = target.parse().map_err(CliError::usage)?;
            Ok(Target {
                path: tree.path("/attributes"),
                label: format!("tree {tree}"),
            })
        }
    }
}

/// Parse `KEY=VALUE` arguments (later pairs win), checking keys and values.
fn parse_pairs(pairs: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for pair in pairs {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(CliError::usage(format!("expected KEY=VALUE, got '{pair}'"))
                .with_hint("remove attributes with `wt attr unset`"));
        };
        let key = key.trim();
        if !attributes::valid_key(key) {
            return Err(CliError::usage(format!(
                "invalid attribute key '{key}': keys are [a-z][a-z0-9_.-]{{0,63}}"
            )));
        }
        attributes::check_value(value)
            .map_err(|e| CliError::usage(format!("attribute '{key}': {e}")))?;
        out.insert(key.to_string(), value.to_string());
    }
    Ok(out)
}

fn check_keys(keys: &[String]) -> Result<()> {
    for key in keys {
        if !attributes::valid_key(key) {
            return Err(CliError::usage(format!(
                "invalid attribute key '{key}': keys are [a-z][a-z0-9_.-]{{0,63}}"
            )));
        }
    }
    Ok(())
}

fn attributes_text(t: &mut Text, label: &str, attrs: &BTreeMap<String, String>) {
    let mut table = Table::new(&["key", "value"]);
    for (k, v) in attrs {
        table.row([k.clone(), v.clone()]);
    }
    table.render(t, &format!("No attributes on {label}."));
}

pub fn run(ctx: &Context, args: AttrArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        AttrCommand::Show { kind, target: t } => {
            let target = target(kind, &t, false)?;
            let body: Value = call(client.get(&target.path))?;
            show(&ctx.out, &body, |text, b: &AttributesBody| {
                attributes_text(text, &target.label, &b.attributes)
            })
        }
        AttrCommand::Set {
            kind,
            target: t,
            pairs,
        } => {
            let target = target(kind, &t, true)?;
            let changes = parse_pairs(&pairs)?;
            let current: AttributesBody = call(client.get(&target.path))?;
            let mut next = current.attributes;
            next.extend(changes.clone());
            attributes::check_attributes(&next).map_err(CliError::usage)?;
            let body: Value = call(client.put(&target.path, &json!({ "attributes": next })))?;
            show(&ctx.out, &body, |text, b: &AttributesBody| {
                text.line(format_args!(
                    "Set {} on {}",
                    changes.keys().cloned().collect::<Vec<_>>().join(", "),
                    target.label
                ));
                text.blank();
                attributes_text(text, &target.label, &b.attributes);
            })
        }
        AttrCommand::Unset {
            kind,
            target: t,
            keys,
        } => {
            let target = target(kind, &t, true)?;
            check_keys(&keys)?;
            let current: AttributesBody = call(client.get(&target.path))?;
            let mut next = current.attributes;
            let missing: Vec<&str> = keys
                .iter()
                .filter(|k| next.remove(k.as_str()).is_none())
                .map(String::as_str)
                .collect();
            let body: Value = call(client.put(&target.path, &json!({ "attributes": next })))?;
            show(&ctx.out, &body, |text, b: &AttributesBody| {
                let removed: Vec<&str> = keys
                    .iter()
                    .map(String::as_str)
                    .filter(|k| !missing.contains(k))
                    .collect();
                if removed.is_empty() {
                    text.line(format_args!("Nothing to remove on {}", target.label));
                } else {
                    text.line(format_args!(
                        "Removed {} from {}",
                        removed.join(", "),
                        target.label
                    ));
                }
                if !missing.is_empty() {
                    text.line(format_args!("Not set: {}", missing.join(", ")));
                }
                text.blank();
                attributes_text(text, &target.label, &b.attributes);
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::{parse, parse_err};

    fn attr(args: &[&str]) -> AttrCommand {
        let mut argv = vec!["attr"];
        argv.extend_from_slice(args);
        match parse(&argv) {
            Command::Attr(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_subcommands() {
        assert!(matches!(
            attr(&["show", "tenant", "acme"]),
            AttrCommand::Show { kind: AttrKind::Tenant, target } if target == "acme"
        ));
        assert!(matches!(
            attr(&["set", "tree", "acme/web", "classification=secret", "owner=ops"]),
            AttrCommand::Set { kind: AttrKind::Tree, pairs, .. } if pairs.len() == 2
        ));
        assert!(matches!(
            attr(&["unset", "account", "ada", "clearance"]),
            AttrCommand::Unset { kind: AttrKind::Account, keys, .. } if keys == ["clearance"]
        ));
        assert_eq!(
            parse_err(&["attr", "set", "tenant", "acme"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["attr", "show", "team", "x"]),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn targets_resolve_to_paths() {
        assert_eq!(
            target(AttrKind::Tenant, "acme", true).unwrap(),
            Target {
                path: "/api/v1/tenants/acme/attributes".into(),
                label: "tenant acme".into()
            }
        );
        assert_eq!(
            target(AttrKind::Account, "ada", true).unwrap().path,
            "/api/v1/admin/accounts/ada/attributes"
        );
        assert_eq!(
            target(AttrKind::Account, "me", false).unwrap().path,
            "/api/v1/me/attributes"
        );
        assert!(target(AttrKind::Account, "me", true).is_err());
        assert_eq!(
            target(AttrKind::Tree, "acme/web", false).unwrap().path,
            "/api/v1/trees/acme/web/attributes"
        );
        assert!(target(AttrKind::Tree, "web", false).is_err());
    }

    #[test]
    fn pairs_are_checked() {
        let m = parse_pairs(&["a=1".into(), "org.unit=x=y".into(), "a=2".into()]).unwrap();
        assert_eq!(m["a"], "2");
        assert_eq!(m["org.unit"], "x=y");
        for bad in ["novalue", "Bad=1", "k=", "=v"] {
            assert!(parse_pairs(&[bad.to_string()]).is_err(), "{bad}");
        }
        assert!(check_keys(&["ok".into()]).is_ok());
        assert!(check_keys(&["NO".into()]).is_err());
    }

    #[test]
    fn renders_attribute_tables() {
        let mut t = Text::default();
        let mut attrs = BTreeMap::new();
        attrs.insert("region".to_string(), "eu-west".to_string());
        attrs.insert("cost.center".to_string(), "r-d".to_string());
        attributes_text(&mut t, "tenant acme", &attrs);
        assert_eq!(
            t.as_str(),
            "KEY          VALUE\ncost.center  r-d\nregion       eu-west\n"
        );
        let mut t = Text::default();
        attributes_text(&mut t, "tenant acme", &BTreeMap::new());
        assert_eq!(t.as_str(), "No attributes on tenant acme.\n");
    }
}
