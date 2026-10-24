//! `wt remote` and remote/server name resolution shared by the sync and
//! auth commands.

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::config::RemoteSection;
use worktree_sdk::{Repository, SdkError};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct RemoteArgs {
    #[command(subcommand)]
    pub command: RemoteCommand,
}

#[derive(Debug, Subcommand)]
pub enum RemoteCommand {
    /// List configured remotes
    List,
    /// Add a remote
    Add {
        name: String,
        /// Server base URL, e.g. http://localhost:7420
        url: String,
        /// Tenant slug on the server
        #[arg(long, value_name = "T")]
        tenant: String,
    },
    /// Remove a remote
    Remove { name: String },
}

#[derive(Serialize)]
struct RemoteEntry<'a> {
    name: &'a str,
    url: &'a str,
    tenant: &'a str,
}

#[derive(Serialize)]
struct Removed<'a> {
    remote: &'a str,
    removed: bool,
}

pub fn run(ctx: &Context, args: RemoteArgs) -> Result<()> {
    let mut repo = ctx.open()?;
    match args.command {
        RemoteCommand::List => {
            let remotes: Vec<RemoteEntry<'_>> = repo
                .config()
                .remote
                .iter()
                .map(|(name, r)| RemoteEntry {
                    name,
                    url: &r.url,
                    tenant: &r.tenant,
                })
                .collect();
            ctx.out.emit(&remotes, |t| {
                let width = remotes.iter().map(|r| r.name.len()).max().unwrap_or(0);
                for r in &remotes {
                    t.line(format_args!(
                        "{}  {}  (tenant {})",
                        format!("{:<width$}", r.name).bold(),
                        r.url,
                        r.tenant.cyan()
                    ));
                }
            })
        }
        RemoteCommand::Add { name, url, tenant } => {
            let mut config = repo.config().clone();
            if config.remote.contains_key(&name) {
                return Err(CliError::failure(format!("remote '{name}' already exists"))
                    .with_hint(format!("remove it first with `wt remote remove {name}`")));
            }
            let url = normalize_server(&url);
            config.remote.insert(
                name.clone(),
                RemoteSection {
                    url: url.clone(),
                    tenant: tenant.clone(),
                },
            );
            repo.set_config(config)?;
            ctx.out.emit(
                &RemoteEntry {
                    name: &name,
                    url: &url,
                    tenant: &tenant,
                },
                |t| {
                    t.line(format_args!(
                        "Added remote {} -> {url} (tenant {tenant})",
                        name.bold()
                    ));
                },
            )
        }
        RemoteCommand::Remove { name } => {
            let mut config = repo.config().clone();
            if config.remote.remove(&name).is_none() {
                return Err(SdkError::RemoteNotConfigured(name).into());
            }
            repo.set_config(config)?;
            ctx.out.emit(
                &Removed {
                    remote: &name,
                    removed: true,
                },
                |t| {
                    t.line(format_args!("Removed remote {}", name.bold()));
                },
            )
        }
    }
}

/// `--remote`, else `[sync].remote` (default "origin").
pub fn remote_name(repo: &Repository, explicit: Option<String>) -> String {
    explicit.unwrap_or_else(|| repo.config().sync.remote.clone())
}

/// Server URL of a configured remote.
pub fn remote_url(repo: &Repository, remote: &str) -> Result<String> {
    repo.config()
        .remote
        .get(remote)
        .map(|r| normalize_server(&r.url))
        .ok_or_else(|| SdkError::RemoteNotConfigured(remote.to_string()).into())
}

/// Canonical server URL: trimmed, `http://` added when no scheme is given,
/// no trailing slash (credentials are keyed by this form).
pub fn normalize_server(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    if url.contains("://") {
        url.to_string()
    } else {
        format!("http://{url}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_server_urls() {
        assert_eq!(
            normalize_server("http://localhost:7420/"),
            "http://localhost:7420"
        );
        assert_eq!(
            normalize_server(" localhost:7420 "),
            "http://localhost:7420"
        );
        assert_eq!(
            normalize_server("https://wt.example.com//"),
            "https://wt.example.com"
        );
        assert_eq!(
            normalize_server("https://wt.example.com/base"),
            "https://wt.example.com/base"
        );
    }
}
