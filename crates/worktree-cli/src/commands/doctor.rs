//! `wt doctor` — diagnose the local setup (ENGINEERING_PLAN §14.8).
//!
//! Checks, in order: worktree discovery; every config layer parses (user,
//! worktree, each tree); user name/email; aliases; per server of the
//! worktree's remotes (or of the saved logins outside a worktree): HTTP
//! reachability, gRPC `Ping`, version skew, clock skew against the `Date`
//! header, credential validity (`/auth/me`) and token expiry; the
//! background daemon (running, version, watching this worktree); the
//! signing key (present, registered, not revoked); hook files.
//!
//! Every network call has a timeout (`--timeout`, default 5 s) and a server
//! that cannot be reached skips its dependent checks, so the command stays
//! fast offline; `--offline` skips the network entirely. Each line is
//! `✓` (ok), `!` (warning), `✗` (error) or `-` (skipped), with a fix where
//! one is known. Exit code 1 when any check is an error.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use clap::{Args, CommandFactory};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use worktree_sdk::config::{
    effective_user, user_config_path, validate_config_file, ConfigLevel, UserSection,
    WorktreeConfig,
};
use worktree_sdk::ipc::{self, DaemonStatus, IpcRequest, IpcResponse};
use worktree_sdk::remote::{self, RestClient};
use worktree_sdk::repo::{TREE_MARKER_DIR, WT_DIR};
use worktree_sdk::signing::KeyStore;
use worktree_sdk::{Repository, SdkError, TreeInfo};

use crate::cli::Cli;
use crate::commands::remote::normalize_server;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

/// Version of this `wt` binary.
const CLI_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Clock skew that is reported as a warning / an error.
const CLOCK_WARN: Duration = Duration::from_secs(30);
const CLOCK_ERROR: Duration = Duration::from_secs(300);
/// Tokens expiring sooner than this are reported.
const EXPIRY_WARN_DAYS: i64 = 7;

#[derive(Debug, Args)]
pub struct DoctorArgs {
    /// Skip every check that needs the network (servers, credentials, clock)
    #[arg(long)]
    pub offline: bool,

    /// Timeout of each network check, in seconds
    #[arg(long, value_name = "SECS", default_value_t = 5)]
    pub timeout: u64,
}

/// Outcome of one check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Ok,
    Warn,
    Error,
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub name: String,
    pub status: Status,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

impl Check {
    fn new(name: impl Into<String>, status: Status, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            message: message.into(),
            fix: None,
        }
    }

    fn ok(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(name, Status::Ok, message)
    }

    fn warn(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(name, Status::Warn, message)
    }

    fn error(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(name, Status::Error, message)
    }

    fn skip(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(name, Status::Skip, message)
    }

    fn fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

#[derive(Serialize)]
struct Report<'a> {
    ok: bool,
    errors: usize,
    warnings: usize,
    checks: &'a [Check],
}

pub fn run(ctx: &Context, args: DoctorArgs) -> Result<()> {
    let timeout = Duration::from_secs(args.timeout.max(1));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| CliError::failure(format!("cannot start the async runtime: {e}")))?;

    let mut checks = Vec::new();
    let (repo, tree) = worktree_check(ctx, &mut checks);
    let repo = repo.as_ref();
    let tree = tree.as_ref();
    let config = repo.map(|r| {
        tree.and_then(|t| r.effective_config(Some(&t.name)).ok())
            .unwrap_or_else(|| r.config().clone())
    });
    checks.extend(config_checks(repo));
    checks.push(user_check(config.as_ref()));
    checks.extend(alias_check(&worktree_sdk::config::load_aliases(ctx.cwd())));

    let servers = servers(repo);
    if args.offline {
        checks.push(Check::skip("network", "skipped (--offline)"));
    } else if servers.is_empty() {
        checks.push(
            Check::skip("server", "no remote configured and no saved login")
                .fix("add a remote with `wt remote add origin <url> --tenant <tenant>`"),
        );
    } else {
        for (url, label) in &servers {
            // device-auth (§16.6): renew an access token about to expire.
            let token = runtime
                .block_on(remote::fresh_credential(url))
                .ok()
                .flatten()
                .map(|c| c.token);
            checks.extend(runtime.block_on(server_checks(label, url, token, timeout, live_ping)));
        }
    }

    let auto = config.as_ref().is_some_and(|c| c.auto_snapshot.enabled);
    let status = runtime.block_on(daemon_status(timeout));
    checks.push(daemon_check(&status, repo.map(Repository::root), auto));

    if let (Some(repo), Some(config)) = (repo, config.as_ref()) {
        let key = config.signing.key.trim().to_string();
        checks.push(runtime.block_on(signing_check(
            &KeyStore::default_location(),
            &key,
            config.signing.auto,
            !args.offline,
            timeout,
        )));
        checks.push(hooks_check(repo, config.hooks.enabled));
    }

    let errors = checks.iter().filter(|c| c.status == Status::Error).count();
    let warnings = checks.iter().filter(|c| c.status == Status::Warn).count();
    let report = Report {
        ok: errors == 0,
        errors,
        warnings,
        checks: &checks,
    };
    ctx.out
        .emit(&report, |t| render(t, &checks, errors, warnings))?;
    if errors > 0 {
        return Err(CliError::failure(format!(
            "{errors} check{} failed",
            if errors == 1 { "" } else { "s" }
        )));
    }
    Ok(())
}

