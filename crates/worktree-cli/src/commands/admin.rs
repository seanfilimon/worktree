//! `wt admin` — server administration: accounts, system information,
//! statistics (server admins only).

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Map};
use worktree_sdk::remote::rest::seg;

use crate::commands::server_common::{
    bytes, fields, new_password, render, timestamp, when, yes_no, Api, Cell, Table,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::date;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// List every account on the server
    Accounts,
    /// Create an account (bypasses the registration setting)
    AccountCreate {
        username: String,
        /// Email address
        #[arg(long, value_name = "EMAIL")]
        email: String,
        /// Make the account a server admin
        #[arg(long)]
        admin: bool,
        /// Read the password from the first line of stdin
        #[arg(long)]
        password_stdin: bool,
    },
    /// Change an account's admin or disabled flag
    AccountSet {
        username: String,
        /// Grant or revoke server admin
        #[arg(long, value_name = "BOOL")]
        admin: Option<bool>,
        /// Disable or re-enable the account
        #[arg(long, value_name = "BOOL")]
        disabled: Option<bool>,
    },
    /// Set an account's password (revokes its tokens)
    Password {
        username: String,
        /// Read the password from the first line of stdin
        #[arg(long)]
        password_stdin: bool,
    },
    /// Issue a one-time password reset code for an account
    ResetCode {
        username: String,
        /// Hours until the code expires (server default: 1)
        #[arg(long, value_name = "N")]
        ttl_hours: Option<u32>,
    },
    /// Show server version, runtime and storage information
    System,
    /// Show object counts across the server
    Stats,
}

