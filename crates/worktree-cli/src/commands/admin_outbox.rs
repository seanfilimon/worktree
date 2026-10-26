//! `wt admin outbox` — the server's durable email outbox
//! (docs/ENGINEERING_PLAN.md §16.11): notification emails are queued in the
//! metadata store, retried with backoff and dead-lettered after a deadline
//! (24 h by default). Server admins can list them, requeue dead ones and
//! delete entries.

use chrono::{DateTime, Utc};
use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;
use serde_json::json;
use worktree_sdk::remote::rest::seg;

use crate::commands::server_common::{
    confirm, fields, nullable, or_dash, render, timestamp, Api, Cell, Table,
};
use crate::context::Context;
use crate::error::Result;
use crate::output::format::{plural, relative_time};
use crate::output::Text;

const OUTBOX: &str = "/api/v1/admin/outbox";

/// Longest subject / error shown in the table (characters).
const SUBJECT_WIDTH: usize = 40;
const ERROR_WIDTH: usize = 60;

#[derive(Debug, Args)]
pub struct OutboxArgs {
    #[command(subcommand)]
    pub command: OutboxCommand,
}

#[derive(Debug, Subcommand)]
pub enum OutboxCommand {
    /// List outbox emails (newest first) with the queue counts
    List {
        /// Only emails in this state
        #[arg(long, value_enum, value_name = "STATUS")]
        status: Option<EmailStatus>,
        /// At most N emails (server default applies when omitted)
        #[arg(long, value_name = "N")]
        limit: Option<u32>,
    },
    /// Requeue an email for another delivery attempt (or every dead one
    /// with --all)
    Retry {
        /// Email id (from `wt admin outbox list`)
        #[arg(required_unless_present = "all", conflicts_with = "all")]
        id: Option<String>,
        /// Requeue every dead-lettered email
        #[arg(long)]
        all: bool,
    },
    /// Delete an email from the outbox (it is not sent)
    Delete {
        /// Email id
        id: String,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
    },
}

/// State of an outbox email.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EmailStatus {
    /// Waiting for (another) delivery attempt
    Pending,
    /// Delivered to the SMTP server
    Sent,
    /// Gave up after the dead-letter deadline
    Dead,
}

impl EmailStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sent => "sent",
            Self::Dead => "dead",
        }
    }
}

/// `GET /api/v1/admin/outbox`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OutboxList {
    enabled: bool,
    dead_after: String,
    dead_after_seconds: u64,
    counts: Counts,
    #[serde(deserialize_with = "nullable")]
    emails: Vec<OutboxEmail>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Counts {
    pending: u64,
    sent: u64,
    dead: u64,
}

/// One outbox email.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OutboxEmail {
    id: String,
    to: String,
    subject: String,
    status: String,
    attempts: u64,
    last_error: String,
    updated_at: String,
    next_attempt_at: String,
    sent_at: String,
    dead_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Retried {
    retried: u64,
}

fn email_path(id: &str) -> String {
    format!("{OUTBOX}/{}", seg(id))
}

/// The query of `wt admin outbox list` (only the given flags).
fn list_query(status: Option<EmailStatus>, limit: Option<u32>) -> Vec<(&'static str, String)> {
    let mut query = Vec::new();
    if let Some(s) = status {
        query.push(("status", s.as_str().to_string()));
    }
    if let Some(n) = limit {
        query.push(("limit", n.to_string()));
    }
    query
}

pub fn run(ctx: &Context, api: &Api, args: OutboxArgs) -> Result<()> {
    let out = &ctx.out;
    match args.command {
        OutboxCommand::List { status, limit } => {
            let query = list_query(status, limit);
            let value = if query.is_empty() {
                api.get(OUTBOX)?
            } else {
                api.get_query(OUTBOX, &query)?
            };
            render(out, &value, |t, list: OutboxList| {
                list_outbox(t, &list, Utc::now())
            })
        }
        OutboxCommand::Retry { all: true, .. } => {
            let value = api.post(&format!("{OUTBOX}/retry"), &json!({}))?;
            render(out, &value, |t, r: Retried| {
                t.line(format_args!(
                    "Requeued {}",
                    plural(r.retried as usize, "dead email", "dead emails")
                ));
            })
        }
        OutboxCommand::Retry { id, .. } => {
            // clap guarantees ID without --all.
            let id = id.unwrap_or_default();
            let value = api.post(&format!("{}/retry", email_path(&id)), &json!({}))?;
            render(out, &value, |t, e: OutboxEmail| {
                let id = if e.id.is_empty() { &id } else { &e.id };
                t.line(format_args!(
                    "Queued email {} for another attempt",
                    id.bold()
                ));
            })
        }
        OutboxCommand::Delete { id, yes } => {
            confirm(yes, &format!("delete outbox email {id}"))?;
            api.delete(&email_path(&id), &[])?;
            out.emit(&json!({ "id": id, "deleted": true }), |t| {
                t.line(format_args!("Deleted email {}", id.bold()));
            })
        }
    }
}

/// `text` cut to `max` characters (with an ellipsis).
fn truncate(text: &str, max: usize) -> String {
    let text = text.lines().next().unwrap_or("");
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max.saturating_sub(1)).collect();
    cut.push('…');
    cut
}

/// "3 hours ago" for past times, "in 5 minutes" for future ones.
fn relative(at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    if at <= now {
        return relative_time(at, now);
    }
    let mirrored = relative_time(now - (at - now), now);
    match mirrored.strip_suffix(" ago") {
        Some(span) => format!("in {span}"),
        None => "now".into(),
    }
}

