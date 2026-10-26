//! `wt compliance` — secret scanning and license compliance
//! (ENGINEERING_PLAN §16.7, specs/security/SecretScanning.md).
//!
//! - `wt compliance scan [PATH]` scans the working files of the current
//!   tree with the client rule set (the same rules the server runs on every
//!   push) and prints each finding with its fingerprint; `--server` scans a
//!   snapshot on the server instead. Both exit 1 when a secret is found.
//! - `findings`, `resolve`, `allow`, `disallow` and `settings` manage the
//!   server side: findings recorded on push, the allowlist of known false
//!   positives and the tree's scanning mode (`off`, `warn`, `block`).
//!
//! The license audit is `wt license audit`.

use std::fs;
use std::path::{Path, PathBuf};

use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use worktree_sdk::remote::rest::seg;
use worktree_sdk::scan::{SecretFinding, SecretScanner};
use worktree_sdk::{Repository, TreeInfo};

use crate::commands::server_common::{
    confirm, decode, nullable, or_dash, render, tree_or_default, when, Api, Cell, Table, TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::Text;

/// Local files larger than this are not scanned.
const MAX_LOCAL_FILE: u64 = 8 * 1024 * 1024;

#[derive(Debug, Args)]
pub struct ComplianceArgs {
    #[command(subcommand)]
    pub command: ComplianceCommand,
}

/// Which server tree a command addresses.
#[derive(Debug, Clone, Default, Args)]
pub struct Scope {
    /// Server tree (TENANT/TREE; default: the current worktree's tree)
    // Shares the id of the global `--tree` so it replaces that flag here.
    #[arg(long, value_name = "TENANT/TREE", value_parser = tree_spec)]
    pub tree: Option<String>,
}

fn tree_spec(spec: &str) -> std::result::Result<String, String> {
    spec.parse::<TreeRef>().map(|_| spec.to_string())
}

#[derive(Debug, Subcommand)]
pub enum ComplianceCommand {
    /// Scan for secrets: the tree's working files (default) or, with
    /// --server, a snapshot on the server; exits 1 when a secret is found
    ///
    /// Each finding shows its fingerprint, which `wt compliance allow`
    /// accepts. Locally the tree's `[secret_scanning]` configuration applies
    /// (exclude globs, extra patterns); inline `wt:allow-secret` markers are
    /// honoured in both modes.
    Scan {
        /// File or directory to scan (default: the whole tree; local only)
        path: Option<PathBuf>,
        /// Scan on the server (optionally naming it: --server URL); the
        /// tree's server-side exclude globs, size cap and allowlist apply
        // Shares the id of the global `--server` so that `--server URL`
        // also selects the server.
        #[arg(
            long = "server",
            id = "server",
            value_name = "URL",
            num_args = 0..=1,
            default_missing_value = "",
            conflicts_with = "path"
        )]
        server: Option<String>,
        /// Branch, tag or snapshot to scan on the server (default: the
        /// default branch)
        #[arg(long = "ref", value_name = "REF", requires = "server")]
        reference: Option<String>,
    },
    /// List the secret findings recorded on push
    Findings {
        /// Which findings
        #[arg(long, value_enum, default_value = "open")]
        status: StatusFilter,
        /// At most this many
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[command(flatten)]
        scope: Scope,
    },
    /// Resolve a finding (or reopen it)
    Resolve {
        /// Finding id (see `wt compliance findings`)
        id: String,
        /// Why it is resolved
        #[arg(
            long = "as",
            value_enum,
            value_name = "RESOLUTION",
            required_unless_present = "reopen"
        )]
        resolution: Option<Resolution>,
        /// A note recorded with the resolution
        #[arg(long)]
        note: Option<String>,
        /// Set the finding back to open
        #[arg(long, conflicts_with = "resolution")]
        reopen: bool,
        #[command(flatten)]
        scope: Scope,
    },
    /// Allowlist a secret by fingerprint (a known false positive), or list
    /// the allowlist
    Allow {
        /// Fingerprint (24 hex digits, shown with every finding)
        #[arg(required_unless_present = "list")]
        fingerprint: Option<String>,
        /// Why the secret may be pushed (required)
        #[arg(long, required_unless_present = "list")]
        reason: Option<String>,
        /// List the allowlist instead
        #[arg(long, conflicts_with_all = ["fingerprint", "reason"])]
        list: bool,
        #[command(flatten)]
        scope: Scope,
    },
    /// Remove a fingerprint from the allowlist
    Disallow {
        fingerprint: String,
        /// Do not ask for confirmation
        #[arg(long, short = 'y')]
        yes: bool,
        #[command(flatten)]
        scope: Scope,
    },
    /// Show or change the tree's secret scanning settings
    Settings {
        /// off (no scanning), warn (record and notify; default) or block
        /// (reject pushes with secrets)
        #[arg(long, value_enum)]
        mode: Option<Mode>,
        /// Never scan paths matching this glob (repeatable; replaces the
        /// list)
        #[arg(
            long = "exclude",
            value_name = "GLOB",
            conflicts_with = "clear_exclude"
        )]
        exclude: Vec<String>,
        /// Remove every exclude glob
        #[arg(long)]
        clear_exclude: bool,
        /// Skip files larger than this many bytes (0 = 1 MiB default)
        #[arg(long, value_name = "BYTES")]
        max_file_bytes: Option<u64>,
        #[command(flatten)]
        scope: Scope,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StatusFilter {
    Open,
    Resolved,
    All,
}