fn render(t: &mut Text, checks: &[Check], errors: usize, warnings: usize) {
    let width = checks.iter().map(|c| c.name.len()).max().unwrap_or(0);
    for c in checks {
        let mark = match c.status {
            Status::Ok => "✓".green(),
            Status::Warn => "!".yellow().bold(),
            Status::Error => "✗".red().bold(),
            Status::Skip => "-".dimmed(),
        };
        let name = format!("{:<width$}", c.name);
        let message = if c.status == Status::Skip {
            c.message.dimmed().to_string()
        } else {
            c.message.clone()
        };
        t.line(format_args!("{mark} {}  {message}", name.bold()));
        if let Some(fix) = &c.fix {
            t.line(format_args!("  {:width$}  {} {fix}", "", "fix:".cyan()));
        }
    }
    t.blank();
    if errors == 0 && warnings == 0 {
        t.line("No problems found.".green());
    } else {
        let plural = |n: usize, word: &str| format!("{n} {word}{}", if n == 1 { "" } else { "s" });
        t.line(format_args!(
            "{}, {}",
            plural(errors, "error"),
            plural(warnings, "warning")
        ));
    }
}

// ---------------------------------------------------------------------------
// Local checks

fn worktree_check(
    ctx: &Context,
    checks: &mut Vec<Check>,
) -> (Option<Repository>, Option<TreeInfo>) {
    match Repository::open(ctx.cwd()) {
        Ok(repo) => match ctx.select_tree(&repo) {
            Ok(tree) => {
                checks.push(Check::ok(
                    "worktree",
                    format!(
                        "{} (tree {}, branch {})",
                        repo.root().display(),
                        tree.name,
                        tree.current_branch
                    ),
                ));
                (Some(repo), Some(tree))
            }
            Err(e) => {
                checks.push(
                    Check::error("worktree", format!("{}: {e}", repo.root().display()))
                        .fix("list trees with `wt tree list`"),
                );
                (Some(repo), None)
            }
        },
        Err(SdkError::NotAWorktree) => {
            checks.push(Check::skip(
                "worktree",
                "not inside a worktree (worktree checks skipped)",
            ));
            (None, None)
        }
        Err(e) => {
            checks.push(Check::error("worktree", e.to_string()).fix(format!(
                "repair {WT_DIR}/config.toml (see `wt config list` once it parses)"
            )));
            (None, None)
        }
    }
}

/// One check per config file that exists (the user file is always reported).
fn config_checks(repo: Option<&Repository>) -> Vec<Check> {
    let mut files = vec![(
        "config.user".to_string(),
        user_config_path(),
        ConfigLevel::User,
    )];
    if let Some(repo) = repo {
        files.push((
            "config.worktree".into(),
            repo.wt_dir().join("config.toml"),
            ConfigLevel::Worktree,
        ));
        for tree in repo.trees().unwrap_or_default() {
            if let Ok(Some(path)) = repo.tree_config_path(&tree.name) {
                files.push((
                    format!("config.tree.{}", tree.name),
                    path,
                    ConfigLevel::Tree,
                ));
            }
        }
    }
    files
        .into_iter()
        .filter_map(|(name, path, level)| {
            let check = match validate_config_file(&path, level) {
                Ok(true) => Check::ok(name, path.display().to_string()),
                Ok(false) if level == ConfigLevel::User => {
                    Check::ok(name, format!("{} not present (optional)", path.display()))
                }
                Ok(false) => return None,
                Err(e) => Check::error(name, e.to_string()).fix(format!("edit {}", path.display())),
            };
            Some(check)
        })
        .collect()
}

fn user_check(config: Option<&WorktreeConfig>) -> Check {
    let user = match config {
        Some(c) => effective_user(&c.user),
        None => effective_user(&UserSection::default()),
    };
    let missing: Vec<&str> = [("user.name", &user.name), ("user.email", &user.email)]
        .into_iter()
        .filter(|(_, v)| v.trim().is_empty())
        .map(|(k, _)| k)
        .collect();
    match missing.as_slice() {
        [] => Check::ok("user", format!("{} <{}>", user.name, user.email)),
        keys => {
            let fix = keys
                .iter()
                .map(|k| {
                    let example = if *k == "user.name" {
                        "Your Name"
                    } else {
                        "you@example.com"
                    };
                    format!("wt config set --global {k} \"{example}\"")
                })
                .collect::<Vec<_>>()
                .join(" && ");
            Check::warn(
                "user",
                format!("{} not set (snapshot authorship)", keys.join(" and ")),
            )
            .fix(fix)
        }
    }
}

/// `None` when no aliases are configured.
fn alias_check(aliases: &BTreeMap<String, String>) -> Option<Check> {
    if aliases.is_empty() {
        return None;
    }
    let command = Cli::command();
    for name in aliases.keys() {
        if crate::alias::is_builtin(&command, name) {
            return Some(
                Check::warn(
                    "aliases",
                    format!("alias '{name}' has the name of a built-in command and is ignored"),
                )
                .fix(format!("wt config unset alias.{name} (or --global)")),
            );
        }
        let argv = vec!["wt".into(), name.into()];
        let table = aliases.clone();
        if let Err(e) = crate::alias::expand_with(
            argv,
            |w| crate::alias::is_builtin(&command, w),
            move |_| table,
        ) {
            return Some(
                Check::error("aliases", e.to_string())
                    .fix(format!("wt config set alias.{name} \"<command> [args]\"")),
            );
        }
    }
    let names: Vec<&str> = aliases.keys().map(String::as_str).collect();
    Some(Check::ok(
        "aliases",
        format!(
            "{} ({})",
            plural(names.len(), "alias", "aliases"),
            names.join(", ")
        ),
    ))
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

// ---------------------------------------------------------------------------
// Servers

/// (normalized URL, label) of every server to check: the worktree's
/// remotes (remotes sharing a URL are checked once), else every server
/// with a saved login.
fn servers(repo: Option<&Repository>) -> Vec<(String, String)> {
    let mut by_url: BTreeMap<String, Vec<String>> = BTreeMap::new();
    match repo {
        Some(repo) => {
            for (name, r) in &repo.config().remote {
                by_url
                    .entry(normalize_server(&r.url))
                    .or_default()
                    .push(name.clone());
            }
        }
        None => {
            for url in remote::CredentialStore::default_location()
                .servers()
                .unwrap_or_default()
            {
                by_url.entry(url.clone()).or_default().push(url);
            }
        }
    }
    by_url
        .into_iter()
        .map(|(url, names)| (url, names.join(",")))
        .collect()
}

/// What `GET /healthz` told us.
#[derive(Debug, Clone)]
pub struct Probe {
    pub status: u16,
    /// The server's `Date` header.
    pub date: Option<DateTime<Utc>>,
    /// Local time halfway through the request.
    pub midpoint: DateTime<Utc>,
}

/// `GET <server>/healthz` (any HTTP answer means "reachable").
pub async fn probe(server: &str, timeout: Duration) -> std::result::Result<Probe, String> {
    let client = reqwest::Client::builder()
        .user_agent(concat!("wt/", env!("CARGO_PKG_VERSION")))
        .timeout(timeout)
        .build()
        .map_err(|e| e.to_string())?;
    let started = Utc::now();
    let clock = Instant::now();
    let resp = client
        .get(format!("{server}/healthz"))
        .send()
        .await
        .map_err(|e| error_chain(&e))?;
    let elapsed = chrono::Duration::from_std(clock.elapsed() / 2).unwrap_or_default();
    let date = resp
        .headers()
        .get(reqwest::header::DATE)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| DateTime::parse_from_rfc2822(v).ok())
        .map(|d| d.with_timezone(&Utc));
    Ok(Probe {
        status: resp.status().as_u16(),
        date,
        midpoint: started + elapsed,
    })
}

