//! `wt login`, `wt logout`, `wt whoami`, `wt register`, `wt password-reset`

use std::io::{BufRead, Write};

use clap::Args;
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::remote::{self, LoginInfo, LoginOutcome, RestClient};
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

    /// TOTP or recovery code for accounts with two-factor authentication
    /// (prompted for when needed)
    #[arg(long, value_name = "CODE", env = "WT_MFA_CODE", hide_env_values = true)]
    pub mfa_code: Option<String>,
}

#[derive(Debug, Args)]
pub struct PasswordResetArgs {
    /// One-time reset code (`wtr_...`) issued by a server administrator
    pub code: String,

    /// Server URL (default: --server, else the worktree's origin, else the only saved login)
    pub server: Option<String>,

    /// Read the new password from the first line of stdin
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
    let password = read_password(args.password_stdin, "Password: ", false)?;
    let info = match block_on(remote::login_start(&server, &username, &password))?? {
        LoginOutcome::LoggedIn(info) => info,
        LoginOutcome::MfaRequired(challenge) => {
            let code = match args.mfa_code.filter(|c| !c.trim().is_empty()) {
                Some(code) => code,
                None => prompt_line("Two-factor code (TOTP or recovery code): ")?,
            };
            block_on(remote::login_mfa(&challenge, &code))??
        }
    };
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
    let password = read_password(args.password_stdin, "Password: ", true)?;
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

#[derive(Serialize)]
struct PasswordReset<'a> {
    server: &'a str,
    reset: bool,
}

/// `wt password-reset CODE [SERVER]`: set a new password with an
/// admin-issued one-time code (public endpoint; no login needed).
pub fn password_reset(ctx: &Context, args: PasswordResetArgs) -> Result<()> {
    let server = match args.server {
        Some(server) => normalize_server(&server),
        None => ctx
            .rest()
            .map_err(|e| e.with_hint("pass the server URL: `wt password-reset CODE <server>`"))?
            .server()
            .to_string(),
    };
    let password = read_password(args.password_stdin, "New password: ", true)?;
    let body = serde_json::json!({ "code": args.code.trim(), "new_password": password });
    let client = RestClient::new(&server, None)?;
    block_on(client.post::<_, serde_json::Value>("/api/v1/auth/password-reset", &body))??;
    ctx.out.emit(
        &PasswordReset {
            server: &server,
            reset: true,
        },
        |t| {
            t.line(format_args!(
                "Password changed on {}; log in with `wt login {server}`",
                server.bold()
            ));
        },
    )
}

/// `--password-stdin` → first stdin line; else `$WT_PASSWORD`; else an
/// interactive `prompt` without echo (confirmed twice when `confirm`).
pub(crate) fn read_password(from_stdin: bool, prompt: &str, confirm: bool) -> Result<String> {
    let password = if from_stdin {
        first_line(std::io::stdin().lock())?
    } else if let Some(password) = std::env::var(PASSWORD_ENV).ok().filter(|p| !p.is_empty()) {
        password
    } else {
        let password = rpassword::prompt_password(prompt)?;
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

/// Print `prompt` on stderr and read one non-empty line from stdin.
pub(crate) fn prompt_line(prompt: &str) -> Result<String> {
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
pub(crate) fn first_line(mut reader: impl BufRead) -> Result<String> {
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
    fn parses_mfa_code_and_password_reset() {
        use crate::cli::Command;
        use crate::commands::iam_common::parse;

        let Command::Login(a) = parse(&["login", "http://s", "-u", "ada", "--mfa-code", "123456"])
        else {
            panic!()
        };
        assert_eq!(a.mfa_code.as_deref(), Some("123456"));
        let Command::PasswordReset(a) = parse(&["password-reset", "wtr_abc", "--password-stdin"])
        else {
            panic!()
        };
        assert_eq!(a.code, "wtr_abc");
        assert!(a.password_stdin && a.server.is_none());
        let Command::PasswordReset(a) = parse(&["password-reset", "wtr_abc", "http://s"]) else {
            panic!()
        };
        assert_eq!(a.server.as_deref(), Some("http://s"));
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