impl StatusFilter {
    fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Resolved => "resolved",
            Self::All => "all",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// The secret was revoked / rotated
    Revoked,
    /// Not a secret
    FalsePositive,
    /// A test or example value
    TestData,
    /// Accepted risk
    WontFix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Off,
    Warn,
    Block,
}

/// `scan.Finding`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Finding {
    id: String,
    path: String,
    line: u64,
    rule: String,
    excerpt: String,
    fingerprint: String,
    branch: String,
    status: String,
    action: String,
    count: u64,
    pushed_by: String,
    last_seen_at: String,
    resolution: String,
}

/// `scan.FindingList`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FindingList {
    #[serde(deserialize_with = "nullable")]
    findings: Vec<Finding>,
    open: u64,
    resolved: u64,
}

/// `scan.AllowEntry`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AllowEntry {
    fingerprint: String,
    reason: String,
    created_by: String,
    created_at: String,
}

/// `scan.AllowList`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AllowList {
    #[serde(deserialize_with = "nullable")]
    entries: Vec<AllowEntry>,
}

/// `scan.Settings`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Settings {
    mode: String,
    #[serde(deserialize_with = "nullable")]
    exclude: Vec<String>,
    max_file_bytes: u64,
    updated_by: String,
}

/// `scan.ScanMatch`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ScanMatch {
    rule: String,
    path: String,
    line: u64,
    excerpt: String,
    fingerprint: String,
    allowed: bool,
}

/// `scan.ScanResult`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ScanResult {
    snapshot_id: String,
    #[serde(rename = "ref")]
    reference: String,
    mode: String,
    files: u64,
    skipped: u64,
    truncated: bool,
    #[serde(deserialize_with = "nullable")]
    matches: Vec<ScanMatch>,
}

/// Result of a local scan (`--json`).
#[derive(Debug, Serialize)]
struct LocalScan<'a> {
    tree: &'a str,
    files: usize,
    skipped: usize,
    findings: &'a [SecretFinding],
}

fn base(tree: &TreeRef) -> String {
    format!("{}/compliance", tree.api())
}

fn resolve_scope(ctx: &Context, scope: &Scope) -> Result<TreeRef> {
    let tree = match &scope.tree {
        Some(spec) => Some(spec.parse::<TreeRef>().map_err(CliError::usage)?),
        None => None,
    };
    tree_or_default(ctx, tree)
}

