//! `wt admin retention` — the server's retention policy
//! (docs/ENGINEERING_PLAN.md §16.11): how long policy versions, audit
//! events, webhook deliveries and backups are kept, the last pruning run,
//! and `wt admin retention run` to prune now.

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use serde_json::json;

use crate::commands::server_common::{fields, render, when, yes_no, Api};
use crate::context::Context;
use crate::error::Result;
use crate::output::format::plural;
use crate::output::Text;

const RETENTION: &str = "/api/v1/admin/retention";

#[derive(Debug, Args)]
pub struct RetentionArgs {
    /// Without a subcommand: show the retention settings and the last run
    #[command(subcommand)]
    pub command: Option<RetentionCommand>,
}

#[derive(Debug, Subcommand)]
pub enum RetentionCommand {
    /// Prune everything past its retention now and print what was removed
    Run,
}

/// `GET /api/v1/admin/retention`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RetentionStatus {
    interval: String,
    config: RetentionConfig,
    running: bool,
    last_run_at: String,
    last_error: String,
    last_report: Option<RetentionReport>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RetentionConfig {
    policy_versions: Rule,
    audit: Rule,
    webhook_deliveries: Rule,
    backups: Rule,
}

/// One category's limits; 0 = no limit.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Rule {
    max_age_seconds: u64,
    keep: u64,
}

/// `POST /api/v1/admin/retention/run` (and the last run's report).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct RetentionReport {
    policy_versions_pruned: u64,
    audit_pruned: u64,
    webhook_deliveries_pruned: u64,
    backups_pruned: u64,
    took_ms: u64,
}

pub fn run(ctx: &Context, api: &Api, args: RetentionArgs) -> Result<()> {
    let out = &ctx.out;
    match args.command {
        None => {
            let value = api.get(RETENTION)?;
            render(out, &value, |t, s: RetentionStatus| status(t, &s))
        }
        Some(RetentionCommand::Run) => {
            let value = api.post(&format!("{RETENTION}/run"), &json!({}))?;
            render(out, &value, |t, r: RetentionReport| {
                t.line(format_args!(
                    "Retention run finished in {} ms",
                    r.took_ms.to_string().bold()
                ));
                fields(t, &report_rows(&r));
            })
        }
    }
}

/// "90d", "36h", "15m", "45s".
fn span(seconds: u64) -> String {
    const MINUTE: u64 = 60;
    const HOUR: u64 = 60 * MINUTE;
    const DAY: u64 = 24 * HOUR;
    match seconds {
        s if s >= DAY && s % DAY == 0 => format!("{}d", s / DAY),
        s if s >= HOUR && s % HOUR == 0 => format!("{}h", s / HOUR),
        s if s >= MINUTE && s % MINUTE == 0 => format!("{}m", s / MINUTE),
        s => format!("{s}s"),
    }
}

/// What a rule prunes: "older than 90d or beyond 100 per policy", "keep all".
fn describe(rule: &Rule, per: &str) -> String {
    let age =
        (rule.max_age_seconds > 0).then(|| format!("older than {}", span(rule.max_age_seconds)));
    let count = (rule.keep > 0).then(|| format!("beyond {}{per}", rule.keep));
    match (age, count) {
        (Some(a), Some(c)) => format!("{a} or {c}"),
        (Some(a), None) => a,
        (None, Some(c)) => c,
        (None, None) => "keep all".into(),
    }
}

fn report_rows(r: &RetentionReport) -> Vec<(&'static str, String)> {
    vec![
        ("policy versions", r.policy_versions_pruned.to_string()),
        ("audit events", r.audit_pruned.to_string()),
        (
            "webhook deliveries",
            r.webhook_deliveries_pruned.to_string(),
        ),
        ("backups", r.backups_pruned.to_string()),
    ]
}

/// "pruned 3 policy versions, 10 audit events, 0 webhook deliveries,
/// 1 backup in 12 ms".
fn report_summary(r: &RetentionReport) -> String {
    let n = |v: u64| v as usize;
    format!(
        "pruned {}, {}, {}, {} in {} ms",
        plural(
            n(r.policy_versions_pruned),
            "policy version",
            "policy versions"
        ),
        plural(n(r.audit_pruned), "audit event", "audit events"),
        plural(
            n(r.webhook_deliveries_pruned),
            "webhook delivery",
            "webhook deliveries"
        ),
        plural(n(r.backups_pruned), "backup", "backups"),
        r.took_ms
    )
}

