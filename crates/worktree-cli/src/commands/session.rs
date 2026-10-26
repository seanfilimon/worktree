//! `wt session` — Your sign-in sessions (§12.2, §12.3 sessions).

use clap::{Args, Subcommand};
use serde::Deserialize;
use serde_json::{json, Value};
use worktree_sdk::remote::rest::seg;

use crate::commands::iam_common::{call, confirm, show, when, yes, Table, YesArgs};
use crate::context::Context;
use crate::error::Result;
use crate::output::Text;

const SESSIONS_PATH: &str = "/api/v1/auth/sessions";

#[derive(Debug, Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// List your sessions (the one this CLI uses is marked current)
    List,
    /// Sign out one session
    Revoke {
        /// Session id
        id: String,
        #[command(flatten)]
        yes: YesArgs,
    },
    /// Sign out every session except the current one
    RevokeOthers {
        #[command(flatten)]
        yes: YesArgs,
    },
}

#[derive(Debug, Deserialize)]
struct Session {
    id: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    last_used_at: Option<String>,
    #[serde(default)]
    last_ip: String,
    #[serde(default)]
    user_agent: String,
    #[serde(default)]
    mfa: bool,
    #[serde(default)]
    current: bool,
    /// "session" for sign-ins, "device: <client>" for device logins
    /// (device-auth, §16.6).
    #[serde(default)]
    name: String,
}

/// How a session was created: `device (<client>)` for `wt login --device`
/// approvals, else `sign-in`.
fn session_via(name: &str) -> String {
    match name.strip_prefix("device: ") {
        Some(client) if !client.trim().is_empty() => format!("device ({})", client.trim()),
        Some(_) => "device".to_string(),
        None => "sign-in".to_string(),
    }
}

#[derive(Debug, Deserialize)]
struct Revoked {
    revoked: u64,
}

pub fn run(ctx: &Context, args: SessionArgs) -> Result<()> {
    let client = ctx.rest()?;
    match args.command {
        SessionCommand::List => {
            let sessions: Value = call(client.get(SESSIONS_PATH))?;
            show(&ctx.out, &sessions, |t, s: &Vec<Session>| {
                session_list(t, s)
            })
        }
        SessionCommand::Revoke { id, yes } => {
            confirm(yes.yes, &format!("revoke session {id}"))?;
            call(client.delete(&format!("{SESSIONS_PATH}/{}", seg(&id))))?;
            ctx.out.emit(&json!({"id": id, "revoked": true}), |t| {
                t.line(format_args!("Revoked session {id}"));
            })
        }
        SessionCommand::RevokeOthers { yes } => {
            confirm(yes.yes, "sign out all your other sessions")?;
            let path = format!("{SESSIONS_PATH}/revoke-others");
            let result: Value = call(client.post(&path, &json!({})))?;
            show(&ctx.out, &result, |t, r: &Revoked| {
                t.line(format_args!("Revoked {} other session(s)", r.revoked));
            })
        }
    }
}

fn session_list(t: &mut Text, sessions: &[Session]) {
    let mut table = Table::new(&[
        "id",
        "current",
        "created",
        "last used",
        "last ip",
        "mfa",
        "user agent",
        "via",
    ]);
    for s in sessions {
        table.row([
            s.id.clone(),
            if s.current { "*" } else { "" }.to_string(),
            when(&s.created_at),
            s.last_used_at.as_deref().map(when).unwrap_or_default(),
            s.last_ip.clone(),
            yes(s.mfa).to_string(),
            s.user_agent.clone(),
            session_via(&s.name),
        ]);
    }
    table.render(t, "No sessions.");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::iam_common::parse;

    #[test]
    fn parses_subcommands() {
        let session = |args: &[&str]| {
            let mut argv = vec!["session"];
            argv.extend_from_slice(args);
            match parse(&argv) {
                Command::Session(a) => a.command,
                other => panic!("{other:?}"),
            }
        };
        assert!(matches!(session(&["list"]), SessionCommand::List));
        assert!(matches!(
            session(&["revoke", "s1", "--yes"]),
            SessionCommand::Revoke { id, yes: YesArgs { yes: true } } if id == "s1"
        ));
        assert!(matches!(
            session(&["revoke-others"]),
            SessionCommand::RevokeOthers {
                yes: YesArgs { yes: false }
            }
        ));
    }

    #[test]
    fn renders_sessions_marking_the_current_one() {
        let sessions: Vec<Session> = serde_json::from_value(json!([
            {"id": "s1", "account_id": "a", "name": "session", "kind": "session",
             "created_at": "2026-01-01T00:00:00Z", "last_ip": "10.0.0.1",
             "user_agent": "worktree-sdk/0.1.0", "mfa": true, "current": true},
            {"id": "s2", "created_at": "", "current": false}
        ]))
        .unwrap();
        let mut t = Text::default();
        session_list(&mut t, &sessions);
        let lines: Vec<&str> = t.as_str().lines().collect();
        assert!(lines[0].starts_with("ID  CURRENT  CREATED"), "{lines:?}");
        assert!(lines[1].starts_with("s1  *"), "{lines:?}");
        assert!(
            lines[1].contains("10.0.0.1  yes  worktree-sdk/0.1.0"),
            "{lines:?}"
        );
        assert!(lines[2].starts_with("s2  -"), "{lines:?}");
        assert!(lines[1].ends_with("sign-in"), "{lines:?}");
    }

    #[test]
    fn device_sessions_show_their_client() {
        let sessions: Vec<Session> = serde_json::from_value(json!([
            {"id": "d1", "name": "device: wt CLI on laptop", "kind": "session",
             "created_at": "2026-01-01T00:00:00Z", "current": false}
        ]))
        .unwrap();
        let mut t = Text::default();
        session_list(&mut t, &sessions);
        assert!(
            t.as_str().contains("device (wt CLI on laptop)"),
            "{}",
            t.as_str()
        );
        assert_eq!(session_via("device: "), "device");
        assert_eq!(session_via("session"), "sign-in");
    }
}
