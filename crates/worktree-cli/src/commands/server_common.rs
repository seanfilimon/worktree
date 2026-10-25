//! Helpers shared by the server-side command groups (`wt tenant`, `wt trees`,
//! `wt mr`, ...): a blocking REST wrapper, `TENANT/TREE` arguments with the
//! worktree default, aligned tables, confirmation and password prompts.

use std::fmt;
use std::future::Future;
use std::io::{BufRead, IsTerminal, Write};
use std::str::FromStr;

use chrono::{DateTime, Datelike, Utc};
use colored::{ColoredString, Colorize};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;
use worktree_sdk::remote::rest::seg;
use worktree_sdk::remote::{RestClient, ORIGIN};
use worktree_sdk::{Repository, SdkError, TreeInfo};

use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::ago;
use crate::output::{Output, Text};

/// Environment variable consulted for passwords before prompting.
const PASSWORD_ENV: &str = "WT_PASSWORD";

// ---------------------------------------------------------------------------
// REST access

/// A [`RestClient`] plus the runtime that drives it, so a command can make
/// several calls over one connection pool.
pub struct Api {
    client: RestClient,
    runtime: tokio::runtime::Runtime,
}

impl Api {
    /// Connect to the server chosen by [`Context::rest`].
    pub fn connect(ctx: &Context) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| CliError::failure(format!("cannot start the async runtime: {e}")))?;
        Ok(Self {
            client: ctx.rest()?,
            runtime,
        })
    }

    pub fn client(&self) -> &RestClient {
        &self.client
    }

    /// Drive `future` to completion on this client's runtime.
    pub fn block_on<F: Future>(&self, future: F) -> F::Output {
        self.runtime.block_on(future)
    }

    pub fn get(&self, path: &str) -> Result<Value> {
        Ok(self.block_on(self.client.get(path))?)
    }

    pub fn get_query(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        Ok(self.block_on(self.client.get_query(path, query))?)
    }

    pub fn post<B: Serialize + ?Sized>(&self, path: &str, body: &B) -> Result<Value> {
        Ok(self.block_on(self.client.post(path, body))?)
    }

    pub fn put<B: Serialize + ?Sized>(&self, path: &str, body: &B) -> Result<Value> {
        Ok(self.block_on(self.client.put(path, body))?)
    }

    pub fn patch<B: Serialize + ?Sized>(&self, path: &str, body: &B) -> Result<Value> {
        Ok(self.block_on(self.client.patch(path, body))?)
    }

    /// DELETE `path`; `query` is appended percent-encoded.
    pub fn delete(&self, path: &str, query: &[(&str, &str)]) -> Result<()> {
        let mut url = path.to_string();
        for (i, (key, value)) in query.iter().enumerate() {
            url.push(if i == 0 { '?' } else { '&' });
            url.push_str(&format!("{key}={}", seg(value)));
        }
        Ok(self.block_on(self.client.delete(&url))?)
    }
}

/// `/api/v1/tenants/{slug}`.
pub fn tenant_path(slug: &str) -> String {
    format!("/api/v1/tenants/{}", seg(slug))
}

/// Print `value` verbatim with `--json`; otherwise decode it as `T` and hand
/// it to `human`.
pub fn render<T: DeserializeOwned>(
    out: &Output,
    value: &Value,
    human: impl FnOnce(&mut Text, T),
) -> Result<()> {
    if out.is_json() {
        return out.emit(value, |_| {});
    }
    let typed = decode(value)?;
    out.emit(value, |t| human(t, typed))
}

/// Decode a server response into a typed view.
pub fn decode<T: DeserializeOwned>(value: &Value) -> Result<T> {
    T::deserialize(value)
        .map_err(|e| CliError::failure(format!("unexpected response from the server: {e}")))
}

/// Deserialize `null` as the type's default (Go encodes empty slices as null).
pub fn nullable<'de, D, T>(deserializer: D) -> std::result::Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

// ---------------------------------------------------------------------------
// TENANT/TREE arguments

/// A server tree named `tenant/tree`.
///
/// Positional `TREE` arguments carry the clap id `tree_ref`: the id `tree`
/// belongs to the global `--tree` flag, and clap copies values of a
/// global's id from subcommands up to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRef {
    pub tenant: String,
    pub name: String,
}

impl TreeRef {
    /// `/api/v1/trees/{tenant}/{tree}`.
    pub fn api(&self) -> String {
        format!("/api/v1/trees/{}/{}", seg(&self.tenant), seg(&self.name))
    }
}

impl fmt::Display for TreeRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.tenant, self.name)
    }
}

impl FromStr for TreeRef {
    type Err = String;