/// `domain.Account` as the CLI displays it.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Account {
    pub username: String,
    pub email: String,
    pub is_admin: bool,
    pub disabled: bool,
    pub created_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ResetCode {
    code: String,
    expires_at: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct SystemInfo {
    version: String,
    go_version: String,
    os: String,
    arch: String,
    started_at: String,
    uptime_seconds: u64,
    allow_registration: bool,
    data_dir: String,
    num_cpu: u64,
    num_goroutine: u64,
    memory: Memory,
    storage: Storage,
    event_bus: EventBus,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Memory {
    alloc_bytes: u64,
    sys_bytes: u64,
    heap_inuse_bytes: u64,
    gc_runs: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Storage {
    objects: u64,
    object_bytes: u64,
    meta_db_bytes: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct EventBus {
    published: u64,
    delivered: u64,
    dropped: u64,
    subscribers: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ServerStats {
    tenants: u64,
    accounts: u64,
    trees: u64,
    branches: u64,
    snapshots: u64,
    staged: u64,
    objects: u64,
    storage_bytes: u64,
}

const ACCOUNTS: &str = "/api/v1/admin/accounts";

fn account_path(username: &str) -> String {
    format!("{ACCOUNTS}/{}", seg(username))
}

pub fn run(ctx: &Context, args: AdminArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let out = &ctx.out;
    match args.command {
        AdminCommand::Accounts => {
            let value = api.get(ACCOUNTS)?;
            render(out, &value, |t, accounts: Vec<Account>| {
                account_table(t, &accounts);
            })
        }
        AdminCommand::AccountCreate {
            username,
            email,
            admin,
            password_stdin,
        } => {
            let password = new_password(password_stdin)?;
            let body = json!({
                "username": username,
                "email": email,
                "password": password,
                "is_admin": admin,
            });
            let value = api.post(ACCOUNTS, &body)?;
            render(out, &value, |t, account: Account| {
                let role = if account.is_admin {
                    " (server admin)"
                } else {
                    ""
                };
                t.line(format_args!(
                    "Created account {}{role}",
                    account.username.green().bold()
                ));
            })
        }
        AdminCommand::AccountSet {
            username,
            admin,
            disabled,
        } => {
            let mut body = Map::new();
            if let Some(admin) = admin {
                body.insert("is_admin".into(), admin.into());
            }
            if let Some(disabled) = disabled {
                body.insert("disabled".into(), disabled.into());
            }
            if body.is_empty() {
                return Err(CliError::usage("nothing to change")
                    .with_hint("pass --admin true|false and/or --disabled true|false"));
            }
            let value = api.patch(&account_path(&username), &body)?;
            render(out, &value, |t, account: Account| {
                t.line(format_args!(
                    "Updated {}: admin {}, disabled {}",
                    account.username.bold(),
                    yes_no(account.is_admin),
                    yes_no(account.disabled)
                ));
            })
        }
        AdminCommand::Password {
            username,
            password_stdin,
        } => {
            let password = new_password(password_stdin)?;
            api.post(
                &format!("{}/password", account_path(&username)),
                &json!({ "password": password }),
            )?;
            out.emit(
                &json!({ "username": username, "password_changed": true }),
                |t| {
                    t.line(format_args!(
                        "Changed the password of {}; its tokens were revoked",
                        username.bold()
                    ));
                },
            )
        }
        AdminCommand::ResetCode {
            username,
            ttl_hours,
        } => {
            let mut body = Map::new();
            if let Some(ttl) = ttl_hours {
                body.insert("ttl_hours".into(), ttl.into());
            }
            let value = api.post(&format!("{}/reset-code", account_path(&username)), &body)?;
            render(out, &value, |t, reset: ResetCode| {
                t.line(format_args!(
                    "Password reset code for {}: {}",
                    username.bold(),
                    reset.code.yellow().bold()
                ));
                t.line(format_args!("expires {}", when(&reset.expires_at)));
                t.line(format!("redeem it with `wt password-reset {}`", reset.code).dimmed());
            })
        }
        AdminCommand::System => {
            let value = api.get("/api/v1/admin/system")?;
            render(out, &value, |t, info: SystemInfo| system(t, &info))
        }
        AdminCommand::Stats => {
            let value = api.get("/api/v1/admin/stats")?;
            render(out, &value, |t, s: ServerStats| {
                fields(
                    t,
                    &[
                        ("tenants", s.tenants.to_string()),
                        ("accounts", s.accounts.to_string()),
                        ("trees", s.trees.to_string()),
                        ("branches", s.branches.to_string()),
                        ("snapshots", s.snapshots.to_string()),
                        ("staged", s.staged.to_string()),
                        ("objects", s.objects.to_string()),
                        ("storage", bytes(s.storage_bytes)),
                    ],
                );
            })
        }
    }
}

/// USER EMAIL ADMIN DISABLED CREATED.
pub fn account_table(t: &mut Text, accounts: &[Account]) {
    let mut table = Table::new(&["USER", "EMAIL", "ADMIN", "DISABLED", "CREATED"]);
    for a in accounts {
        let admin = Cell::from(yes_no(a.is_admin));
        let disabled = Cell::from(yes_no(a.disabled));
        table.row([
            Cell::from(a.username.as_str()).bold(),
            a.email.as_str().into(),
            if a.is_admin { admin.yellow() } else { admin },
            if a.disabled { disabled.red() } else { disabled },
            when(&a.created_at).into(),
        ]);
    }
    table.render(t, "No accounts.");
}

fn system(t: &mut Text, info: &SystemInfo) {
    let started = timestamp(&info.started_at).map_or_else(|| "-".into(), date);
    fields(
        t,
        &[
            ("version", info.version.clone()),
            ("go", info.go_version.clone()),
            ("platform", format!("{}/{}", info.os, info.arch)),
            ("started", started),
            ("uptime", uptime(info.uptime_seconds)),
            ("registration", yes_no(info.allow_registration).into()),
            ("data dir", info.data_dir.clone()),
            ("cpus", info.num_cpu.to_string()),
            ("goroutines", info.num_goroutine.to_string()),
            (
                "memory",
                format!(
                    "{} allocated, {} heap in use, {} from the OS, {} GC runs",
                    bytes(info.memory.alloc_bytes),
                    bytes(info.memory.heap_inuse_bytes),
                    bytes(info.memory.sys_bytes),
                    info.memory.gc_runs
                ),
            ),
            (
                "storage",
                format!(
                    "{} objects ({}), metadata {}",
                    info.storage.objects,
                    bytes(info.storage.object_bytes),
                    bytes(info.storage.meta_db_bytes)
                ),
            ),
            (
                "event bus",
                format!(
                    "{} published, {} delivered, {} dropped, {} subscribers",
                    info.event_bus.published,
                    info.event_bus.delivered,
                    info.event_bus.dropped,
                    info.event_bus.subscribers
                ),
            ),
        ],
    );
}

/// "3d 4h 5m", "12m 30s".
fn uptime(secs: u64) -> String {
    let (d, h, m, s) = (secs / 86_400, secs / 3600 % 24, secs / 60 % 60, secs % 60);
    match (d, h) {
        (0, 0) => format!("{m}m {s}s"),
        (0, _) => format!("{h}h {m}m"),
        _ => format!("{d}d {h}h {m}m"),
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn admin(args: &[&str]) -> AdminCommand {
        let mut full = vec!["admin"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Admin(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_every_subcommand() {
        assert!(matches!(admin(&["accounts"]), AdminCommand::Accounts));
        assert!(matches!(admin(&["system"]), AdminCommand::System));
        assert!(matches!(admin(&["stats"]), AdminCommand::Stats));
        let AdminCommand::AccountCreate {
            username,
            email,
            admin: is_admin,
            password_stdin,
        } = admin(&[
            "account-create",
            "bob",
            "--email",
            "bob@example.com",
            "--admin",
            "--password-stdin",
        ])
        else {
            panic!()
        };
        assert_eq!(
            (username.as_str(), email.as_str()),
            ("bob", "bob@example.com")
        );
        assert!(is_admin && password_stdin);
        let AdminCommand::AccountSet {
            username,
            admin: is_admin,
            disabled,
        } = admin(&[
            "account-set",
            "bob",
            "--admin",
            "false",
            "--disabled",
            "true",
        ])
        else {
            panic!()
        };
        assert_eq!(
            (username.as_str(), is_admin, disabled),
            ("bob", Some(false), Some(true))
        );
        assert!(matches!(
            admin(&["password", "bob", "--password-stdin"]),
            AdminCommand::Password {
                password_stdin: true,
                ..
            }
        ));
        assert!(matches!(
            admin(&["reset-code", "bob", "--ttl-hours", "2"]),
            AdminCommand::ResetCode {
                ttl_hours: Some(2),
                ..
            }
        ));
        assert_eq!(
            parse_err(&["admin", "account-create", "bob"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["admin", "account-set", "bob", "--admin", "maybe"]),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn uptimes() {
        assert_eq!(uptime(75), "1m 15s");
        assert_eq!(uptime(3 * 3600 + 120), "3h 2m");
        assert_eq!(uptime(2 * 86_400 + 3600), "2d 1h 0m");
    }

    #[test]
    fn account_tables() {
        colored::control::set_override(false);
        let mut t = Text::default();
        account_table(
            &mut t,
            &[Account {
                username: "admin".into(),
                email: "a@x".into(),
                is_admin: true,
                ..Account::default()
            }],
        );
        assert_eq!(
            t.as_str(),
            "USER   EMAIL  ADMIN  DISABLED  CREATED\nadmin  a@x    yes    no        -\n"
        );
    }
}