pub fn run(ctx: &Context, args: ComplianceArgs) -> Result<()> {
    let out = &ctx.out;
    match args.command {
        ComplianceCommand::Scan {
            path,
            server,
            reference,
        } => match server {
            Some(_) => server_scan(ctx, reference),
            None => local_scan(ctx, path),
        },
        ComplianceCommand::Findings {
            status,
            limit,
            scope,
        } => {
            let tree = resolve_scope(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let value = api.get_query(
                &format!("{}/findings", base(&tree)),
                &[
                    ("status", status.as_str().to_string()),
                    ("limit", limit.to_string()),
                ],
            )?;
            render(out, &value, |t, list: FindingList| {
                findings_table(t, &list, &tree)
            })
        }
        ComplianceCommand::Resolve {
            id,
            resolution,
            note,
            reopen,
            scope,
        } => {
            let tree = resolve_scope(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let body = resolve_body(resolution, note, reopen);
            let value = api.post(
                &format!("{}/findings/{}/resolve", base(&tree), seg(&id)),
                &body,
            )?;
            render(out, &value, |t, f: Finding| {
                if f.status == "open" {
                    t.line(format_args!("Reopened finding {}", f.id.bold()));
                } else {
                    t.line(format_args!(
                        "Resolved finding {} ({}:{}) as {}",
                        f.id.bold(),
                        f.path,
                        f.line,
                        f.resolution.green()
                    ));
                }
            })
        }
        ComplianceCommand::Allow {
            fingerprint,
            reason,
            list,
            scope,
        } => {
            let tree = resolve_scope(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            if list {
                let value = api.get(&format!("{}/allowlist", base(&tree)))?;
                return render(out, &value, |t, al: AllowList| allow_table(t, &al));
            }
            let body = json!({
                "fingerprint": fingerprint.unwrap_or_default().trim(),
                "reason": reason.unwrap_or_default().trim(),
            });
            let value = api.post(&format!("{}/allowlist", base(&tree)), &body)?;
            render(out, &value, |t, e: AllowEntry| {
                t.line(format_args!(
                    "Allowed secret {} on {tree}: {}",
                    e.fingerprint.green().bold(),
                    e.reason
                ));
            })
        }
        ComplianceCommand::Disallow {
            fingerprint,
            yes,
            scope,
        } => {
            let tree = resolve_scope(ctx, &scope)?;
            confirm(
                yes,
                &format!("remove {fingerprint} from the allowlist of {tree}"),
            )?;
            let api = Api::connect(ctx)?;
            api.delete(
                &format!("{}/allowlist", base(&tree)),
                &[("fingerprint", fingerprint.trim())],
            )?;
            out.emit(
                &json!({ "tree": tree.to_string(), "fingerprint": fingerprint, "removed": true }),
                |t| {
                    t.line(format_args!(
                        "Removed {} from the allowlist of {tree}",
                        fingerprint.bold()
                    ));
                },
            )
        }
        ComplianceCommand::Settings {
            mode,
            exclude,
            clear_exclude,
            max_file_bytes,
            scope,
        } => {
            let tree = resolve_scope(ctx, &scope)?;
            let api = Api::connect(ctx)?;
            let body = settings_body(mode, exclude, clear_exclude, max_file_bytes);
            let path = format!("{}/settings", base(&tree));
            let value = if body.is_empty() {
                api.get(&path)?
            } else {
                api.put(&path, &Value::Object(body))?
            };
            render(out, &value, |t, s: Settings| settings_view(t, &s, &tree))
        }
    }
}

/// The POST body of `wt compliance resolve`.
fn resolve_body(resolution: Option<Resolution>, note: Option<String>, reopen: bool) -> Value {
    let mut body = Map::new();
    if reopen {
        body.insert("reopen".into(), true.into());
    } else if let Some(r) = resolution {
        body.insert("resolution".into(), json!(r));
    }
    if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
        body.insert("note".into(), n.trim().into());
    }
    Value::Object(body)
}

/// The PUT body of `wt compliance settings` (only the given fields).
fn settings_body(
    mode: Option<Mode>,
    exclude: Vec<String>,
    clear_exclude: bool,
    max_file_bytes: Option<u64>,
) -> Map<String, Value> {
    let mut body = Map::new();
    if let Some(m) = mode {
        body.insert("mode".into(), json!(m));
    }
    let exclude: Vec<String> = exclude
        .into_iter()
        .map(|g| g.trim().to_string())
        .filter(|g| !g.is_empty())
        .collect();
    if clear_exclude {
        body.insert("exclude".into(), json!([]));
    } else if !exclude.is_empty() {
        body.insert("exclude".into(), json!(exclude));
    }
    if let Some(n) = max_file_bytes {
        body.insert("max_file_bytes".into(), n.into());
    }
    body
}

fn settings_view(t: &mut Text, s: &Settings, tree: &TreeRef) {
    t.line(format_args!(
        "Secret scanning on {tree}: {}",
        s.mode.as_str().bold()
    ));
    let max = if s.max_file_bytes == 0 {
        "1 MiB (default)".to_string()
    } else {
        format!("{} bytes", s.max_file_bytes)
    };
    t.line(format_args!("  max file size: {max}"));
    if s.exclude.is_empty() {
        t.line("  exclude: (none)");
    } else {
        t.line(format_args!("  exclude: {}", s.exclude.join(", ")));
    }
    if !s.updated_by.is_empty() {
        t.line(format!("  last changed by {}", s.updated_by).dimmed());
    }
}

fn findings_table(t: &mut Text, list: &FindingList, tree: &TreeRef) {
    let mut table = Table::new(&[
        "ID",
        "LOCATION",
        "RULE",
        "EXCERPT",
        "FINGERPRINT",
        "LAST PUSH",
        "SEEN",
    ]);
    for f in &list.findings {
        let push = match f.action.as_str() {
            "blocked" => Cell::from("blocked").red(),
            "bypassed" => Cell::from("bypassed").yellow(),
            other => Cell::from(other),
        };
        let mut location = format!("{}:{}", f.path, f.line);
        if f.status == "resolved" {
            location.push_str(&format!(" ({})", or_dash(&f.resolution)));
        }
        table.row([
            Cell::from(f.id.as_str()).bold(),
            Cell::from(location),
            Cell::from(f.rule.as_str()).cyan(),
            Cell::from(f.excerpt.as_str()),
            Cell::from(f.fingerprint.as_str()),
            push,
            Cell::from(format!(
                "{} x{} by {} on {}",
                when(&f.last_seen_at),
                f.count,
                or_dash(&f.pushed_by),
                or_dash(&f.branch)
            )),
        ]);
    }
    table.render(t, &format!("No findings on {tree}."));
    t.line(
        format!(
            "{} open, {} resolved. Allow a false positive with `wt compliance allow FINGERPRINT --reason ...`.",
            list.open, list.resolved
        )
        .dimmed(),
    );
}

fn allow_table(t: &mut Text, al: &AllowList) {
    let mut table = Table::new(&["FINGERPRINT", "REASON", "BY", "WHEN"]);
    for e in &al.entries {
        table.row([
            Cell::from(e.fingerprint.as_str()).green(),
            Cell::from(e.reason.as_str()),
            Cell::from(e.created_by.as_str()),
            Cell::from(when(&e.created_at)),
        ]);
    }
    table.render(t, "The allowlist is empty.");
}

/// `wt compliance scan --server`: POST {tree}/compliance/scan.
fn server_scan(ctx: &Context, reference: Option<String>) -> Result<()> {
    // The global `--tree` may name the server tree (TENANT/TREE).
    let explicit = match ctx.tree_flag().filter(|t| t.contains('/')) {
        Some(spec) => Some(spec.parse::<TreeRef>().map_err(CliError::usage)?),
        None => None,
    };
    let tree = tree_or_default(ctx, explicit)?;
    let api = Api::connect(ctx)?;
    let body = match &reference {
        Some(r) => json!({ "ref": r }),
        None => json!({}),
    };
    let value = api.post(&format!("{}/scan", base(&tree)), &body)?;
    let res: ScanResult = decode(&value)?;
    ctx.out.emit(&value, |t| {
        let mut table = Table::new(&["LOCATION", "RULE", "EXCERPT", "FINGERPRINT"]);
        for m in &res.matches {
            let fp = if m.allowed {
                Cell::from(format!("{} (allowed)", m.fingerprint)).green()
            } else {
                Cell::from(m.fingerprint.as_str())
            };
            table.row([
                Cell::from(format!("{}:{}", m.path, m.line)).bold(),
                Cell::from(m.rule.as_str()).cyan(),
                Cell::from(m.excerpt.as_str()),
                fp,
            ]);
        }
        table.render(t, "No secrets found.");
        let mut summary = format!(
            "Scanned {} file(s) of {} {} on the server (mode {})",
            res.files,
            tree,
            or_dash(&res.reference),
            res.mode
        );
        if res.skipped > 0 {
            summary.push_str(&format!("; {} skipped", res.skipped));
        }
        if res.truncated {
            summary.push_str("; stopped at the scan budget");
        }
        t.line(summary.dimmed());
        if !res.snapshot_id.is_empty() {
            t.line(format!("snapshot {}", res.snapshot_id).dimmed());
        }
    })?;
    let blocking = res.matches.iter().filter(|m| !m.allowed).count();
    if blocking > 0 {
        return Err(CliError::failure(format!(
            "{blocking} possible secret(s) found on the server"
        )));
    }
    Ok(())
}

/// `wt compliance scan [PATH]`: the tree's working files.
fn local_scan(ctx: &Context, path: Option<PathBuf>) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let scope = match &path {
        Some(p) => ctx
            .tree_paths(&repo, &tree, std::slice::from_ref(p))?
            .into_iter()
            .next()
            .unwrap_or_default(),
        None => String::new(),
    };
    let configured = repo.secret_scanner(&tree.name)?;
    // An explicit scan runs even when automatic scanning is disabled.
    let scanner = if configured.enabled() {
        configured
    } else {
        SecretScanner::default()
    };
    let (findings, files, skipped) = scan_tree(&repo, &tree, &scope, &scanner)?;
    let result = LocalScan {
        tree: &tree.name,
        files,
        skipped,
        findings: &findings,
    };
    ctx.out.emit(&result, |t| {
        let mut table = Table::new(&["LOCATION", "RULE", "EXCERPT", "FINGERPRINT"]);
        for f in &findings {
            table.row([
                Cell::from(format!("{}:{}", f.path, f.line)).bold(),
                Cell::from(f.rule.as_str()).cyan(),
                Cell::from(f.excerpt.as_str()),
                Cell::from(f.fingerprint.as_str()),
            ]);
        }
        table.render(t, "No secrets found.");
        let mut summary = format!("Scanned {files} file(s) of tree {}", tree.name);
        if skipped > 0 {
            summary.push_str(&format!("; {skipped} skipped (larger than 8 MiB or unreadable)"));
        }
        t.line(summary.dimmed());
        if !findings.is_empty() {
            t.line(
                "Remove the secrets, mark a line with `wt:allow-secret`, or allow a fingerprint on the server with `wt compliance allow`."
                    .dimmed(),
            );
        }
    })?;
    if findings.is_empty() {
        Ok(())
    } else {
        Err(CliError::failure(format!(
            "{} possible secret(s) found",
            findings.len()
        )))
    }
}

/// Scan the working files of `tree` under `scope` ("" = everything),
/// skipping ignored paths, nested trees and metadata directories. Returns
/// the findings, the number of files scanned and skipped.
fn scan_tree(
    repo: &Repository,
    tree: &TreeInfo,
    scope: &str,
    scanner: &SecretScanner,
) -> Result<(Vec<SecretFinding>, usize, usize)> {
    let base = repo.tree_dir(&tree.name)?;
    let root_prefix = tree.path.to_string_lossy().replace('\\', "/");
    let start = if scope.is_empty() {
        base.clone()
    } else {
        base.join(scope)
    };
    let mut findings = Vec::new();
    let (mut files, mut skipped) = (0usize, 0usize);
    let mut visit = |rel: &str, abs: &Path| {
        let data = match fs::metadata(abs) {
            Ok(m) if m.len() <= MAX_LOCAL_FILE => fs::read(abs).ok(),
            _ => None,
        };
        match data {
            Some(data) => {
                files += 1;
                findings.extend(scanner.scan(rel, &data));
            }
            None => skipped += 1,
        }
    };
    if start.is_file() {
        visit(scope, &start);
    } else if start.is_dir() {
        walk(repo, &root_prefix, &base, &start, &mut visit)?;
    } else {
        return Err(CliError::failure(format!(
            "'{scope}' does not exist in tree '{}'",
            tree.name
        )));
    }
    findings.sort_by(|a, b| (&a.path, a.line).cmp(&(&b.path, b.line)));
    Ok((findings, files, skipped))
}

fn walk(
    repo: &Repository,
    root_prefix: &str,
    base: &Path,
    dir: &Path,
    visit: &mut dyn FnMut(&str, &Path),
) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| CliError::failure(format!("cannot read '{}': {e}", dir.display())))?
        .filter_map(|e| e.ok())
        .collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let abs = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if matches!(name.as_str(), ".wt" | ".git") {
            continue;
        }
        let Ok(rel_path) = abs.strip_prefix(base) else {
            continue;
        };
        let rel = rel_path.to_string_lossy().replace('\\', "/");
        let root_rel = if root_prefix.is_empty() {
            rel.clone()
        } else {
            format!("{root_prefix}/{rel}")
        };
        if repo.is_ignored(&root_rel)? {
            continue;
        }
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            // Another tree's directory.
            if rel != ".wt-tree" && abs.join(".wt-tree").is_dir() {
                continue;
            }
            walk(repo, root_prefix, base, &abs, visit)?;
        } else if kind.is_file() {
            visit(&rel, &abs);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use clap::Parser;

    use super::*;
    use crate::cli::{Cli, Command};
    use crate::commands::server_common::testing::{parse, parse_err};

    fn compliance(args: &[&str]) -> ComplianceCommand {
        let mut full = vec!["compliance"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Compliance(a) => a.command,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_scan_modes() {
        assert!(matches!(
            compliance(&["scan"]),
            ComplianceCommand::Scan {
                path: None,
                server: None,
                reference: None
            }
        ));
        let ComplianceCommand::Scan {
            path,
            server,
            reference,
        } = compliance(&["scan", "src"])
        else {
            panic!()
        };
        assert_eq!(path, Some(PathBuf::from("src")));
        assert!(server.is_none() && reference.is_none());
        let ComplianceCommand::Scan {
            server, reference, ..
        } = compliance(&["scan", "--server", "--ref", "v1"])
        else {
            panic!()
        };
        assert_eq!(server.as_deref(), Some(""));
        assert_eq!(reference.as_deref(), Some("v1"));
        // `--server URL` also selects the server for the request.
        let cli = Cli::try_parse_from(["wt", "compliance", "scan", "--server", "http://h:1"])
            .expect("parse");
        assert_eq!(cli.global.server.as_deref(), Some("http://h:1"));
        assert_eq!(
            parse_err(&["compliance", "scan", "--ref", "v1"]),
            ErrorKind::MissingRequiredArgument
        );
        assert_eq!(
            parse_err(&["compliance", "scan", "src", "--server"]),
            ErrorKind::ArgumentConflict
        );
    }

    #[test]
    fn parses_server_commands() {
        let ComplianceCommand::Findings {
            status,
            limit,
            scope,
        } = compliance(&["findings", "--status", "all", "--tree", "acme/web"])
        else {
            panic!()
        };
        assert_eq!((status, limit), (StatusFilter::All, 100));
        assert_eq!(scope.tree.as_deref(), Some("acme/web"));
        assert_eq!(
            parse_err(&["compliance", "findings", "--tree", "web"]),
            ErrorKind::ValueValidation
        );

        let ComplianceCommand::Allow {
            fingerprint,
            reason,
            list,
            ..
        } = compliance(&["allow", "0123456789abcdef01234567", "--reason", "fixture"])
        else {
            panic!()
        };
        assert_eq!(fingerprint.as_deref(), Some("0123456789abcdef01234567"));
        assert_eq!(reason.as_deref(), Some("fixture"));
        assert!(!list);
        assert!(matches!(
            compliance(&["allow", "--list"]),
            ComplianceCommand::Allow { list: true, .. }
        ));
        assert_eq!(
            parse_err(&["compliance", "allow", "0123"]),
            ErrorKind::MissingRequiredArgument
        );

        let ComplianceCommand::Resolve {
            resolution, reopen, ..
        } = compliance(&["resolve", "abc", "--as", "false-positive"])
        else {
            panic!()
        };
        assert_eq!(resolution, Some(Resolution::FalsePositive));
        assert!(!reopen);
        assert_eq!(
            resolve_body(resolution, Some(" rotated ".into()), false),
            json!({ "resolution": "false_positive", "note": "rotated" })
        );
        assert_eq!(resolve_body(None, None, true), json!({ "reopen": true }));
        assert_eq!(
            parse_err(&["compliance", "resolve", "abc"]),
            ErrorKind::MissingRequiredArgument
        );
        assert!(matches!(
            compliance(&["resolve", "abc", "--reopen"]),
            ComplianceCommand::Resolve { reopen: true, .. }
        ));
    }

    #[test]
    fn settings_body_has_only_given_fields() {
        assert!(settings_body(None, vec![], false, None).is_empty());
        let body = settings_body(
            Some(Mode::Block),
            vec![" vendor/** ".into(), "".into()],
            false,
            Some(2048),
        );
        assert_eq!(
            Value::Object(body),
            json!({ "mode": "block", "exclude": ["vendor/**"], "max_file_bytes": 2048 })
        );
        assert_eq!(
            Value::Object(settings_body(None, vec![], true, None)),
            json!({ "exclude": [] })
        );
        let ComplianceCommand::Settings { mode, exclude, .. } = compliance(&[
            "settings",
            "--mode",
            "warn",
            "--exclude",
            "a/**",
            "--exclude",
            "b",
        ]) else {
            panic!()
        };
        assert_eq!(mode, Some(Mode::Warn));
        assert_eq!(exclude, vec!["a/**", "b"]);
    }

    #[test]
    fn decodes_server_types() {
        let list: FindingList = decode(&json!({
            "findings": [{ "id": "f1", "path": "a.env", "line": 2, "rule": "aws-access-key",
                "excerpt": "K=AKIA****", "fingerprint": "ab", "status": "open", "action": "blocked",
                "count": 3, "pushed_by": "bob", "branch": "main", "last_seen_at": "2026-01-01T00:00:00Z" }],
            "open": 1, "resolved": 0
        }))
        .expect("decode");
        assert_eq!(list.findings[0].count, 3);
        let empty: FindingList =
            decode(&json!({ "findings": null, "open": 0, "resolved": 0 })).expect("decode");
        assert!(empty.findings.is_empty());
        let res: ScanResult =
            decode(&json!({ "ref": "main", "matches": null, "files": 2 })).expect("decode");
        assert_eq!(res.reference, "main");
        let mut t = Text::default();
        findings_table(
            &mut t,
            &list,
            &TreeRef {
                tenant: "acme".into(),
                name: "web".into(),
            },
        );
        assert!(t.as_str().contains("a.env:2"), "{}", t.as_str());
    }
}