    fn from_str(spec: &str) -> std::result::Result<Self, Self::Err> {
        match spec.split_once('/') {
            Some((tenant, name))
                if !tenant.is_empty() && !name.is_empty() && !name.contains('/') =>
            {
                Ok(Self {
                    tenant: tenant.to_string(),
                    name: name.to_string(),
                })
            }
            _ => Err(format!("'{spec}' is not of the form TENANT/TREE")),
        }
    }
}

/// `explicit`, else the tree of the current worktree on its origin remote.
pub fn tree_or_default(ctx: &Context, explicit: Option<TreeRef>) -> Result<TreeRef> {
    if let Some(tree) = explicit {
        return Ok(tree);
    }
    let hint = "pass the tree as TENANT/TREE";
    let repo = match Repository::open(ctx.cwd()) {
        Ok(repo) => repo,
        Err(SdkError::NotAWorktree) => {
            return Err(CliError::usage("no TREE given and not inside a worktree").with_hint(hint));
        }
        Err(e) => return Err(e.into()),
    };
    let tree = ctx.select_tree(&repo)?;
    default_tree(&repo, &tree)
}

/// The server tree backing local `tree`: the origin tenant plus the tree's
/// remote name.
pub fn default_tree(repo: &Repository, tree: &TreeInfo) -> Result<TreeRef> {
    let tenant = origin_tenant(repo).ok_or_else(|| {
        CliError::usage(format!(
            "no TREE given and the worktree has no '{ORIGIN}' remote"
        ))
        .with_hint("pass the tree as TENANT/TREE, or add a remote with `wt remote add origin <url> --tenant <tenant>`")
    })?;
    let name = if tree.remote_name.is_empty() {
        &tree.name
    } else {
        &tree.remote_name
    };
    Ok(TreeRef {
        tenant,
        name: name.clone(),
    })
}

/// Tenant of the origin remote of the worktree containing the cwd, if any.
pub fn cwd_origin_tenant(ctx: &Context) -> Option<String> {
    Repository::open(ctx.cwd())
        .ok()
        .and_then(|repo| origin_tenant(&repo))
}

fn origin_tenant(repo: &Repository) -> Option<String> {
    repo.config().remote.get(ORIGIN).map(|r| r.tenant.clone())
}

// ---------------------------------------------------------------------------
// Tables and formatting

/// How a table cell is colored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Paint {
    Plain,
    Bold,
    Green,
    Red,
    Yellow,
    Cyan,
}

/// One table cell: its text and color.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    text: String,
    paint: Paint,
}

impl Cell {
    fn painted(self, paint: Paint) -> Self {
        Self { paint, ..self }
    }

    pub fn bold(self) -> Self {
        self.painted(Paint::Bold)
    }

    pub fn green(self) -> Self {
        self.painted(Paint::Green)
    }

    pub fn red(self) -> Self {
        self.painted(Paint::Red)
    }

    pub fn yellow(self) -> Self {
        self.painted(Paint::Yellow)
    }

    pub fn cyan(self) -> Self {
        self.painted(Paint::Cyan)
    }

    /// The text with its color applied.
    pub fn colored(&self) -> ColoredString {
        let text = self.text.as_str();
        match self.paint {
            Paint::Plain => text.normal(),
            Paint::Bold => text.bold(),
            Paint::Green => text.green(),
            Paint::Red => text.red(),
            Paint::Yellow => text.yellow(),
            Paint::Cyan => text.cyan(),
        }
    }
}

impl From<String> for Cell {
    fn from(text: String) -> Self {
        Self {
            text,
            paint: Paint::Plain,
        }
    }
}

impl From<&str> for Cell {
    fn from(text: &str) -> Self {
        text.to_string().into()
    }
}

/// A table with a header row; columns are padded to their widest cell
/// (measured without color codes) and separated by two spaces.
#[derive(Debug)]
pub struct Table {
    header: Vec<&'static str>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    pub fn new(header: &[&'static str]) -> Self {
        Self {
            header: header.to_vec(),
            rows: Vec::new(),
        }
    }

    pub fn row(&mut self, cells: impl IntoIterator<Item = Cell>) {
        self.rows.push(cells.into_iter().collect());
    }

    /// Render the table, or `empty` when it has no rows.
    pub fn render(&self, t: &mut Text, empty: &str) {
        if self.rows.is_empty() {
            t.line(empty.dimmed());
            return;
        }
        let mut widths: Vec<usize> = self.header.iter().map(|h| h.chars().count()).collect();
        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                let w = cell.text.chars().count();
                match widths.get_mut(i) {
                    Some(width) => *width = (*width).max(w),
                    None => widths.push(w),
                }
            }
        }
        let header: Vec<Cell> = self.header.iter().map(|&h| Cell::from(h).bold()).collect();
        for row in std::iter::once(&header).chain(&self.rows) {
            let mut line = String::new();
            for (i, cell) in row.iter().enumerate() {
                if i > 0 {
                    line.push_str("  ");
                }
                let last = i + 1 == row.len();
                let pad = if last {
                    0
                } else {
                    widths[i].saturating_sub(cell.text.chars().count())
                };
                line.push_str(&cell.colored().to_string());
                line.push_str(&" ".repeat(pad));
            }
            t.line(line.trim_end());
        }
    }
}