/// An error with its sources ("error sending request: connection refused").
fn error_chain(e: &reqwest::Error) -> String {
    let mut text = e.to_string();
    let mut source = std::error::Error::source(e);
    while let Some(s) = source {
        let part = s.to_string();
        if !text.contains(&part) {
            text.push_str(": ");
            text.push_str(&part);
        }
        source = s.source();
    }
    text
}

/// The real gRPC Ping (with the saved credential).
async fn live_ping(server: String) -> worktree_sdk::Result<remote::PingInfo> {
    remote::ping(&server).await
}

/// Every check of one server. `token` is the saved credential (if any);
/// `ping` performs the gRPC Ping.
pub async fn server_checks<P, F>(
    label: &str,
    url: &str,
    token: Option<String>,
    timeout: Duration,
    ping: P,
) -> Vec<Check>
where
    P: FnOnce(String) -> F,
    F: Future<Output = worktree_sdk::Result<remote::PingInfo>>,
{
    let name = |what: &str| format!("{label}: {what}");
    let mut out = Vec::new();
    let probe = match probe(url, timeout).await {
        Ok(p) => p,
        Err(e) => {
            out.push(
                Check::error(name("server"), format!("cannot reach {url}: {e}")).fix(
                    "check the URL (`wt remote list`) and that the server is running; \
                     `wt doctor --offline` skips network checks",
                ),
            );
            return out;
        }
    };
    out.push(if (200..300).contains(&probe.status) {
        Check::ok(name("server"), format!("{url} is reachable"))
    } else {
        Check::warn(
            name("server"),
            format!(
                "{url}/healthz answered {} (server unhealthy?)",
                probe.status
            ),
        )
        .fix("see the server log or `wt admin system`")
    });

    match tokio::time::timeout(timeout, ping(url.to_string())).await {
        Ok(Ok(info)) => {
            out.push(Check::ok(
                name("sync"),
                format!("gRPC Ping ok (server {})", info.server_version),
            ));
            out.push(version_check(&name("version"), CLI_VERSION, &info.server_version));
        }
        Ok(Err(e)) => out.push(
            Check::error(name("sync"), format!("gRPC Ping failed: {e}"))
                .fix("the REST API answers but the sync service does not; check proxies for HTTP/2 (h2c) support"),
        ),
        Err(_) => out.push(Check::error(
            name("sync"),
            format!("gRPC Ping timed out after {}s", timeout.as_secs()),
        )),
    }

    out.push(clock_check(&name("clock"), &probe));

    let Some(token) = token else {
        out.push(Check::warn(name("credential"), "not logged in").fix(format!("wt login {url}")));
        return out;
    };
    let client = match RestClient::new(url, Some(token)) {
        Ok(c) => c,
        Err(e) => {
            out.push(Check::error(name("credential"), e.to_string()));
            return out;
        }
    };
    let (credential, valid) = credential_check(&name("credential"), &client, url, timeout).await;
    out.push(credential);
    if valid {
        // device-auth (§16.6): logins with a refresh token renew themselves.
        let renews = remote::load_credential(url)
            .ok()
            .flatten()
            .is_some_and(|c| c.refresh_token.is_some());
        if renews {
            out.push(Check::ok(
                name("token"),
                "the access token renews automatically (refresh token)",
            ));
        } else {
            out.push(expiry_check(&name("token"), &client, timeout, Utc::now()).await);
        }
    }
    out
}

/// Version skew: a different major version is a warning.
pub fn version_check(name: &str, cli: &str, server: &str) -> Check {
    let major = |v: &str| {
        v.trim_start_matches('v')
            .split(['.', '-', '+'])
            .next()
            .and_then(|m| m.parse::<u64>().ok())
    };
    match (major(cli), major(server)) {
        (Some(a), Some(b)) if a != b => Check::warn(
            name,
            format!("wt {cli} and server {server} differ in major version"),
        )
        .fix("upgrade wt (or the server) so both have the same major version"),
        (_, None) => Check::ok(
            name,
            format!("wt {cli}, server {server:?} (unversioned build)"),
        ),
        _ => Check::ok(name, format!("wt {cli}, server {server}")),
    }
}

