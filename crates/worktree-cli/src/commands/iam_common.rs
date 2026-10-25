//! Shared helpers for the IAM command groups (ENGINEERING_PLAN §12.4):
//! running REST calls, rendering server JSON, aligned tables, confirmation
//! prompts, `--file`/stdin input and the common argument shapes.

use std::future::Future;
use std::io::{IsTerminal, Read};
use std::path::Path;

use chrono::{DateTime, Local};
use clap::Args;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use worktree_sdk::remote::rest::seg;

use crate::commands::auth::prompt_line;
use crate::context::block_on;
use crate::error::{CliError, Result};
use crate::output::{Output, Text};

/// Placeholder for empty table cells.
pub const NONE: &str = "-";

/// Run one SDK REST call to completion.
pub fn call<T>(future: impl Future<Output = worktree_sdk::Result<T>>) -> Result<T> {
    Ok(block_on(future)??)
}

/// `/api/v1/tenants/{tenant}` (plus `rest`, which starts with '/' or is empty).
pub fn tenant_path(tenant: &str, rest: &str) -> String {
    format!("/api/v1/tenants/{}{rest}", seg(tenant))
}

/// Print `value` (server JSON) as-is with `--json`; otherwise decode it as
/// `T` and render it with `human`.
pub fn show<T: DeserializeOwned>(
    out: &Output,
    value: &Value,
    human: impl FnOnce(&mut Text, &T),
) -> Result<()> {
    if out.is_json() {
        return out.emit(value, |_| {});
    }
    let parsed: T = serde_json::from_value(value.clone())
        .map_err(|e| CliError::failure(format!("unexpected response from the server: {e}")))?;
    out.emit(value, |t| human(t, &parsed))
}

/// An aligned plain-text table with upper-case headers.
#[derive(Debug)]
pub struct Table {
    headers: Vec<&'static str>,
    rows: Vec<Vec<String>>,
}

impl Table {
    pub fn new(headers: &[&'static str]) -> Self {
        Self {
            headers: headers.to_vec(),
            rows: Vec::new(),
        }
    }

    /// Add a row; missing cells render empty, empty cells as [`NONE`].
    pub fn row<I, S>(&mut self, cells: I)
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.rows.push(
            cells
                .into_iter()
                .map(|c| {
                    let c = c.into();
                    if c.is_empty() {
                        NONE.to_string()
                    } else {
                        c
                    }
                })
                .collect(),
        );
    }

    /// Render the table, or `empty` when it has no rows.
    pub fn render(&self, t: &mut Text, empty: &str) {
        if self.rows.is_empty() {
            t.line(empty);
            return;
        }
        let mut widths: Vec<usize> = self.headers.iter().map(|h| h.len()).collect();
        for row in &self.rows {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.chars().count());
            }
        }
        let line = |cells: &mut dyn Iterator<Item = &str>| {
            let mut out = String::new();
            for (i, (cell, width)) in cells.zip(&widths).enumerate() {
                if i > 0 {
                    out.push_str("  ");
                }
                out.push_str(cell);
                let pad = width.saturating_sub(cell.chars().count());
                out.extend(std::iter::repeat_n(' ', pad));
            }
            out.trim_end().to_string()
        };
        let header = self
            .headers
            .iter()
            .map(|h| h.to_uppercase())
            .collect::<Vec<_>>();
        t.line(line(&mut header.iter().map(String::as_str)));
        for row in &self.rows {
            t.line(line(&mut row.iter().map(String::as_str)));
        }
    }
}

/// `key: value` lines with aligned values; empty values render as [`NONE`].
pub fn fields(t: &mut Text, pairs: &[(&str, String)]) {
    let width = pairs.iter().map(|(k, _)| k.len() + 1).max().unwrap_or(0);
    for (key, value) in pairs {
        let value = if value.is_empty() { NONE } else { value };
        t.line(format_args!("{:width$} {value}", format!("{key}:")));
    }
}

/// Local `YYYY-MM-DD HH:MM` for an RFC 3339 server timestamp; empty input
/// stays empty and unparsable input is shown as-is.
pub fn when(timestamp: &str) -> String {
    DateTime::parse_from_rfc3339(timestamp).map_or_else(
        |_| timestamp.to_string(),
        |at| {
            at.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        },
    )
}

/// "yes" / "" for boolean table cells.
pub fn yes(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        ""
    }
}

/// Ask before a destructive action unless `--yes` was given. Without a
/// terminal on stdin there is nobody to ask, so `--yes` is required.
pub fn confirm(assume_yes: bool, action: &str) -> Result<()> {
    if assume_yes {
        return Ok(());
    }
    if !std::io::stdin().is_terminal() {
        return Err(
            CliError::usage(format!("refusing to {action} without confirmation"))
                .with_hint("pass --yes to confirm"),
        );
    }
    let answer = prompt_line(&format!("{action}? [y/N] ")).unwrap_or_default();
    if matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
        Ok(())
    } else {
        Err(CliError::failure("aborted"))
    }
}