/// Aligned `key  value` lines, indented by two spaces.
pub fn fields(t: &mut Text, rows: &[(&str, String)]) {
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, value) in rows {
        t.line(format_args!(
            "  {}  {value}",
            format!("{key:<width$}").dimmed()
        ));
    }
}

/// Parse an RFC 3339 server timestamp; Go's zero time and garbage are None.
pub fn timestamp(ts: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|t| t.with_timezone(&Utc))
        .filter(|t| t.year() > 1)
}

/// Relative time of a server timestamp ("3 hours ago"), "-" when unset.
pub fn when(ts: &str) -> String {
    timestamp(ts).map_or_else(|| "-".to_string(), ago)
}

/// `text`, or "-" when empty.
pub fn or_dash(text: &str) -> String {
    if text.is_empty() {
        "-".into()
    } else {
        text.to_string()
    }
}

/// "yes" / "no".
pub fn yes_no(flag: bool) -> &'static str {
    if flag {
        "yes"
    } else {
        "no"
    }
}

/// Binary-prefixed byte sizes ("1.5 MiB").
pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    if n < 1024 {
        return format!("{n} B");
    }
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// Color one line of a unified diff.
pub fn colorize_patch_line(line: &str) -> ColoredString {
    if line.starts_with("+++") || line.starts_with("---") {
        line.bold()
    } else if line.starts_with('+') {
        line.green()
    } else if line.starts_with('-') {
        line.red()
    } else if line.starts_with("@@") {
        line.cyan()
    } else if line.starts_with('\\') {
        line.dimmed()
    } else {
        line.normal()
    }
}

// ---------------------------------------------------------------------------
// Prompts