/// Clock skew between this machine and the server's `Date` header.
pub fn clock_check(name: &str, probe: &Probe) -> Check {
    let Some(date) = probe.date else {
        return Check::skip(name, "the server sent no Date header");
    };
    let skew = date - probe.midpoint;
    let secs = skew.num_seconds();
    let abs = Duration::from_secs(secs.unsigned_abs());
    let direction = if secs > 0 { "behind" } else { "ahead of" };
    let text = format!("local clock is {}s {direction} the server", secs.abs());
    if abs >= CLOCK_ERROR {
        Check::error(name, text).fix(
            "synchronize the system clock (NTP); token expiry, TOTP codes and signatures depend on it",
        )
    } else if abs >= CLOCK_WARN {
        Check::warn(name, text).fix("synchronize the system clock (NTP)")
    } else {
        Check::ok(name, format!("in sync (within {}s)", CLOCK_WARN.as_secs()))
    }
}

#[derive(Deserialize)]
struct Me {
    account: MeAccount,
}

#[derive(Deserialize)]
struct MeAccount {
    username: String,
}

/// `GET /api/v1/auth/me`; also whether the credential is valid.
pub async fn credential_check(
    name: &str,
    client: &RestClient,
    url: &str,
    timeout: Duration,
) -> (Check, bool) {
    match tokio::time::timeout(timeout, client.get::<Me>("/api/v1/auth/me")).await {
        Ok(Ok(me)) => {
            let source = if std::env::var_os(remote::credentials::TOKEN_ENV).is_some() {
                " (WT_TOKEN)"
            } else {
                ""
            };
            (
                Check::ok(
                    name,
                    format!("logged in as {}{source}", me.account.username),
                ),
                true,
            )
        }
        Ok(Err(SdkError::NotLoggedIn(_))) => (
            Check::error(
                name,
                "the saved credential was rejected (expired or revoked)",
            )
            .fix(format!("wt login {url}")),
            false,
        ),
        Ok(Err(e)) => (Check::error(name, e.to_string()), false),
        Err(_) => (
            Check::error(
                name,
                format!("/auth/me timed out after {}s", timeout.as_secs()),
            ),
            false,
        ),
    }
}

#[derive(Deserialize)]
struct SessionView {
    #[serde(default)]
    current: bool,
    #[serde(default)]
    expires_at: Option<DateTime<Utc>>,
}

/// Expiry of the current sign-in session (`GET /api/v1/auth/sessions`).
pub async fn expiry_check(
    name: &str,
    client: &RestClient,
    timeout: Duration,
    now: DateTime<Utc>,
) -> Check {
    let sessions = match tokio::time::timeout(
        timeout,
        client.get::<Vec<SessionView>>("/api/v1/auth/sessions"),
    )
    .await
    {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => return Check::skip(name, format!("expiry not available: {e}")),
        Err(_) => return Check::skip(name, "expiry not available: timed out"),
    };
    let Some(current) = sessions.into_iter().find(|s| s.current) else {
        return Check::skip(name, "API token (expiry is not reported for API tokens)");
    };
    let Some(expires) = current.expires_at else {
        return Check::ok(name, "the session does not expire");
    };
    let left = expires - now;
    if left <= chrono::Duration::zero() {
        Check::error(
            name,
            format!("the session expired {}", expires.to_rfc3339()),
        )
        .fix("wt login")
    } else if left < chrono::Duration::days(EXPIRY_WARN_DAYS) {
        Check::warn(
            name,
            format!("the session expires in {}", human_duration(left)),
        )
        .fix("log in again soon with `wt login`")
    } else {
        Check::ok(
            name,
            format!("the session expires in {}", human_duration(left)),
        )
    }
}

fn human_duration(d: chrono::Duration) -> String {
    let hours = d.num_hours();
    if hours >= 48 {
        plural((hours / 24) as usize, "day", "days")
    } else if hours >= 1 {
        plural(hours as usize, "hour", "hours")
    } else {
        plural(d.num_minutes().max(1) as usize, "minute", "minutes")
    }
}

// ---------------------------------------------------------------------------
// Daemon

/// What asking the daemon for its status gave: `Ok(None)` = no bg.json.
type DaemonAnswer = std::result::Result<Option<DaemonStatus>, String>;

async fn daemon_status(timeout: Duration) -> DaemonAnswer {
    let info = match ipc::read_daemon_info() {
        Ok(Some(info)) => info,
        Ok(None) => return Ok(None),
        Err(e) => {
            return Err(format!(
                "cannot read {}: {e}",
                ipc::daemon_info_path().display()
            ))
        }
    };
    match tokio::time::timeout(
        timeout,
        ipc::send_to(info.port, &info.token, &IpcRequest::Status),
    )
    .await
    {
        Ok(Ok(IpcResponse::Status(status))) => Ok(Some(status)),
        Ok(Ok(IpcResponse::Error { message })) => Err(message),
        Ok(Ok(IpcResponse::Ok { message })) => Err(format!("unexpected answer: {message}")),
        Ok(Err(SdkError::DaemonNotRunning)) => {
            Err("bg.json exists but no daemon answers (it was not shut down cleanly)".into())
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!("no answer within {}s", timeout.as_secs())),
    }
}

pub fn daemon_check(answer: &DaemonAnswer, root: Option<&Path>, auto_snapshot: bool) -> Check {
    let status = match answer {
        Ok(Some(status)) => status,
        Ok(None) if root.is_some() && auto_snapshot => {
            return Check::warn(
                "daemon",
                "not running; auto-snapshots of this worktree are off",
            )
            .fix("wt daemon start");
        }
        Ok(None) => return Check::ok("daemon", "not running"),
        Err(e) => {
            return Check::warn("daemon", e.clone()).fix("wt daemon restart");
        }
    };
    let running = format!("running (pid {}, version {})", status.pid, status.version);
    if status.version != CLI_VERSION {
        return Check::warn("daemon", format!("{running}, but wt is {CLI_VERSION}"))
            .fix("wt daemon restart");
    }
    let Some(root) = root.filter(|_| auto_snapshot) else {
        return Check::ok("daemon", running);
    };
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| crate::paths::normalize(p));
    let root = canon(root);
    let Some(watched) = status
        .watched
        .iter()
        .find(|w| root.starts_with(canon(&w.root)))
    else {
        return Check::warn("daemon", format!("{running}, not watching this worktree"))
            .fix("wt daemon watch");
    };
    if watched.paused {
        return Check::warn(
            "daemon",
            format!("{running}, watching this worktree (paused)"),
        )
        .fix("wt daemon resume");
    }
    if let Some(err) = watched
        .last_error
        .as_deref()
        .or(watched.sync_error.as_deref())
    {
        return Check::warn("daemon", format!("{running}, last error: {err}"))
            .fix("wt daemon logs");
    }
    Check::ok("daemon", format!("{running}, watching this worktree"))
}

