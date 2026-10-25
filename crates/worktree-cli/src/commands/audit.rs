//! `wt audit` — query the server audit log (newest first).

use std::collections::BTreeMap;
use std::io::{ErrorKind, Write};

use chrono::{DateTime, SecondsFormat, Utc};
use clap::{Args, ValueEnum};
use serde::Deserialize;

use crate::commands::server_common::{or_dash, render, when, Api, Cell, Table};
use crate::context::Context;
use crate::error::Result;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct AuditArgs {
    /// Only events of this tenant (default: every tenant you administer)
    #[arg(long, value_name = "T")]
    pub tenant: Option<String>,

    /// Only events with this action, e.g. `branch.push` or `tree.create`;
    /// a trailing `.*` matches a prefix, e.g. `iam.*`
    #[arg(long, value_name = "A")]
    pub action: Option<String>,

    /// Only events on this resource type, e.g. `tree` or `account`
    #[arg(long, value_name = "T")]
    pub resource_type: Option<String>,

    /// Only events on this resource id
    #[arg(long, value_name = "ID")]
    pub resource_id: Option<String>,

    /// Only events at or after this RFC 3339 time, e.g. 2026-01-31T00:00:00Z
    #[arg(long, value_name = "RFC3339", value_parser = parse_since)]
    pub since: Option<DateTime<Utc>>,

    /// Only events before this RFC 3339 time (exclusive)
    #[arg(long, value_name = "RFC3339", value_parser = parse_since)]
    pub until: Option<DateTime<Utc>>,

    /// Maximum number of events (server default: 100; ignored with
    /// `--format ndjson`)
    #[arg(short = 'n', value_name = "N", value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub max_count: Option<u32>,

    /// Output format: `table`, or `ndjson` to export every matching event
    /// as one JSON object per line (no limit)
    #[arg(long, value_enum, value_name = "FORMAT", default_value_t = AuditFormat::Table)]
    pub format: AuditFormat,
}

/// `wt audit --format`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum AuditFormat {
    /// Aligned table (the JSON array with `--json`)
    Table,
    /// Newline-delimited JSON export of every match
    Ndjson,
}

/// `domain.AuditEvent`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AuditEvent {
    actor: String,
    action: String,
    resource_type: String,
    resource_id: String,
    details: BTreeMap<String, String>,
    remote_addr: String,
    at: String,
}

fn parse_since(s: &str) -> std::result::Result<DateTime<Utc>, String> {
    DateTime::parse_from_rfc3339(s)
        .map(|t| t.with_timezone(&Utc))
        .map_err(|e| format!("not an RFC 3339 timestamp ({e})"))
}

fn rfc3339(t: Option<DateTime<Utc>>) -> String {
    t.map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
        .unwrap_or_default()
}

/// Query parameters of `GET /api/v1/audit` (empty values are dropped by
/// the client).
fn query(args: &AuditArgs) -> Vec<(&'static str, String)> {
    let ndjson = args.format == AuditFormat::Ndjson;
    let opt = |v: &Option<String>| v.clone().unwrap_or_default();
    vec![
        ("tenant", opt(&args.tenant)),
        ("action", opt(&args.action)),
        ("resource_type", opt(&args.resource_type)),
        ("resource_id", opt(&args.resource_id)),
        ("since", rfc3339(args.since)),
        ("until", rfc3339(args.until)),
        (
            "limit",
            match args.max_count {
                Some(n) if !ndjson => n.to_string(),
                _ => String::new(),
            },
        ),
        (
            "format",
            if ndjson {
                "ndjson".into()
            } else {
                String::new()
            },
        ),
    ]
}

pub fn run(ctx: &Context, args: AuditArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let query = query(&args);
    if args.format == AuditFormat::Ndjson {
        let body = api.block_on(api.client().get_bytes_query("/api/v1/audit", &query))?;
        let mut stdout = std::io::stdout().lock();
        return match stdout.write_all(&body).and_then(|()| stdout.flush()) {
            Err(e) if e.kind() != ErrorKind::BrokenPipe => Err(e.into()),
            _ => Ok(()),
        };
    }
    let value = api.get_query("/api/v1/audit", &query)?;
    render(&ctx.out, &value, |t, events: Vec<AuditEvent>| {
        table(t, &events)
    })
}

