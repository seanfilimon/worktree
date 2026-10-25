//! `wt audit` — query the server audit log (newest first).

use std::collections::BTreeMap;

use chrono::{DateTime, SecondsFormat, Utc};
use clap::Args;
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

    /// Only events with this action, e.g. `branch.push` or `tree.create`
    #[arg(long, value_name = "A")]
    pub action: Option<String>,

    /// Only events at or after this RFC 3339 time, e.g. 2026-01-31T00:00:00Z
    #[arg(long, value_name = "RFC3339", value_parser = parse_since)]
    pub since: Option<DateTime<Utc>>,

    /// Maximum number of events (server default: 100)
    #[arg(short = 'n', value_name = "N", value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub max_count: Option<u32>,
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

pub fn run(ctx: &Context, args: AuditArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let query = [
        ("tenant", args.tenant.unwrap_or_default()),
        ("action", args.action.unwrap_or_default()),
        (
            "since",
            args.since
                .map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
                .unwrap_or_default(),
        ),
        (
            "limit",
            args.max_count.map(|n| n.to_string()).unwrap_or_default(),
        ),
    ];
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