// ---------------------------------------------------------------------------
// Signing

#[derive(Deserialize)]
struct ServerKey {
    id: String,
    #[serde(default)]
    revoked_at: Option<String>,
}

/// The configured signing key: present locally, registered, and (online)
/// still known and not revoked on its server.
pub async fn signing_check(
    keys: &KeyStore,
    key: &str,
    auto: bool,
    online: bool,
    timeout: Duration,
) -> Check {
    const NAME: &str = "signing";
    if key.is_empty() {
        return Check::skip(NAME, "no signing key configured ([signing] key)");
    }
    if !keys.exists(key) {
        return Check::error(
            NAME,
            format!(
                "key '{key}' is configured but missing in {}",
                keys.dir().display()
            ),
        )
        .fix(format!("wt identity generate {key}"));
    }
    let info = match keys.info(key) {
        Ok(info) => info,
        Err(e) => return Check::error(NAME, e.to_string()),
    };
    let mode = if auto { "auto" } else { "with --sign" };
    let what = format!("key '{key}' ({}, signs {mode})", info.fingerprint);
    if info.key_id.is_empty() || info.server.is_empty() {
        return Check::warn(NAME, format!("{what} is not registered with a server"))
            .fix(format!("wt identity register {key}"));
    }
    if !online {
        return Check::ok(NAME, format!("{what} registered on {}", info.server));
    }
    let Ok(client) = RestClient::for_server(&info.server) else {
        return Check::ok(
            NAME,
            format!(
                "{what} registered on {} (not verified: not logged in)",
                info.server
            ),
        );
    };
    match tokio::time::timeout(timeout, client.get::<Vec<ServerKey>>("/api/v1/auth/keys")).await {
        Ok(Ok(server_keys)) => registration_check(&what, &info.key_id, &info.server, &server_keys),
        _ => Check::ok(
            NAME,
            format!(
                "{what} registered on {} (not verified: server unreachable)",
                info.server
            ),
        ),
    }
}

fn registration_check(what: &str, key_id: &str, server: &str, keys: &[ServerKey]) -> Check {
    const NAME: &str = "signing";
    match keys.iter().find(|k| k.id == key_id) {
        None => Check::error(
            NAME,
            format!("{what}: registration {key_id} no longer exists on {server}"),
        )
        .fix("register it again with `wt identity register`"),
        Some(k) if k.revoked_at.as_deref().is_some_and(|r| !r.is_empty()) => Check::error(
            NAME,
            format!("{what} was revoked on {server}"),
        )
        .fix("generate and register a new key: `wt identity generate NAME && wt identity register NAME`"),
        Some(_) => Check::ok(NAME, format!("{what} registered on {server}")),
    }
}

// ---------------------------------------------------------------------------
// Hooks

const PHASES: [&str; 2] = ["pre-snapshot", "post-snapshot"];
/// Hook extensions recognized on Windows, in priority order ("" = bare).
const WINDOWS_EXTENSIONS: [&str; 5] = ["exe", "cmd", "bat", "ps1", ""];

/// Hooks and problems found in one hooks directory.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct HookScan {
    pub hooks: Vec<PathBuf>,
    /// (problem, fix)
    pub problems: Vec<(String, String)>,
}

/// Inspect `dir` the way hook discovery reads it (see `worktree_sdk::hooks`).
pub fn scan_hook_dir(dir: &Path, windows: bool, has_sh: bool) -> HookScan {
    let mut scan = HookScan::default();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return scan;
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    let mut chosen: BTreeMap<&str, (usize, PathBuf)> = BTreeMap::new();
    for path in files {
        let file = path
            .file_name()
            .map(|f| f.to_string_lossy().into_owned())
            .unwrap_or_default();
        if file.starts_with('.') || file.ends_with(".sample") {
            continue;
        }
        let (stem, ext) = match file.split_once('.') {
            Some((s, e)) => (s.to_string(), e.to_ascii_lowercase()),
            None => (file.clone(), String::new()),
        };
        let phase = PHASES.iter().find(|p| **p == stem);
        let rank = if windows {
            WINDOWS_EXTENSIONS.iter().position(|e| *e == ext)
        } else {
            ext.is_empty().then_some(0)
        };
        let (Some(phase), Some(rank)) = (phase, rank) else {
            let expected = if windows {
                "pre-snapshot / post-snapshot (.exe, .cmd, .bat, .ps1 or no extension)"
            } else {
                "pre-snapshot / post-snapshot (no extension)"
            };
            scan.problems.push((
                format!("{} is not a hook name and never runs", path.display()),
                format!("rename it to {expected}"),
            ));
            continue;
        };
        if !windows && !is_executable(&path) {
            scan.problems.push((
                format!("{} is not executable and is skipped", path.display()),
                format!("chmod +x {}", path.display()),
            ));
            continue;
        }
        if windows && ext.is_empty() && !has_sh {
            scan.problems.push((
                format!("{} runs with sh, which is not on PATH", path.display()),
                "install Git for Windows (sh.exe) or use a .cmd/.ps1 hook".into(),
            ));
        }
        match chosen.get(phase) {
            Some((best, other)) if *best <= rank => {
                scan.problems.push((
                    format!(
                        "{} is ignored: {} runs instead",
                        path.display(),
                        other.display()
                    ),
                    "keep one hook file per phase".into(),
                ));
            }
            Some((_, other)) => {
                scan.problems.push((
                    format!(
                        "{} is ignored: {} runs instead",
                        other.display(),
                        path.display()
                    ),
                    "keep one hook file per phase".into(),
                ));
                chosen.insert(phase, (rank, path));
            }
            None => {
                chosen.insert(phase, (rank, path));
            }
        }
    }
    scan.hooks = chosen.into_values().map(|(_, p)| p).collect();
    scan
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_path: &Path) -> bool {
    true
}