/// The WHEN column: next attempt (pending), delivery (sent) or give-up
/// time (dead).
fn when_cell(e: &OutboxEmail, now: DateTime<Utc>) -> String {
    let (label, ts) = match e.status.as_str() {
        "pending" => ("next", &e.next_attempt_at),
        "sent" => ("sent", &e.sent_at),
        "dead" => ("dead", &e.dead_at),
        _ => ("updated", &e.updated_at),
    };
    timestamp(ts).map_or_else(|| "-".into(), |at| format!("{label} {}", relative(at, now)))
}

fn status_cell(status: &str) -> Cell {
    match status {
        "pending" => Cell::from("pending").yellow(),
        "sent" => Cell::from("sent").green(),
        "dead" => Cell::from("dead").red(),
        other => Cell::from(or_dash(other)),
    }
}

fn list_outbox(t: &mut Text, list: &OutboxList, now: DateTime<Utc>) {
    let enabled = if list.enabled {
        "yes".to_string()
    } else {
        "no (--smtp-url not set)".yellow().to_string()
    };
    let dead_after = if list.dead_after_seconds == 0 && list.dead_after.is_empty() {
        "-".to_string()
    } else if list.dead_after.is_empty() {
        format!("{}s", list.dead_after_seconds)
    } else {
        list.dead_after.clone()
    };
    let c = &list.counts;
    fields(
        t,
        &[
            ("email enabled", enabled),
            ("dead-letter after", dead_after),
            (
                "emails",
                format!("{} pending, {} sent, {} dead", c.pending, c.sent, c.dead),
            ),
        ],
    );
    t.blank();
    let mut table = Table::new(&[
        "ID",
        "STATUS",
        "TO",
        "SUBJECT",
        "ATTEMPTS",
        "WHEN",
        "LAST ERROR",
    ]);
    for e in &list.emails {
        let error = Cell::from(or_dash(&truncate(&e.last_error, ERROR_WIDTH)));
        table.row([
            Cell::from(e.id.as_str()).bold(),
            status_cell(&e.status),
            e.to.as_str().into(),
            truncate(&e.subject, SUBJECT_WIDTH).into(),
            e.attempts.to_string().into(),
            when_cell(e, now).into(),
            if e.last_error.is_empty() {
                error
            } else {
                error.red()
            },
        ]);
    }
    table.render(t, "The email outbox is empty.");
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    #[test]
    fn builds_paths_and_queries() {
        assert_eq!(email_path("em-1 x"), "/api/v1/admin/outbox/em-1%20x");
        assert!(list_query(None, None).is_empty());
        assert_eq!(
            list_query(Some(EmailStatus::Dead), Some(20)),
            vec![("status", "dead".to_string()), ("limit", "20".to_string())]
        );
        assert_eq!(
            list_query(Some(EmailStatus::Pending), None),
            vec![("status", "pending".to_string())]
        );
    }

    #[test]
    fn small_helpers() {
        assert_eq!(truncate("short", 10), "short");
        assert_eq!(truncate("0123456789abc", 10), "012345678…");
        assert_eq!(truncate("first\nsecond", 40), "first");
        let now = Utc::now();
        assert_eq!(relative(now - Duration::hours(3), now), "3 hours ago");
        assert_eq!(relative(now + Duration::minutes(5), now), "in 5 minutes");
        assert_eq!(relative(now + Duration::seconds(2), now), "now");
    }

    #[test]
    fn renders_the_outbox() {
        colored::control::set_override(false);
        let now: DateTime<Utc> = "2026-10-26T12:00:00Z".parse().expect("now");
        let list: OutboxList = serde_json::from_value(json!({
            "enabled": true, "dead_after": "24h0m0s", "dead_after_seconds": 86400,
            "counts": {"pending": 1, "sent": 1, "dead": 1},
            "emails": [
                {"id": "em-3", "to": "alice@example.com", "subject": "Merge request !4 needs your review now please",
                 "status": "pending", "attempts": 2, "last_error": "dial tcp: connection refused",
                 "created_at": "2026-10-26T11:00:00Z", "next_attempt_at": "2026-10-26T12:05:00Z",
                 "deadline_at": "2026-10-27T11:00:00Z"},
                {"id": "em-2", "to": "bob@example.com", "subject": "Welcome", "status": "sent",
                 "attempts": 1, "sent_at": "2026-10-26T09:00:00Z"},
                {"id": "em-1", "to": "carol@example.com", "subject": "Reset", "status": "dead",
                 "attempts": 9, "last_error": "550 no such user", "dead_at": "2026-10-24T12:00:00Z"}
            ]
        }))
        .expect("list");
        let mut t = Text::default();
        list_outbox(&mut t, &list, now);
        let text = t.as_str();
        for want in [
            "  email enabled      yes\n",
            "  dead-letter after  24h0m0s\n",
            "  emails             1 pending, 1 sent, 1 dead\n",
            "ID    STATUS   TO                 SUBJECT                                   ATTEMPTS  WHEN               LAST ERROR\n",
            "em-3  pending  alice@example.com  Merge request !4 needs your review now …  2         next in 5 minutes  dial tcp: connection refused\n",
            "em-2  sent     bob@example.com    Welcome                                   1         sent 3 hours ago   -\n",
            "em-1  dead     carol@example.com  Reset                                     9         dead 2 days ago    550 no such user\n",
        ] {
            assert!(text.contains(want), "missing {want:?} in\n{text}");
        }

        let empty: OutboxList = serde_json::from_value(json!({
            "enabled": false, "counts": {}, "emails": null
        }))
        .expect("empty");
        let mut t = Text::default();
        list_outbox(&mut t, &empty, now);
        assert_eq!(
            t.as_str(),
            "  email enabled      no (--smtp-url not set)\n  dead-letter after  -\n  emails             0 pending, 0 sent, 0 dead\n\nThe email outbox is empty.\n"
        );
    }
}
