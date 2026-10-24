//! `wt login`, `wt logout`, `wt whoami`, `wt register`

use std::io::{BufRead, Write};

use clap::Args;
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::remote::{self, LoginInfo};
use worktree_sdk::{Repository, SdkError};

use crate::commands::remote::{normalize_server, remote_name, remote_url};
use crate::context::{block_on, Context};
use crate::error::{CliError, Result};

/// Environment variable consulted for the password before prompting.
const PASSWORD_ENV: &str = "WT_PASSWORD";

#[derive(Debug, Args)]
pub struct LoginArgs {
    /// Server URL (default: the URL of the worktree's default remote)
    pub server: Option<String>,

    /// Account name (prompted for if omitted)
    #[arg(short, long, value_name = "USER")]
    pub username: Option<String>,

    /// Read the password from the first line of stdin
    #[arg(long)]
    pub password_stdin: bool,
}

#[derive(Debug, Args)]
pub struct LogoutArgs {
    /// Server URL (default: the URL of the worktree's default remote)
    pub server: Option<String>,
}

#[derive(Debug, Args)]
pub struct WhoamiArgs {
    /// Server URL (default: the URL of the worktree's default remote)
    pub server: Option<String>,
}

#[derive(Debug, Args)]
pub struct RegisterArgs {
    /// Server URL (default: the URL of the worktree's default remote)
    pub server: Option<String>,

    /// Account name
    #[arg(short, long, value_name = "USER")]
    pub username: String,

    /// Email address
    #[arg(short, long, value_name = "EMAIL")]
    pub email: String,

    /// Read the password from the first line of stdin
    #[arg(long)]
    pub password_stdin: bool,
}

/// Login result without the token (secrets never go to stdout).
#[derive(Serialize)]
struct Session<'a> {
    server: &'a str,
    username: &'a str,
    account_id: &'a str,
    tenant: &'a str,
}

impl<'a> From<&'a LoginInfo> for Session<'a> {
    fn from(info: &'a LoginInfo) -> Self {
        Self {
            server: &info.server,
            username: &info.username,
            account_id: &info.account_id,
            tenant: &info.tenant,
        }
    }
}

#[derive(Serialize)]
struct LoggedOut<'a> {
    server: &'a str,
    was_logged_in: bool,
}

#[derive(Serialize)]
struct Identity<'a> {
    server: &'a str,
    username: &'a str,
    tenant: Option<&'a str>,
    server_version: &'a str,
}

pub fn login(ctx: &Context, args: LoginArgs) -> Result<()> {
    let server = resolve_server(ctx, args.server)?;
    let username = match args.username {
        Some(user) => user,
        None if args.password_stdin => {
            return Err(CliError::usage(
                "--username is required with --password-stdin",
            ));
        }
        None => prompt_line("Username: ")?,
    };
    let password = read_password(args.password_stdin, false)?;
    let info = block_on(remote::login(&server, &username, &password))??;
    ctx.out.emit(&Session::from(&info), |t| {
        t.line(format_args!(
            "Logged in to {} as {}",
            info.server.bold(),
            info.username.green().bold()
        ));
    })
}

pub fn register(ctx: &Context, args: RegisterArgs) -> Result<()> {
    let server = resolve_server(ctx, args.server)?;
    let password = read_password(args.password_stdin, true)?;
    let info = block_on(remote::register(
        &server,
        &args.username,
        &args.email,
        &password,
    ))??;
    ctx.out.emit(&Session::from(&info), |t| {
        t.line(format_args!(
            "Registered {} on {}; you are now logged in",
            info.username.green().bold(),
            info.server.bold()
        ));
    })
}

pub fn logout(ctx: &Context, args: LogoutArgs) -> Result<()> {
    let server = resolve_server(ctx, args.server)?;
    let was_logged_in = remote::load_credential(&server)?.is_some();
    if was_logged_in {
        remote::remove_credential(&server)?;
    }
    ctx.out.emit(
        &LoggedOut {
            server: &server,
            was_logged_in,
        },
        |t| {
            if was_logged_in {
                t.line(format_args!("Logged out of {}", server.bold()));
            } else {
                t.line(format_args!("Not logged in to {}", server.bold()));
            }
        },
    )
}