/// Whether `program` (or `program.exe`) is found on PATH.
fn on_path(program: &str) -> bool {
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path)
        .any(|dir| dir.join(program).is_file() || dir.join(format!("{program}.exe")).is_file())
}

fn hooks_check(repo: &Repository, enabled: bool) -> Check {
    const NAME: &str = "hooks";
    let mut dirs = vec![repo.wt_dir().join(worktree_sdk::hooks::HOOKS_DIR)];
    for tree in repo.trees().unwrap_or_default() {
        if tree.path.as_os_str().is_empty() {
            continue;
        }
        if let Ok(dir) = repo.tree_dir(&tree.name) {
            dirs.push(
                dir.join(TREE_MARKER_DIR)
                    .join(worktree_sdk::hooks::HOOKS_DIR),
            );
        }
    }
    let windows = cfg!(windows);
    let has_sh = !windows || on_path("sh");
    let mut hooks = Vec::new();
    let mut problems = Vec::new();
    for dir in dirs {
        let scan = scan_hook_dir(&dir, windows, has_sh);
        hooks.extend(scan.hooks);
        problems.extend(scan.problems);
    }
    if let Some((problem, fix)) = problems.first() {
        let more = if problems.len() > 1 {
            format!(" (+{} more)", problems.len() - 1)
        } else {
            String::new()
        };
        return Check::warn(NAME, format!("{problem}{more}")).fix(fix.clone());
    }
    if hooks.is_empty() {
        return Check::skip(NAME, "no hooks");
    }
    let shown: Vec<String> = hooks
        .iter()
        .map(|h| {
            h.strip_prefix(repo.root())
                .unwrap_or(h)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    if !enabled {
        return Check::warn(
            NAME,
            format!(
                "hooks are disabled ([hooks] enabled = false): {}",
                shown.join(", ")
            ),
        )
        .fix("wt config set hooks.enabled true");
    }
    Check::ok(NAME, shown.join(", "))
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode, Uri};
    use axum::response::IntoResponse;
    use axum::{Json, Router};
    use serde_json::{json, Value};
    use worktree_sdk::ipc::WatchedWorktree;

    use super::*;
    use crate::commands::server_common::testing::parse;

    const TOKEN: &str = "wt_doctor_token";

    /// Canned (status, body) per path; requests need `Bearer TOKEN` except
    /// /healthz.
    type Routes = Arc<Mutex<BTreeMap<String, (u16, Value)>>>;

    async fn handle(
        State(routes): State<Routes>,
        uri: Uri,
        headers: HeaderMap,
    ) -> impl IntoResponse {
        let path = uri.path().to_string();
        if path != "/healthz" {
            let auth = headers
                .get("authorization")
                .and_then(|v| v.to_str().ok())
                .unwrap_or_default();
            if auth != format!("Bearer {TOKEN}") {
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({"error": {"code": "unauthenticated", "message": "bad token"}})),
                );
            }
        }
        let routes = routes.lock().unwrap();
        match routes.get(&path) {
            Some((status, body)) => (StatusCode::from_u16(*status).unwrap(), Json(body.clone())),
            None => (
                StatusCode::NOT_FOUND,
                Json(json!({"error": {"code": "not_found", "message": path}})),
            ),
        }
    }

    async fn fake_server(routes: &[(&str, u16, Value)]) -> String {
        let map: BTreeMap<String, (u16, Value)> = routes
            .iter()
            .map(|(p, s, b)| (p.to_string(), (*s, b.clone())))
            .collect();
        let state: Routes = Arc::new(Mutex::new(map));
        let app = Router::new().fallback(handle).with_state(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        format!("http://{addr}")
    }

    async fn ping_ok(_: String) -> worktree_sdk::Result<remote::PingInfo> {
        Ok(remote::PingInfo {
            server_version: "0.9.0".into(),
            username: Some("ada".into()),
            tenant: None,
        })
    }

    async fn ping_fails(_: String) -> worktree_sdk::Result<remote::PingInfo> {
        Err(SdkError::Remote("h2 protocol error".into()))
    }

    fn statuses(checks: &[Check]) -> Vec<(String, Status)> {
        checks.iter().map(|c| (c.name.clone(), c.status)).collect()
    }

    const T: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn healthy_server_with_valid_login() {
        let soon = (Utc::now() + chrono::Duration::days(30)).to_rfc3339();
        let url = fake_server(&[
            ("/healthz", 200, json!("ok")),
            ("/api/v1/auth/me", 200, json!({"account": {"username": "ada"}, "memberships": []})),
            (
                "/api/v1/auth/sessions",
                200,
                json!([{"id": "s0", "current": false}, {"id": "s1", "current": true, "expires_at": soon}]),
            ),
        ])
        .await;
        let checks = server_checks("origin", &url, Some(TOKEN.into()), T, ping_ok).await;
        assert_eq!(
            statuses(&checks),
            [
                ("origin: server".into(), Status::Ok),
                ("origin: sync".into(), Status::Ok),
                ("origin: version".into(), Status::Ok),
                ("origin: clock".into(), Status::Ok),
                ("origin: credential".into(), Status::Ok),
                ("origin: token".into(), Status::Ok),
            ],
            "{checks:#?}"
        );
        assert!(checks[4].message.contains("ada"), "{checks:#?}");
        assert!(checks[5].message.contains("days"), "{checks:#?}");
    }

    #[tokio::test]
    async fn rejected_credential_and_failed_ping() {
        let url = fake_server(&[("/healthz", 503, json!("probe failed"))]).await;
        let checks = server_checks("up", &url, Some("wrong".into()), T, ping_fails).await;
        assert_eq!(
            statuses(&checks),
            [
                ("up: server".into(), Status::Warn),
                ("up: sync".into(), Status::Error),
                ("up: clock".into(), Status::Ok),
                ("up: credential".into(), Status::Error),
            ],
            "{checks:#?}"
        );
        assert_eq!(
            checks[3].fix.as_deref(),
            Some(format!("wt login {url}").as_str())
        );
    }

    #[tokio::test]
    async fn not_logged_in_and_expiring_session() {
        let url = fake_server(&[("/healthz", 200, json!("ok"))]).await;
        let checks = server_checks("origin", &url, None, T, ping_ok).await;
        let last = checks.last().unwrap();
        assert_eq!(
            (last.name.as_str(), last.status),
            ("origin: credential", Status::Warn)
        );

        let tomorrow = (Utc::now() + chrono::Duration::hours(20)).to_rfc3339();
        let url = fake_server(&[(
            "/api/v1/auth/sessions",
            200,
            json!([{"id": "s1", "current": true, "expires_at": tomorrow}]),
        )])
        .await;
        let client = RestClient::new(&url, Some(TOKEN.into())).unwrap();
        let check = expiry_check("t", &client, T, Utc::now()).await;
        assert_eq!(check.status, Status::Warn, "{check:?}");
        assert!(check.message.contains("hours"), "{check:?}");
        let check = expiry_check("t", &client, T, Utc::now() + chrono::Duration::days(2)).await;
        assert_eq!(check.status, Status::Error, "{check:?}");

        // An API token has no current session.
        let url = fake_server(&[("/api/v1/auth/sessions", 200, json!([]))]).await;
        let client = RestClient::new(&url, Some(TOKEN.into())).unwrap();
        assert_eq!(
            expiry_check("t", &client, T, Utc::now()).await.status,
            Status::Skip
        );
    }

    #[tokio::test]
    async fn unreachable_server_is_one_fast_error() {
        // Bind and drop: nothing listens on this port any more.
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let url = format!("http://127.0.0.1:{port}");
        let started = Instant::now();
        let checks = server_checks("origin", &url, Some(TOKEN.into()), T, ping_ok).await;
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(
            statuses(&checks),
            [("origin: server".into(), Status::Error)]
        );
        assert!(checks[0].message.contains("cannot reach"), "{checks:?}");
    }

    #[test]
    fn clock_skew_thresholds() {
        let now = Utc::now();
        let probe = |offset: i64| Probe {
            status: 200,
            date: Some(now + chrono::Duration::seconds(offset)),
            midpoint: now,
        };
        assert_eq!(clock_check("c", &probe(2)).status, Status::Ok);
        let warn = clock_check("c", &probe(-90));
        assert_eq!(warn.status, Status::Warn);
        assert!(warn.message.contains("90s ahead of"), "{warn:?}");
        let err = clock_check("c", &probe(600));
        assert_eq!(err.status, Status::Error);
        assert!(err.message.contains("behind"), "{err:?}");
        let none = Probe {
            date: None,
            ..probe(0)
        };
        assert_eq!(clock_check("c", &none).status, Status::Skip);
    }

    #[test]
    fn version_skew() {
        assert_eq!(version_check("v", "0.1.0", "0.2.0-dev").status, Status::Ok);
        assert_eq!(version_check("v", "1.4.0", "v1.0.0").status, Status::Ok);
        assert_eq!(version_check("v", "1.4.0", "2.0.0").status, Status::Warn);
        assert_eq!(version_check("v", "0.1.0", "dev").status, Status::Ok);
    }

    fn status(version: &str, watched: Vec<WatchedWorktree>) -> DaemonAnswer {
        Ok(Some(DaemonStatus {
            pid: 42,
            version: version.into(),
            started_at: Utc::now(),
            watched,
        }))
    }

    fn watched(root: &Path, paused: bool, error: Option<&str>) -> WatchedWorktree {
        WatchedWorktree {
            root: root.to_path_buf(),
            paused,
            pending_changes: 0,
            snapshots_created: 0,
            last_snapshot: None,
            last_snapshot_at: None,
            last_stage_at: None,
            last_error: error.map(str::to_string),
            last_fetch_at: None,
            sync_state: None,
            sync_error: None,
        }
    }

    #[test]
    fn daemon_states() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let check = |answer: &DaemonAnswer, auto: bool| daemon_check(answer, Some(root), auto);
        assert_eq!(check(&Ok(None), true).status, Status::Warn);
        assert_eq!(
            check(&Ok(None), true).fix.as_deref(),
            Some("wt daemon start")
        );
        assert_eq!(check(&Ok(None), false).status, Status::Ok);
        assert_eq!(daemon_check(&Ok(None), None, true).status, Status::Ok);
        assert_eq!(check(&Err("stale".into()), true).status, Status::Warn);
        assert_eq!(
            check(&status(CLI_VERSION, vec![]), true).fix.as_deref(),
            Some("wt daemon watch")
        );
        assert_eq!(
            check(&status(CLI_VERSION, vec![]), false).status,
            Status::Ok
        );
        assert_eq!(
            check(&status("0.0.1-old", vec![]), true).fix.as_deref(),
            Some("wt daemon restart")
        );
        let ok = check(&status(CLI_VERSION, vec![watched(root, false, None)]), true);
        assert_eq!(ok.status, Status::Ok, "{ok:?}");
        assert!(ok.message.contains("watching this worktree"));
        assert_eq!(
            check(&status(CLI_VERSION, vec![watched(root, true, None)]), true)
                .fix
                .as_deref(),
            Some("wt daemon resume")
        );
        assert_eq!(
            check(
                &status(CLI_VERSION, vec![watched(root, false, Some("disk full"))]),
                true
            )
            .status,
            Status::Warn
        );
    }

    #[tokio::test]
    async fn signing_key_states() {
        let dir = tempfile::tempdir().unwrap();
        let keys = KeyStore::new(dir.path());
        assert_eq!(
            signing_check(&keys, "", false, false, T).await.status,
            Status::Skip
        );
        let missing = signing_check(&keys, "work", true, false, T).await;
        assert_eq!(missing.status, Status::Error);
        assert_eq!(missing.fix.as_deref(), Some("wt identity generate work"));
        keys.generate("work", false).unwrap();
        let unregistered = signing_check(&keys, "work", true, false, T).await;
        assert_eq!(unregistered.status, Status::Warn, "{unregistered:?}");
        keys.set_registration("work", "key-1", "http://s").unwrap();
        assert_eq!(
            signing_check(&keys, "work", true, false, T).await.status,
            Status::Ok
        );

        let present = [ServerKey {
            id: "key-1".into(),
            revoked_at: None,
        }];
        assert_eq!(
            registration_check("k", "key-1", "s", &present).status,
            Status::Ok
        );
        assert_eq!(
            registration_check("k", "key-2", "s", &present).status,
            Status::Error
        );
        let revoked = [ServerKey {
            id: "key-1".into(),
            revoked_at: Some("2026-01-01T00:00:00Z".into()),
        }];
        assert_eq!(
            registration_check("k", "key-1", "s", &revoked).status,
            Status::Error
        );
    }

    #[test]
    fn hook_directories() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert_eq!(
            scan_hook_dir(&d.join("missing"), false, true),
            HookScan::default()
        );
        for f in [
            "pre-snapshot",
            "pre-snapshot.cmd",
            "post-snapshot.ps1",
            "pre-commit",
            "x.sample",
        ] {
            std::fs::write(d.join(f), "exit 0\n").unwrap();
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                d.join("pre-snapshot"),
                std::fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
        let win = scan_hook_dir(d, true, false);
        let names: Vec<String> = win
            .hooks
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["post-snapshot.ps1", "pre-snapshot.cmd"]);
        let problems: Vec<&str> = win.problems.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(problems.len(), 3, "{problems:#?}");
        assert!(problems
            .iter()
            .any(|p| p.contains("pre-commit") && p.contains("not a hook name")));
        assert!(problems.iter().any(|p| p.contains("not on PATH")));
        assert!(problems.iter().any(|p| p.contains("is ignored")));

        let unix = scan_hook_dir(d, false, true);
        assert_eq!(unix.hooks.len(), 1);
        assert!(unix.hooks[0].ends_with("pre-snapshot"));
        // .cmd/.ps1 do not run on Unix, nor does pre-commit.
        assert_eq!(unix.problems.len(), 3, "{:#?}", unix.problems);

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                d.join("pre-snapshot"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap();
            let scan = scan_hook_dir(d, false, true);
            assert!(scan.hooks.is_empty());
            assert!(scan
                .problems
                .iter()
                .any(|(_, fix)| fix.starts_with("chmod +x")));
        }
    }

    #[test]
    fn local_checks() {
        let mut config = WorktreeConfig::new("w", "main");
        config.user.name = "Ada".into();
        config.user.email = "ada@example.com".into();
        assert_eq!(user_check(Some(&config)).status, Status::Ok);

        assert_eq!(alias_check(&BTreeMap::new()), None);
        let aliases: BTreeMap<String, String> = [("st".to_string(), "status".to_string())].into();
        assert_eq!(alias_check(&aliases).unwrap().status, Status::Ok);
        let shadow: BTreeMap<String, String> = [("log".to_string(), "status".to_string())].into();
        assert_eq!(alias_check(&shadow).unwrap().status, Status::Warn);
        let cycle: BTreeMap<String, String> = [
            ("a".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ]
        .into();
        assert_eq!(alias_check(&cycle).unwrap().status, Status::Error);

        let dir = tempfile::tempdir().unwrap();
        let repo = Repository::init(dir.path(), Default::default()).unwrap();
        let checks = config_checks(Some(&repo));
        assert!(checks
            .iter()
            .any(|c| c.name == "config.worktree" && c.status == Status::Ok));
        assert_eq!(hooks_check(&repo, true).status, Status::Skip);
    }

    #[test]
    fn renders_marks_and_fixes() {
        colored::control::set_override(false);
        let checks = [
            Check::ok("worktree", "/w"),
            Check::warn("user", "user.email not set").fix("wt config set --global user.email x"),
            Check::error("origin: server", "cannot reach"),
            Check::skip("hooks", "no hooks"),
        ];
        let mut t = Text::default();
        render(&mut t, &checks, 1, 1);
        let text = t.as_str();
        assert!(text.contains("✓ worktree        /w"), "{text}");
        assert!(text.contains("! user  "), "{text}");
        assert!(
            text.contains("fix: wt config set --global user.email x"),
            "{text}"
        );
        assert!(text.contains("✗ origin: server  cannot reach"), "{text}");
        assert!(text.contains("- hooks"), "{text}");
        assert!(text.ends_with("1 error, 1 warning\n"), "{text}");
        let json = serde_json::to_value(&checks[1]).unwrap();
        assert_eq!(json["status"], "warn");
        assert!(serde_json::to_value(&checks[0])
            .unwrap()
            .get("fix")
            .is_none());
    }

    #[test]
    fn parses_flags() {
        let crate::cli::Command::Doctor(a) = parse(&["doctor"]) else {
            panic!()
        };
        assert!(!a.offline);
        assert_eq!(a.timeout, 5);
        let crate::cli::Command::Doctor(a) =
            parse(&["doctor", "--offline", "--timeout", "2", "--json"])
        else {
            panic!()
        };
        assert!(a.offline);
        assert_eq!(a.timeout, 2);
    }
}