/// `--yes` for destructive commands.
#[derive(Debug, Args)]
pub struct YesArgs {
    /// Do not ask for confirmation
    #[arg(short = 'y', long)]
    pub yes: bool,
}

/// Contents of `path`, or of stdin when `path` is `-`.
pub fn read_input(path: &Path) -> Result<String> {
    if path == Path::new("-") {
        let mut text = String::new();
        std::io::stdin().read_to_string(&mut text)?;
        return Ok(text);
    }
    std::fs::read_to_string(path)
        .map_err(|e| CliError::failure(format!("cannot read '{}': {e}", path.display())))
}

/// A server tree named `tenant/tree`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRef {
    pub tenant: String,
    pub name: String,
}

impl TreeRef {
    /// `/api/v1/trees/{tenant}/{tree}` plus `rest`.
    pub fn path(&self, rest: &str) -> String {
        format!(
            "/api/v1/trees/{}/{}{rest}",
            seg(&self.tenant),
            seg(&self.name)
        )
    }
}

impl std::str::FromStr for TreeRef {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        match s.split_once('/') {
            Some((tenant, name))
                if !tenant.is_empty() && !name.is_empty() && !name.contains('/') =>
            {
                Ok(Self {
                    tenant: tenant.to_string(),
                    name: name.to_string(),
                })
            }
            _ => Err(format!("expected TENANT/TREE, got '{s}'")),
        }
    }
}

impl std::fmt::Display for TreeRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.tenant, self.name)
    }
}

/// A principal spelled `user:<name>` or `sa:<tenant>/<name>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrincipalRef {
    User(String),
    ServiceAccount { tenant: String, name: String },
}

impl std::str::FromStr for PrincipalRef {
    type Err = String;

    fn from_str(s: &str) -> std::result::Result<Self, String> {
        let bad = || format!("expected user:<name> or sa:<tenant>/<name>, got '{s}'");
        match s.split_once(':') {
            Some(("user", name)) if !name.is_empty() => Ok(Self::User(name.to_string())),
            Some(("sa", rest)) => match rest.split_once('/') {
                Some((tenant, name)) if !tenant.is_empty() && !name.is_empty() => {
                    Ok(Self::ServiceAccount {
                        tenant: tenant.to_string(),
                        name: name.to_string(),
                    })
                }
                _ => Err(bad()),
            },
            _ => Err(bad()),
        }
    }
}

impl std::fmt::Display for PrincipalRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User(name) => write!(f, "user:{name}"),
            Self::ServiceAccount { tenant, name } => write!(f, "sa:{tenant}/{name}"),
        }
    }
}

/// An account as embedded in member lists (`{account: {username, ..}}`).
#[derive(Debug, Deserialize)]
pub struct AccountRef {
    pub username: String,
}

/// A tree grant (`domain.Grant`).
#[derive(Debug, Deserialize)]
pub struct Grant {
    pub subject_type: String,
    pub subject: String,
    pub role: String,
    #[serde(default)]
    pub created_by: String,
    #[serde(default)]
    pub created_at: String,
}

/// Grants as a table.
pub fn grant_table(grants: &[Grant]) -> Table {
    let mut table = Table::new(&["subject", "type", "role", "created by", "created"]);
    for g in grants {
        table.row([
            g.subject.clone(),
            g.subject_type.clone(),
            g.role.clone(),
            g.created_by.clone(),
            when(&g.created_at),
        ]);
    }
    table
}

/// An API token, session or service-account key (`domain.APIToken`).
#[derive(Debug, Deserialize)]
pub struct ApiToken {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub scopes: Vec<String>,
}

/// Tokens or keys as a table.
pub fn token_table(tokens: &[ApiToken]) -> Table {
    let mut table = Table::new(&["id", "name", "scopes", "created", "expires", "last used"]);
    for k in tokens {
        table.row([
            k.id.clone(),
            k.name.clone(),
            scopes(&k.scopes),
            when(&k.created_at),
            k.expires_at
                .as_deref()
                .map(when)
                .unwrap_or_else(|| "never".into()),
            k.last_used_at.as_deref().map(when).unwrap_or_default(),
        ]);
    }
    table
}

/// `--scope` lists render as "all" when unrestricted.
pub fn scopes(scopes: &[String]) -> String {
    if scopes.is_empty() {
        "all".into()
    } else {
        scopes.join(",")
    }
}

/// Body of `POST /auth/tokens` and `POST .../service-accounts/{name}/keys`.
#[derive(Debug, Serialize)]
pub struct NewToken<'a> {
    pub name: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttl_hours: Option<u32>,
    pub scopes: &'a [String],
}

/// Response of the token-creating endpoints: the raw secret, shown once.
#[derive(Debug, Deserialize)]
pub struct CreatedToken {
    pub token: String,
    pub api_token: ApiToken,
}