pub fn whoami(ctx: &Context, args: WhoamiArgs) -> Result<()> {
    let server = resolve_server(ctx, args.server)?;
    if remote::load_credential(&server)?.is_none() {
        return Err(SdkError::NotLoggedIn(server).into());
    }
    let ping = block_on(remote::ping(&server))??;
    let Some(username) = ping.username.as_deref() else {
        return Err(
            CliError::failure(format!("the saved credential for {server} was rejected"))
                .with_hint(format!("run `wt login {server}`")),
        );
    };
    let identity = Identity {
        server: &server,
        username,
        tenant: ping.tenant.as_deref(),
        server_version: &ping.server_version,
    };
    ctx.out.emit(&identity, |t| {
        let tenant = identity
            .tenant
            .map(|t| format!(" (tenant {})", t.cyan()))
            .unwrap_or_default();
        t.line(format_args!(
            "{} on {}{tenant}",
            username.green().bold(),
            server.bold()
        ));
        t.line(format_args!("server version {}", identity.server_version));
    })
}

/// The explicit server, else the URL of the default remote.
fn resolve_server(ctx: &Context, explicit: Option<String>) -> Result<String> {
    if let Some(server) = explicit {
        return Ok(normalize_server(&server));
    }
    let hint = "pass the server URL, e.g. `wt login http://localhost:7420`";
    let repo = match Repository::open(ctx.cwd()) {
        Ok(repo) => repo,
        Err(SdkError::NotAWorktree) => {
            return Err(
                CliError::usage("no server given and not inside a worktree").with_hint(hint)
            );
        }
        Err(e) => return Err(e.into()),
    };
    let remote = remote_name(&repo, None);
    remote_url(&repo, &remote).map_err(|_| {
        CliError::usage(format!(
            "no server given and remote '{remote}' is not configured"
        ))
        .with_hint(hint)
    })
}

/// `--password-stdin` → first stdin line; else `$WT_PASSWORD`; else an
/// interactive prompt without echo (confirmed twice when `confirm`).
fn read_password(from_stdin: bool, confirm: bool) -> Result<String> {
    let password = if from_stdin {
        first_line(std::io::stdin().lock())?
    } else if let Some(password) = std::env::var(PASSWORD_ENV).ok().filter(|p| !p.is_empty()) {
        password
    } else {
        let password = rpassword::prompt_password("Password: ")?;
        if confirm && rpassword::prompt_password("Confirm password: ")? != password {
            return Err(CliError::failure("passwords do not match"));
        }
        password
    };
    if password.is_empty() {
        return Err(CliError::failure("the password must not be empty"));
    }
    Ok(password)
}

fn prompt_line(prompt: &str) -> Result<String> {
    let mut stderr = std::io::stderr();
    write!(stderr, "{prompt}")?;
    stderr.flush()?;
    let line = first_line(std::io::stdin().lock())?;
    if line.is_empty() {
        return Err(CliError::failure("no input given"));
    }
    Ok(line)
}

/// First line of `reader` without its line terminator.
fn first_line(mut reader: impl BufRead) -> Result<String> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let trimmed = line.trim_end_matches(['\r', '\n']);
    Ok(trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_line_strips_terminators() {
        assert_eq!(
            first_line(&b"s3cret\r\nmore\n"[..]).expect("read"),
            "s3cret"
        );
        assert_eq!(first_line(&b"pw\n"[..]).expect("read"), "pw");
        assert_eq!(first_line(&b"no newline"[..]).expect("read"), "no newline");
        assert_eq!(first_line(&b""[..]).expect("read"), "");
        assert_eq!(
            first_line(&b"  spaces kept  \n"[..]).expect("read"),
            "  spaces kept  "
        );
    }

    #[test]
    fn session_omits_the_token() {
        let info = LoginInfo {
            server: "http://localhost:7420".into(),
            username: "ada".into(),
            account_id: "acc-1".into(),
            tenant: "ada".into(),
            token: "super-secret-token".into(),
        };
        let json = serde_json::to_string(&Session::from(&info)).expect("json");
        assert!(!json.contains("super-secret-token"), "{json}");
        assert!(json.contains("\"username\":\"ada\""), "{json}");
    }
}