/// Ask before a destructive `action` ("delete tree acme/web") unless `yes`.
/// Without a terminal on stdin the command is refused instead.
pub fn confirm(yes: bool, action: &str) -> Result<()> {
    if yes {
        return Ok(());
    }
    let stdin = std::io::stdin();
    if !stdin.is_terminal() {
        return Err(
            CliError::usage(format!("refusing to {action} without confirmation"))
                .with_hint("pass --yes to confirm"),
        );
    }
    let mut stderr = std::io::stderr();
    write!(stderr, "{}? [y/N] ", capitalize(action))?;
    stderr.flush()?;
    if is_yes(&first_line(stdin.lock())?) {
        Ok(())
    } else {
        Err(CliError::failure("aborted"))
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

fn is_yes(answer: &str) -> bool {
    matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes")
}

/// A password to set: the first stdin line with `--password-stdin`, else
/// `$WT_PASSWORD`, else a no-echo prompt entered twice.
pub fn new_password(from_stdin: bool) -> Result<String> {
    let password = if from_stdin {
        first_line(std::io::stdin().lock())?
    } else if let Some(password) = std::env::var(PASSWORD_ENV).ok().filter(|p| !p.is_empty()) {
        password
    } else {
        let password = rpassword::prompt_password("New password: ")?;
        if rpassword::prompt_password("Confirm password: ")? != password {
            return Err(CliError::failure("passwords do not match"));
        }
        password
    };
    if password.is_empty() {
        return Err(CliError::failure("the password must not be empty"));
    }
    Ok(password)
}

/// First line of `reader` without its line terminator.
fn first_line(mut reader: impl BufRead) -> Result<String> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    Ok(line.trim_end_matches(['\r', '\n']).to_string())
}

/// Command-line parsing helpers for the per-module clap tests.
#[cfg(test)]
pub mod testing {
    use clap::error::ErrorKind;
    use clap::Parser;

    use crate::cli::{Cli, Command};

    pub fn parse(args: &[&str]) -> Command {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        match Cli::try_parse_from(argv) {
            Ok(cli) => cli.command,
            Err(e) => panic!("failed to parse {args:?}: {e}"),
        }
    }

    pub fn parse_err(args: &[&str]) -> ErrorKind {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        match Cli::try_parse_from(argv) {
            Ok(cli) => panic!("{args:?} should not parse, got {:?}", cli.command),
            Err(e) => e.kind(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use worktree_sdk::config::RemoteSection;
    use worktree_sdk::InitOptions;

    use super::*;

    #[test]
    fn parses_tree_refs() {
        let tree: TreeRef = "acme/web".parse().expect("parse");
        assert_eq!(tree.tenant, "acme");
        assert_eq!(tree.name, "web");
        assert_eq!(tree.to_string(), "acme/web");
        for bad in ["acme", "/web", "acme/", "a/b/c", ""] {
            assert!(bad.parse::<TreeRef>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn tree_paths_are_encoded() {
        let tree = TreeRef {
            tenant: "acme".into(),
            name: "my tree".into(),
        };
        assert_eq!(tree.api(), "/api/v1/trees/acme/my%20tree");
        assert_eq!(tenant_path("acme"), "/api/v1/tenants/acme");
    }

    fn init_repo(dir: &Path) -> Repository {
        Repository::init(
            dir,
            InitOptions {
                name: Some("web".into()),
                default_branch: None,
            },
        )
        .expect("init")
    }

    #[test]
    fn default_tree_uses_origin_tenant_and_remote_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut repo = init_repo(dir.path());
        let root = repo.tree("web").expect("root tree");
        let err = default_tree(&repo, &root).expect_err("no origin");
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("origin"), "{err}");

        let mut config = repo.config().clone();
        config.remote.insert(
            ORIGIN.into(),
            RemoteSection {
                url: "http://localhost:7420".into(),
                tenant: "acme".into(),
            },
        );
        repo.set_config(config).expect("save config");
        let tree = default_tree(&repo, &root).expect("default");
        assert_eq!(tree.to_string(), "acme/web");

        let renamed = TreeInfo {
            remote_name: "website".into(),
            ..root
        };
        assert_eq!(
            default_tree(&repo, &renamed).expect("default").to_string(),
            "acme/website"
        );
    }

    #[test]
    fn tables_align_columns_and_handle_empty() {
        colored::control::set_override(false);
        let mut table = Table::new(&["NAME", "ROLE", "NOTE"]);
        table.row([Cell::from("ada"), Cell::from("owner").green(), "".into()]);
        table.row([
            Cell::from("grace"),
            "developer".into(),
            Cell::from("x").cyan(),
        ]);
        let mut t = Text::default();
        table.render(&mut t, "nothing");
        assert_eq!(
            t.as_str(),
            "NAME   ROLE       NOTE\nada    owner\ngrace  developer  x\n"
        );

        let mut t = Text::default();
        Table::new(&["A"]).render(&mut t, "No rows.");
        assert_eq!(t.as_str(), "No rows.\n");
    }

    #[test]
    fn fields_align_keys() {
        colored::control::set_override(false);
        let mut t = Text::default();
        fields(&mut t, &[("kind", "org".into()), ("created", "now".into())]);
        assert_eq!(t.as_str(), "  kind     org\n  created  now\n");
    }

    #[test]
    fn timestamps() {
        assert_eq!(when(""), "-");
        assert_eq!(when("0001-01-01T00:00:00Z"), "-");
        assert_eq!(when("not a time"), "-");
        assert!(when("2020-01-01T00:00:00Z").ends_with("years ago"));
        assert!(timestamp("2026-01-02T03:04:05.123456789Z").is_some());
    }

    #[test]
    fn small_formatters() {
        assert_eq!(or_dash(""), "-");
        assert_eq!(or_dash("x"), "x");
        assert_eq!(yes_no(true), "yes");
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1023), "1023 B");
        assert_eq!(bytes(1536), "1.5 KiB");
        assert_eq!(bytes(5 * 1024 * 1024), "5.0 MiB");
        assert_eq!(capitalize("delete tree"), "Delete tree");
        assert_eq!(capitalize(""), "");
    }

    #[test]
    fn answers() {
        for yes in ["y", "Y", "yes", " YES "] {
            assert!(is_yes(yes), "{yes:?}");
        }
        for no in ["", "n", "no", "yep"] {
            assert!(!is_yes(no), "{no:?}");
        }
        assert_eq!(first_line(&b"pw\r\nrest"[..]).expect("read"), "pw");
    }

    #[test]
    fn nullable_vectors() {
        #[derive(Deserialize)]
        struct S {
            #[serde(default, deserialize_with = "nullable")]
            items: Vec<String>,
        }
        let s: S = serde_json::from_str(r#"{"items":null}"#).expect("null");
        assert!(s.items.is_empty());
        let s: S = serde_json::from_str("{}").expect("missing");
        assert!(s.items.is_empty());
        let s: S = serde_json::from_str(r#"{"items":["a"]}"#).expect("list");
        assert_eq!(s.items, ["a"]);
    }

    #[test]
    fn patch_lines_are_classified() {
        assert_eq!(colorize_patch_line("+x"), "+x".green());
        assert_eq!(colorize_patch_line("-x"), "-x".red());
        assert_eq!(colorize_patch_line("+++ b/x"), "+++ b/x".bold());
        assert_eq!(colorize_patch_line("@@ -1 +1 @@"), "@@ -1 +1 @@".cyan());
        assert_eq!(colorize_patch_line(" ctx"), " ctx".normal());
    }
}