/// Print a freshly created secret with the one-time warning.
pub fn reveal_token(t: &mut Text, what: &str, created: &CreatedToken) {
    t.line(format_args!(
        "Created {what} '{}' (id {})",
        created.api_token.name, created.api_token.id
    ));
    t.blank();
    t.line(format_args!("    {}", created.token));
    t.blank();
    t.line("Copy it now: this token is shown only once and cannot be retrieved later.");
}

/// Parse a `wt` command line in tests.
#[cfg(test)]
pub fn parse(args: &[&str]) -> crate::cli::Command {
    use clap::Parser;
    let argv = std::iter::once("wt").chain(args.iter().copied());
    crate::cli::Cli::try_parse_from(argv)
        .unwrap_or_else(|e| panic!("failed to parse {args:?}: {e}"))
        .command
}

/// The clap error kind of a command line that must not parse.
#[cfg(test)]
pub fn parse_err(args: &[&str]) -> clap::error::ErrorKind {
    use clap::Parser;
    let argv = std::iter::once("wt").chain(args.iter().copied());
    match crate::cli::Cli::try_parse_from(argv) {
        Ok(cli) => panic!("{args:?} should not parse, got {:?}", cli.command),
        Err(e) => e.kind(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_align_and_mark_empty_cells() {
        let mut table = Table::new(&["name", "role", "description"]);
        table.row(["alice", "owner", "first"]);
        table.row(["bob-the-builder", "", "x"]);
        let mut t = Text::default();
        table.render(&mut t, "none");
        assert_eq!(
            t.as_str(),
            "NAME             ROLE   DESCRIPTION\n\
             alice            owner  first\n\
             bob-the-builder  -      x\n"
        );
        let mut t = Text::default();
        Table::new(&["a"]).render(&mut t, "No rows.");
        assert_eq!(t.as_str(), "No rows.\n");
    }

    #[test]
    fn field_lists_align() {
        let mut t = Text::default();
        fields(
            &mut t,
            &[("name", "x".into()), ("description", String::new())],
        );
        assert_eq!(t.as_str(), "name:        x\ndescription: -\n");
    }

    #[test]
    fn timestamps_render_locally_or_verbatim() {
        let shown = when("2026-10-25T12:30:00Z");
        assert_eq!(shown.len(), 16, "{shown}");
        assert!(shown.starts_with("2026-10-2"), "{shown}");
        assert_eq!(when("soon"), "soon");
        assert_eq!(when(""), "");
    }

    #[test]
    fn tree_refs_parse() {
        let tree: TreeRef = "acme/web".parse().unwrap();
        assert_eq!(tree.to_string(), "acme/web");
        assert_eq!(tree.path("/grants"), "/api/v1/trees/acme/web/grants");
        for bad in ["acme", "/web", "acme/", "a/b/c"] {
            assert!(bad.parse::<TreeRef>().is_err(), "{bad}");
        }
    }

    #[test]
    fn principals_parse() {
        assert_eq!("user:ada".parse(), Ok(PrincipalRef::User("ada".into())));
        let sa: PrincipalRef = "sa:acme/ci".parse().unwrap();
        assert_eq!(
            sa,
            PrincipalRef::ServiceAccount {
                tenant: "acme".into(),
                name: "ci".into()
            }
        );
        assert_eq!(sa.to_string(), "sa:acme/ci");
        for bad in ["ada", "user:", "sa:ci", "team:x", "sa:/ci"] {
            assert!(bad.parse::<PrincipalRef>().is_err(), "{bad}");
        }
    }

    #[test]
    fn paths_encode_segments() {
        assert_eq!(tenant_path("a b", "/teams"), "/api/v1/tenants/a%20b/teams");
        assert_eq!(tenant_path("acme", ""), "/api/v1/tenants/acme");
    }

    #[test]
    fn token_tables_and_reveal() {
        let created: CreatedToken = serde_json::from_value(serde_json::json!({
            "token": "wt_secret",
            "api_token": {"id": "k1", "name": "ci", "created_at": "2026-01-01T00:00:00Z",
                          "scopes": ["tree:read"], "kind": "api"}
        }))
        .unwrap();
        let mut t = Text::default();
        reveal_token(&mut t, "API token", &created);
        assert!(t.as_str().contains("    wt_secret\n"), "{}", t.as_str());
        assert!(t.as_str().contains("shown only once"));
        let mut t = Text::default();
        token_table(std::slice::from_ref(&created.api_token)).render(&mut t, "");
        let out = t.as_str();
        assert!(out.starts_with("ID  NAME  SCOPES     CREATED"), "{out}");
        assert!(out.contains("tree:read") && out.contains("never"), "{out}");
        assert_eq!(scopes(&[]), "all");
    }

    #[test]
    fn confirm_with_yes_never_prompts() {
        assert!(confirm(true, "delete everything").is_ok());
    }

    #[test]
    fn reads_policy_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("p.json");
        std::fs::write(&file, "{}").unwrap();
        assert_eq!(read_input(&file).unwrap(), "{}");
        let err = read_input(&dir.path().join("missing.json")).unwrap_err();
        assert!(err.to_string().contains("cannot read"), "{err}");
    }
}