fn table(t: &mut Text, events: &[AuditEvent]) {
    let mut table = Table::new(&["WHEN", "ACTOR", "ACTION", "RESOURCE", "FROM", "DETAILS"]);
    for e in events {
        let resource = match (e.resource_type.is_empty(), e.resource_id.is_empty()) {
            (true, _) => "-".to_string(),
            (false, true) => e.resource_type.clone(),
            (false, false) => format!("{}:{}", e.resource_type, e.resource_id),
        };
        let details: Vec<String> = e.details.iter().map(|(k, v)| format!("{k}={v}")).collect();
        table.row([
            when(&e.at).into(),
            Cell::from(e.actor.as_str()).bold(),
            Cell::from(e.action.as_str()).cyan(),
            resource.into(),
            or_dash(&e.remote_addr).into(),
            details.join(" ").into(),
        ]);
    }
    table.render(t, "No audit events.");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use serde_json::json;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn audit(args: &[&str]) -> AuditArgs {
        let mut full = vec!["audit"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Audit(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_filters() {
        let a = audit(&[]);
        assert!(a.tenant.is_none() && a.action.is_none() && a.since.is_none());
        assert_eq!(a.max_count, None);
        let a = audit(&[
            "--tenant",
            "acme",
            "--action",
            "tree.create",
            "--since",
            "2026-01-31T10:00:00+02:00",
            "-n",
            "20",
        ]);
        assert_eq!(a.tenant.as_deref(), Some("acme"));
        assert_eq!(a.action.as_deref(), Some("tree.create"));
        assert_eq!(
            a.since
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true)),
            Some("2026-01-31T08:00:00Z".to_string())
        );
        assert_eq!(a.max_count, Some(20));
        assert_eq!(
            parse_err(&["audit", "--since", "yesterday"]),
            ErrorKind::ValueValidation
        );
        assert_eq!(parse_err(&["audit", "-n", "0"]), ErrorKind::ValueValidation);
        assert_eq!(a.format, AuditFormat::Table);
    }

    fn param(q: &[(&str, String)], key: &str) -> String {
        q.iter()
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| panic!("no {key} parameter"))
    }

    #[test]
    fn parses_resource_filters_and_builds_the_query() {
        let a = audit(&[
            "--action",
            "iam.*",
            "--resource-type",
            "tree",
            "--resource-id",
            "t1",
            "--until",
            "2026-02-01T01:00:00+01:00",
            "-n",
            "5",
        ]);
        assert_eq!(a.resource_type.as_deref(), Some("tree"));
        assert_eq!(a.resource_id.as_deref(), Some("t1"));
        let q = query(&a);
        assert_eq!(param(&q, "action"), "iam.*");
        assert_eq!(param(&q, "resource_type"), "tree");
        assert_eq!(param(&q, "resource_id"), "t1");
        assert_eq!(param(&q, "until"), "2026-02-01T00:00:00Z");
        assert_eq!(param(&q, "since"), "");
        assert_eq!(param(&q, "limit"), "5");
        assert_eq!(param(&q, "format"), "");

        let a = audit(&["--format", "ndjson", "-n", "5"]);
        assert_eq!(a.format, AuditFormat::Ndjson);
        let q = query(&a);
        assert_eq!(param(&q, "format"), "ndjson");
        assert_eq!(param(&q, "limit"), "", "ndjson exports are unlimited");

        assert_eq!(
            parse_err(&["audit", "--format", "csv"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["audit", "--until", "tomorrow"]),
            ErrorKind::ValueValidation
        );
    }

    #[test]
    fn renders_events() {
        colored::control::set_override(false);
        let events: Vec<AuditEvent> = serde_json::from_value(json!([{
            "actor": "admin", "action": "tenant.create", "resource_type": "tenant",
            "resource_id": "t1", "details": {"slug": "acme", "a": "b"}, "at": ""
        }]))
        .expect("events");
        let mut t = Text::default();
        table(&mut t, &events);
        assert_eq!(
            t.as_str(),
            "WHEN  ACTOR  ACTION         RESOURCE   FROM  DETAILS\n-     admin  tenant.create  tenant:t1  -     a=b slug=acme\n"
        );
    }
}
