//! `wt events` — stream live server events until Ctrl-C, one line each
//! (one JSON object per line with `--json`).

use std::io::{ErrorKind, Write};

use chrono::Local;
use clap::Args;
use colored::Colorize;
use serde_json::Value;

use crate::commands::server_common::{timestamp, Api, TreeRef};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::short;

#[derive(Debug, Args)]
pub struct EventsArgs {
    /// Only events of this tenant
    #[arg(long, value_name = "T")]
    pub tenant: Option<String>,

    /// Only events of this tree (TENANT/TREE)
    // Shares the id of the global `--tree` so it replaces that flag here.
    #[arg(long, value_name = "TREE", value_parser = tree_spec)]
    pub tree: Option<String>,

    /// Only events of this type, e.g. `branch.pushed` (repeatable)
    #[arg(long = "type", value_name = "TYPE")]
    pub types: Vec<String>,
}

fn tree_spec(spec: &str) -> std::result::Result<String, String> {
    spec.parse::<TreeRef>().map(|_| spec.to_string())
}

/// Event types are padded to this width (the longest built-in type).
const KIND_WIDTH: usize = 15;

/// Payload fields worth showing, in display order.
const SUMMARY_FIELDS: &[&str] = &[
    "number",
    "name",
    "title",
    "branch_name",
    "source_branch",
    "target_branch",
    "state",
    "tip",
    "target",
    "snapshot_id",
];

pub fn run(ctx: &Context, args: EventsArgs) -> Result<()> {
    let query = scope(args.tenant, args.tree.as_deref())?;
    let api = Api::connect(ctx)?;
    let json = ctx.out.is_json();
    let mut failure = None;
    let stream = api.client().events(&query, |kind, data| {
        if !args.types.is_empty() && !args.types.iter().any(|t| t == kind) {
            return true;
        }
        let line = if json {
            data.to_string()
        } else {
            human_line(kind, &data)
        };
        match write_line(&line) {
            Ok(()) => true,
            Err(e) => {
                if e.kind() != ErrorKind::BrokenPipe {
                    failure = Some(e);
                }
                false
            }
        }
    });
    api.block_on(async {
        tokio::select! {
            result = stream => result.map_err(CliError::from),
            signal = tokio::signal::ctrl_c() => signal.map_err(CliError::from),
        }
    })?;
    failure.map_or(Ok(()), |e| Err(e.into()))
}

/// The `tenant` / `tree` query of `GET /events`.
fn scope(tenant: Option<String>, tree: Option<&str>) -> Result<[(&'static str, String); 2]> {
    let Some(spec) = tree else {
        return Ok([
            ("tenant", tenant.unwrap_or_default()),
            ("tree", String::new()),
        ]);
    };
    let tree: TreeRef = spec.parse().map_err(CliError::usage)?;
    if let Some(t) = tenant.filter(|t| *t != tree.tenant) {
        return Err(CliError::usage(format!(
            "--tenant {t} does not match --tree {tree}"
        )));
    }
    Ok([("tenant", tree.tenant), ("tree", tree.name)])
}

fn write_line(line: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{line}")?;
    stdout.flush()
}

/// `14:03:07  branch.created  ada  name=feature tip=0123abcd`.
fn human_line(kind: &str, event: &Value) -> String {
    let at = event
        .get("at")
        .and_then(Value::as_str)
        .and_then(timestamp)
        .map_or_else(
            || "--:--:--".to_string(),
            |t| t.with_timezone(&Local).format("%H:%M:%S").to_string(),
        );
    let actor = event.get("actor").and_then(Value::as_str).unwrap_or("-");
    let kind = format!("{kind:<KIND_WIDTH$}");
    let mut line = format!("{}  {}  {}", at.dimmed(), kind.cyan().bold(), actor.bold());
    let summary = event.get("payload").map(summarize).unwrap_or_default();
    if !summary.is_empty() {
        line.push_str("  ");
        line.push_str(&summary);
    }
    line
}

/// `key=value` pairs of the interesting scalar payload fields.
fn summarize(payload: &Value) -> String {
    let Some(fields) = payload.as_object() else {
        return String::new();
    };
    SUMMARY_FIELDS
        .iter()
        .filter_map(|&key| {
            let value = match fields.get(key)? {
                Value::String(s) if s.is_empty() => return None,
                Value::String(s) if matches!(key, "tip" | "target" | "snapshot_id") => {
                    short(s).to_string()
                }
                Value::String(s) => s.clone(),
                Value::Number(n) => n.to_string(),
                _ => return None,
            };
            Some(format!("{key}={value}"))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind as ClapErrorKind;
    use serde_json::json;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn events(args: &[&str]) -> EventsArgs {
        let mut full = vec!["events"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Events(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_filters() {
        let a = events(&[]);
        assert!(a.tenant.is_none() && a.tree.is_none() && a.types.is_empty());
        let a = events(&[
            "--tenant",
            "acme",
            "--tree",
            "acme/web",
            "--type",
            "branch.pushed",
            "--type",
            "mr.opened",
        ]);
        assert_eq!(a.tenant.as_deref(), Some("acme"));
        assert_eq!(a.tree.as_deref(), Some("acme/web"));
        assert_eq!(a.types, ["branch.pushed", "mr.opened"]);
        assert_eq!(
            parse_err(&["events", "--tree", "web"]),
            ClapErrorKind::ValueValidation
        );
    }

    #[test]
    fn scopes() {
        let q = scope(None, None).expect("unscoped");
        assert_eq!(q, [("tenant", String::new()), ("tree", String::new())]);
        let q = scope(Some("acme".into()), None).expect("tenant");
        assert_eq!(q[0].1, "acme");
        let q = scope(None, Some("acme/web")).expect("tree");
        assert_eq!(q, [("tenant", "acme".into()), ("tree", "web".into())]);
        assert!(scope(Some("acme".into()), Some("acme/web")).is_ok());
        let err = scope(Some("other".into()), Some("acme/web")).expect_err("mismatch");
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn human_lines() {
        colored::control::set_override(false);
        let event = json!({
            "type": "branch.created",
            "actor": "ada",
            "at": "0001-01-01T00:00:00Z",
            "payload": {"name": "feature", "tip": "0123456789abcdef", "protected": false, "tree_id": "x"}
        });
        assert_eq!(
            human_line("branch.created", &event),
            "--:--:--  branch.created   ada  name=feature tip=01234567"
        );
        let mr =
            json!({"payload": {"number": 3, "title": "Fix", "state": "open", "description": ""}});
        assert_eq!(
            human_line("mr.opened", &mr),
            "--:--:--  mr.opened        -  number=3 title=Fix state=open"
        );
        assert_eq!(summarize(&json!("text")), "");
    }
}
