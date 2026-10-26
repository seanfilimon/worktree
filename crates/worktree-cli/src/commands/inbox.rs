//! `wt inbox` — your notifications: list, mark read, mark done, watch
//! levels and email settings (ENGINEERING_PLAN §15.3).
//!
//! Without a subcommand it lists your unread notifications (`--all` adds
//! read ones). Notification ids may be abbreviated to any unique prefix.

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::server_common::{
    decode, fields, nullable, render, tenant_path, tree_or_default, when, yes_no, Api, Cell, Table,
    TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

const BASE: &str = "/api/v1/notifications";

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct InboxArgs {
    #[command(subcommand)]
    pub command: Option<InboxCommand>,

    /// Also list notifications you have already read
    #[arg(long)]
    pub all: bool,

    /// Maximum number of notifications to list
    #[arg(long, short = 'n', value_name = "N", default_value_t = 50)]
    pub limit: u32,
}

#[derive(Debug, Subcommand)]
pub enum InboxCommand {
    /// Mark notifications read (`--all` marks every notification read)
    Read {
        /// Notification ids (or unique prefixes)
        #[arg(
            value_name = "ID",
            required_unless_present = "all",
            conflicts_with = "all"
        )]
        ids: Vec<String>,
        /// Mark every unread notification read
        #[arg(long)]
        all: bool,
    },
    /// Mark notifications done (removes them from the inbox)
    Done {
        /// Notification ids (or unique prefixes)
        #[arg(value_name = "ID", required = true)]
        ids: Vec<String>,
    },
    /// Show or set your watch level for a tree (or with --tenant, a tenant)
    ///
    /// participating: only what involves you (review requests, mentions,
    /// your merge requests, CI failures on them, TODOs assigned to you);
    /// watching: also every merge request event, CI failure and release;
    /// ignoring: nothing at all. A tree's setting overrides its tenant's.
    Watch {
        /// TENANT/TREE (default: the tree of the current worktree)
        #[arg(id = "tree_ref", value_name = "TREE", conflicts_with = "tenant")]
        tree: Option<TreeRef>,
        /// Watch a whole tenant (every tree) instead of one tree
        #[arg(long, value_name = "TENANT")]
        tenant: Option<String>,
        /// New watch level (omit to show the current one)
        #[arg(long, value_name = "LEVEL")]
        level: Option<WatchLevel>,
        /// Also email watching notifications (when the server sends email)
        #[arg(long, conflicts_with = "no_email")]
        email: bool,
        /// Do not email watching notifications
        #[arg(long)]
        no_email: bool,
    },
    /// List your stored watch settings
    Watches,
    /// Show or change your notification settings
    Settings {
        /// Email participating notifications (review requests, mentions, ...)
        #[arg(long, value_name = "on|off")]
        email: Option<OnOff>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum WatchLevel {
    Participating,
    Watching,
    Ignoring,
}

impl WatchLevel {
    fn as_str(self) -> &'static str {
        match self {
            Self::Participating => "participating",
            Self::Watching => "watching",
            Self::Ignoring => "ignoring",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OnOff {
    On,
    Off,
}

/// `domain.Notification`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Notification {
    id: String,
    reason: String,
    title: String,
    url: String,
    created_at: String,
    read_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct NotificationList {
    #[serde(deserialize_with = "nullable")]
    notifications: Vec<Notification>,
    unread_count: u64,
}

/// `domain.WatchSetting` (plus `tenant`/`tree` names in listings).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Watch {
    level: String,
    email: bool,
    updated_at: String,
    tenant: String,
    tree: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct WatchList {
    #[serde(deserialize_with = "nullable")]
    watches: Vec<Watch>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Settings {
    email: bool,
    email_configured: bool,
}

pub fn run(ctx: &Context, args: InboxArgs) -> Result<()> {
    let out = &ctx.out;
    match args.command {
        None => {
            let api = Api::connect(ctx)?;
            let mut query = vec![("limit", args.limit.clamp(1, 1000).to_string())];
            if !args.all {
                query.push(("unread", "true".to_string()));
            }
            let value = api.get_query(BASE, &query)?;
            render(out, &value, |t, list: NotificationList| {
                list_notifications(t, &list, args.all)
            })
        }
        Some(InboxCommand::Read { ids, all }) => {
            let api = Api::connect(ctx)?;
            if all {
                let value = api.post(&format!("{BASE}/read-all"), &json!({}))?;
                let n = value["updated"].as_u64().unwrap_or(0);
                return out.emit(&value, |t| {
                    t.line(format_args!("Marked {n} notification(s) read"));
                });
            }
            let ids = resolve_ids(&api, &ids)?;
            let mut done = Vec::new();
            for id in &ids {
                done.push(api.post(&format!("{BASE}/{}/read", seg(id)), &json!({}))?);
            }
            out.emit(&Value::Array(done), |t| {
                t.line(format_args!("Marked {} notification(s) read", ids.len()));
            })
        }
        Some(InboxCommand::Done { ids }) => {
            let api = Api::connect(ctx)?;
            let ids = resolve_ids(&api, &ids)?;
            for id in &ids {
                api.delete(&format!("{BASE}/{}", seg(id)), &[])?;
            }
            out.emit(&json!({ "deleted": ids }), |t| {
                t.line(format_args!("Removed {} notification(s)", ids.len()));
            })
        }
        Some(InboxCommand::Watch {
            tree,
            tenant,
            level,
            email,
            no_email,
        }) => {
            let (path, name) = match tenant {
                Some(slug) => (
                    format!("{}/watch", tenant_path(&slug)),
                    format!("tenant {slug}"),
                ),
                None => {
                    let tree = tree_or_default(ctx, tree)?;
                    (format!("{}/watch", tree.api()), format!("tree {tree}"))
                }
            };
            let api = Api::connect(ctx)?;
            let email = (email || no_email).then_some(email);
            let value = if level.is_none() && email.is_none() {
                api.get(&path)?
            } else {
                let level = match level {
                    Some(l) => l.as_str().to_string(),
                    None => {
                        let cur: Watch = decode(&api.get(&path)?)?;
                        cur.level
                    }
                };
                api.put(&path, &watch_body(&level, email))?
            };
            render(out, &value, |t, w: Watch| show_watch(t, &w, &name))
        }
        Some(InboxCommand::Watches) => {
            let api = Api::connect(ctx)?;
            let value = api.get(&format!("{BASE}/watches"))?;
            render(out, &value, |t, list: WatchList| {
                list_watches(t, &list.watches)
            })
        }
        Some(InboxCommand::Settings { email }) => {
            let api = Api::connect(ctx)?;
            let path = format!("{BASE}/settings");
            let value = match email {
                Some(v) => api.put(&path, &json!({ "email": v == OnOff::On }))?,
                None => api.get(&path)?,
            };
            render(out, &value, |t, s: Settings| {
                fields(
                    t,
                    &[
                        ("email", yes_no(s.email).to_string()),
                        (
                            "server email",
                            if s.email_configured {
                                "configured".to_string()
                            } else {
                                "not configured (no emails are sent)".to_string()
                            },
                        ),
                    ],
                );
            })
        }
    }
}

/// The PUT body of `wt inbox watch`.
fn watch_body(level: &str, email: Option<bool>) -> Value {
    let mut body = Map::new();
    body.insert("level".into(), level.into());
    if let Some(email) = email {
        body.insert("email".into(), email.into());
    }
    Value::Object(body)
}

/// Expand abbreviated ids to full notification ids (exact ids pass
/// through untouched).
fn resolve_ids(api: &Api, ids: &[String]) -> Result<Vec<String>> {
    if ids.iter().all(|id| id.len() >= 36) {
        return Ok(ids.to_vec());
    }
    let value = api.get_query(BASE, &[("limit", "1000".to_string())])?;
    let list: NotificationList = decode(&value)?;
    let known: Vec<&str> = list.notifications.iter().map(|n| n.id.as_str()).collect();
    ids.iter().map(|id| resolve_id(&known, id)).collect()
}

fn resolve_id(known: &[&str], id: &str) -> Result<String> {
    if id.len() >= 36 {
        return Ok(id.to_string());
    }
    let matches: Vec<&&str> = known.iter().filter(|k| k.starts_with(id)).collect();
    match matches.as_slice() {
        [one] => Ok((**one).to_string()),
        [] => Err(CliError::failure(format!("no notification with id {id}"))
            .with_hint("list them with `wt inbox --all`")),
        _ => Err(CliError::usage(format!(
            "notification id prefix {id} is ambiguous ({} matches)",
            matches.len()
        ))),
    }
}

/// "tenant/tree" from an admin path like /trees/acme/web/merge-requests/1.
fn tree_of(url: &str) -> String {
    let mut parts = url.trim_start_matches('/').split('/');
    match (parts.next(), parts.next(), parts.next()) {
        (Some("trees"), Some(tenant), Some(tree)) => format!("{tenant}/{tree}"),
        _ => "-".into(),
    }
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

fn reason_cell(reason: &str) -> Cell {
    let label = reason.replace('_', " ");
    match reason {
        "review_requested" | "mentioned" => Cell::from(label).yellow(),
        "ci_failed" => Cell::from(label).red(),
        "mr_merged" => Cell::from(label).green(),
        "watching" | "release" => Cell::from(label).cyan(),
        _ => Cell::from(label),
    }
}

fn list_notifications(t: &mut Text, list: &NotificationList, all: bool) {
    let mut table = Table::new(&["", "ID", "REASON", "TREE", "WHEN", "TITLE"]);
    for n in &list.notifications {
        let unread = n.read_at.is_none();
        let title = if unread {
            Cell::from(n.title.as_str()).bold()
        } else {
            Cell::from(n.title.as_str())
        };
        table.row([
            if unread {
                Cell::from("●").yellow()
            } else {
                Cell::from(" ")
            },
            Cell::from(short(&n.id)),
            reason_cell(&n.reason),
            tree_of(&n.url).into(),
            when(&n.created_at).into(),
            title,
        ]);
    }
    let empty = if all {
        "No notifications."
    } else {
        "No unread notifications."
    };
    table.render(t, empty);
    if !list.notifications.is_empty() {
        t.blank();
        t.line(
            format!(
                "{} unread. Mark read with `wt inbox read ID` (or --all); remove with `wt inbox done ID`.",
                list.unread_count
            )
            .dimmed(),
        );
    }
}

fn show_watch(t: &mut Text, w: &Watch, name: &str) {
    let level = match w.level.as_str() {
        "watching" => w.level.cyan(),
        "ignoring" => w.level.red(),
        _ => w.level.normal(),
    };
    t.line(format_args!("{name}: {level}"));
    fields(
        t,
        &[
            ("email", yes_no(w.email).to_string()),
            ("updated", when(&w.updated_at)),
        ],
    );
}

fn list_watches(t: &mut Text, watches: &[Watch]) {
    let mut table = Table::new(&["SCOPE", "LEVEL", "EMAIL", "UPDATED"]);
    for w in watches {
        let scope = if w.tree.is_empty() {
            format!("{} (tenant)", w.tenant)
        } else {
            format!("{}/{}", w.tenant, w.tree)
        };
        table.row([
            Cell::from(scope).bold(),
            w.level.as_str().into(),
            yes_no(w.email).into(),
            when(&w.updated_at).into(),
        ]);
    }
    table.render(t, "No watch settings (you participate in every tree).");
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn inbox(args: &[&str]) -> InboxArgs {
        let mut full = vec!["inbox"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Inbox(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_listing() {
        let a = inbox(&[]);
        assert!(a.command.is_none() && !a.all && a.limit == 50);
        let a = inbox(&["--all", "-n", "5"]);
        assert!(a.all && a.limit == 5);
        assert_eq!(
            parse_err(&["inbox", "--all", "read", "abc"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn parses_subcommands() {
        let Some(InboxCommand::Read { ids, all }) = inbox(&["read", "a1", "b2"]).command else {
            panic!()
        };
        assert_eq!(
            (ids, all),
            (vec!["a1".to_string(), "b2".to_string()], false)
        );
        assert!(matches!(
            inbox(&["read", "--all"]).command,
            Some(InboxCommand::Read { all: true, .. })
        ));
        assert_eq!(
            parse_err(&["inbox", "read"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["inbox", "read", "x", "--all"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["inbox", "done"]),
            ErrorKind::MissingRequiredArgument
        );

        let Some(InboxCommand::Watch {
            tree,
            tenant,
            level,
            email,
            no_email,
        }) = inbox(&["watch", "acme/web", "--level", "watching", "--email"]).command
        else {
            panic!()
        };
        assert_eq!(tree.map(|t| t.to_string()).as_deref(), Some("acme/web"));
        assert_eq!(
            (tenant, level, email, no_email),
            (None, Some(WatchLevel::Watching), true, false)
        );
        assert!(matches!(
            inbox(&["watch", "--tenant", "acme", "--level", "ignoring"]).command,
            Some(InboxCommand::Watch {
                level: Some(WatchLevel::Ignoring),
                tenant: Some(_),
                ..
            })
        ));
        assert_eq!(
            parse_err(&["inbox", "watch", "acme/web", "--tenant", "acme"]),
            ErrorKind::ArgumentConflict
        );
        assert_eq!(
            parse_err(&["inbox", "watch", "acme/web", "--level", "loud"]),
            ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["inbox", "watch", "--email", "--no-email"]),
            ErrorKind::ArgumentConflict
        );
        assert!(matches!(
            inbox(&["settings", "--email", "off"]).command,
            Some(InboxCommand::Settings {
                email: Some(OnOff::Off)
            })
        ));
        assert!(matches!(
            inbox(&["watches"]).command,
            Some(InboxCommand::Watches)
        ));
    }

    #[test]
    fn helpers() {
        assert_eq!(tree_of("/trees/acme/web/merge-requests/3"), "acme/web");
        assert_eq!(tree_of("/notifications"), "-");
        assert_eq!(short("0123456789abcdef"), "01234567");
        assert_eq!(short("abc"), "abc");
        assert_eq!(
            watch_body("watching", Some(true)),
            json!({"level": "watching", "email": true})
        );
        assert_eq!(watch_body("ignoring", None), json!({"level": "ignoring"}));
        let known = [
            "aaaa1111-0000-0000-0000-000000000000",
            "aaab2222-0000-0000-0000-000000000000",
        ];
        assert_eq!(resolve_id(&known, "aaaa").unwrap(), known[0]);
        assert!(resolve_id(&known, "aaa").is_err());
        assert!(resolve_id(&known, "zz").is_err());
    }

    #[test]
    fn renders_list() {
        let list = NotificationList {
            notifications: vec![Notification {
                id: "0123456789".into(),
                reason: "review_requested".into(),
                title: "bob requested a review on !1".into(),
                url: "/trees/acme/web/merge-requests/1".into(),
                created_at: "2026-01-02T03:04:05Z".into(),
                ..Default::default()
            }],
            unread_count: 1,
        };
        let mut t = Text::default();
        list_notifications(&mut t, &list, false);
        let s = t.as_str();
        assert!(
            s.contains("01234567") && s.contains("acme/web") && s.contains("review requested"),
            "{s}"
        );
        let mut t = Text::default();
        list_notifications(&mut t, &NotificationList::default(), false);
        assert!(t.as_str().contains("No unread notifications."));
    }
}