fn status(t: &mut Text, s: &RetentionStatus) {
    let c = &s.config;
    let interval = match s.interval.as_str() {
        "" => "-".to_string(),
        "0s" => "off".to_string(),
        other => format!("every {other}"),
    };
    let last_run = if s.last_run_at.is_empty() {
        "never".to_string()
    } else {
        when(&s.last_run_at)
    };
    let mut rows = vec![
        (
            "policy versions",
            describe(&c.policy_versions, " per policy"),
        ),
        ("audit events", describe(&c.audit, "")),
        (
            "webhook deliveries",
            describe(&c.webhook_deliveries, " per webhook"),
        ),
        ("backups", describe(&c.backups, "")),
        ("schedule", interval),
        ("running", yes_no(s.running).into()),
        ("last run", last_run),
    ];
    if let Some(r) = &s.last_report {
        rows.push(("last result", report_summary(r)));
    }
    if !s.last_error.is_empty() {
        rows.push(("last error", s.last_error.red().to_string()));
    }
    fields(t, &rows);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_rules() {
        assert_eq!(span(90 * 86_400), "90d");
        assert_eq!(span(36 * 3600), "36h");
        assert_eq!(span(900), "15m");
        assert_eq!(span(45), "45s");
        assert_eq!(span(86_400 + 1), "86401s");
        let rule = |max_age_seconds, keep| Rule {
            max_age_seconds,
            keep,
        };
        assert_eq!(
            describe(&rule(90 * 86_400, 100), " per policy"),
            "older than 90d or beyond 100 per policy"
        );
        assert_eq!(describe(&rule(3600, 0), ""), "older than 1h");
        assert_eq!(describe(&rule(0, 7), ""), "beyond 7");
        assert_eq!(describe(&rule(0, 0), " per policy"), "keep all");
    }

    #[test]
    fn renders_status_and_reports() {
        colored::control::set_override(false);
        let s: RetentionStatus = serde_json::from_value(json!({
            "interval": "1h0m0s",
            "config": {
                "policy_versions": {"max_age": "2160h0m0s", "max_age_seconds": 7_776_000, "keep": 100},
                "audit": {"max_age": "8760h0m0s", "max_age_seconds": 31_536_000},
                "webhook_deliveries": {"max_age": "0s", "max_age_seconds": 0, "keep": 0},
                "backups": {"max_age": "720h0m0s", "max_age_seconds": 2_592_000, "keep": 7}
            },
            "running": false,
            "last_run_at": "2020-01-01T00:00:00Z",
            "last_error": "disk full",
            "last_report": {"at": "2020-01-01T00:00:00Z", "policy_versions_pruned": 3,
                            "audit_pruned": 1, "webhook_deliveries_pruned": 0,
                            "backups_pruned": 2, "took_ms": 12}
        }))
        .expect("status");
        let mut t = Text::default();
        status(&mut t, &s);
        let text = t.as_str();
        for want in [
            "  policy versions     older than 90d or beyond 100 per policy\n",
            "  audit events        older than 365d\n",
            "  webhook deliveries  keep all\n",
            "  backups             older than 30d or beyond 7\n",
            "  schedule            every 1h0m0s\n",
            "  running             no\n",
            "  last result         pruned 3 policy versions, 1 audit event, 0 webhook deliveries, 2 backups in 12 ms\n",
            "  last error          disk full\n",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }
        assert!(text.contains("  last run            "), "{text}");
        assert!(text.contains("years ago"), "{text}");

        let fresh: RetentionStatus =
            serde_json::from_value(json!({"interval": "0s", "config": {}})).expect("fresh");
        let mut t = Text::default();
        status(&mut t, &fresh);
        let text = t.as_str();
        assert!(text.contains("  schedule            off\n"), "{text}");
        assert!(text.contains("  last run            never\n"), "{text}");
        assert!(!text.contains("last result"), "{text}");

        let r: RetentionReport = serde_json::from_value(json!({
            "policy_versions_pruned": 1, "audit_pruned": 2, "took_ms": 5
        }))
        .expect("report");
        let mut t = Text::default();
        fields(&mut t, &report_rows(&r));
        assert_eq!(
            t.as_str(),
            "  policy versions     1\n  audit events        2\n  webhook deliveries  0\n  backups             0\n"
        );
    }
}
