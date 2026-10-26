//! Config-as-code (ENGINEERING_PLAN §16.3): `wt trees config show|validate`,
//! the local checks of `wt status` and `wt push`, and `wt branch
//! describe|owner`.
//!
//! The `.wt-tree/{config,branches,protections}.toml` and
//! `.wt-tree/access/**` files of a tree configure the server when they are
//! pushed to the tree's default branch; the server validates them with the
//! same rules and codes (`worktree_protocol::config::as_code`).

use std::path::{Path, PathBuf};

use clap::Subcommand;
use colored::Colorize;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use worktree_protocol::config::as_code::{self, Diagnostic, Severity};
use worktree_protocol::config::branches::check_owner;
use worktree_sdk::remote::rest::path_segs;
use worktree_sdk::{Repository, TreeInfo};

use crate::commands::server_common::{
    decode, default_tree, nullable, render, tree_or_default, Api, TreeRef,
};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::short;
use crate::output::Text;

/// `wt trees config ...`
#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show the configuration the server applied from the tree's .wt-tree/
    /// files (source snapshot, declared branches and protections, warnings)
    /// and the errors of the last rejected push
    Show {
        #[arg(id = "tree_ref", value_name = "TREE")]
        tree: Option<TreeRef>,
    },
    /// Validate .wt-tree/{config,branches,protections}.toml and
    /// .wt-tree/access/** locally (PATH: a tree directory or its .wt-tree/
    /// directory; default: the current tree)
    Validate {
        path: Option<PathBuf>,
        /// Also ask the server (checks owners, iam:admin for access/, and a
        /// dry run of the access document)
        #[arg(long)]
        online: bool,
        /// Server tree to validate against with --online (default: the
        /// current tree on origin)
        #[arg(long = "remote-tree", id = "remote_tree", value_name = "T/NAME")]
        remote_tree: Option<TreeRef>,
    },
}

// ---------------------------------------------------------------------------
// Reading the files

/// The config-as-code files below `dir` (a tree directory), as tree-relative
/// paths and contents, sorted by path.
pub fn local_files(dir: &Path) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for rel in [
        as_code::CONFIG_FILE,
        as_code::BRANCHES_FILE,
        as_code::PROTECTIONS_FILE,
    ] {
        if let Some(text) = read_optional(&dir.join(rel))? {
            out.push((rel.to_string(), text));
        }
    }
    let access = dir.join(as_code::ACCESS_DIR);
    let mut stack = vec![access.clone()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let rel = path
                .strip_prefix(dir)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            if as_code::is_config_path(&rel) {
                if let Some(text) = read_optional(&path)? {
                    out.push((rel, text));
                }
            }
        }
    }
    out.sort();
    Ok(out)
}

fn read_optional(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(CliError::failure(format!(
            "cannot read '{}': {e}",
            path.display()
        ))),
    }
}

/// The config-as-code files of the tip of `branch` (what a push sends).
fn snapshot_files(repo: &Repository, tree: &str, branch: &str) -> Result<Vec<(String, String)>> {
    let Some(tip) = repo.branch_tip(tree, branch)? else {
        return Ok(Vec::new());
    };
    let snap = repo.get_snapshot(tip.as_str())?;
    let manifest = repo.manifest_of(&snap)?;
    let mut out = Vec::new();
    for e in &manifest.entries {
        if as_code::is_config_path(&e.path) {
            let bytes = repo.read_file_at(&snap, &e.path)?;
            out.push((e.path.clone(), String::from_utf8_lossy(&bytes).into_owned()));
        }
    }
    Ok(out)
}

fn validate(files: &[(String, String)]) -> Vec<Diagnostic> {
    as_code::validate_files(files.iter().map(|(p, s)| (p.as_str(), s.as_str())))
}

// ---------------------------------------------------------------------------
// Rendering

/// One line per diagnostic: `file:line:col: CODE message`.
pub fn render_diagnostics(t: &mut Text, list: &[Diagnostic]) {
    for d in list {
        let code = match d.severity {
            Severity::Error => d.code.red().bold(),
            Severity::Warning => d.code.yellow().bold(),
        };
        let mut loc = d.file.clone();
        if d.line > 0 {
            loc.push_str(&format!(":{}:{}", d.line, d.col));
        }
        t.line(format_args!("  {}: {code} {}", loc.bold(), d.message));
    }
}

fn errors(list: &[Diagnostic]) -> usize {
    list.iter().filter(|d| d.is_error()).count()
}

// ---------------------------------------------------------------------------
// wt status / wt push

/// Problems in the working copy of `tree`'s config-as-code files (for
/// `wt status`; best effort: unreadable files are skipped).
pub fn status_problems(repo: &Repository, tree: &TreeInfo) -> Vec<Diagnostic> {
    let Ok(dir) = repo.tree_dir(&tree.name) else {
        return Vec::new();
    };
    local_files(&dir)
        .map(|files| validate(&files))
        .unwrap_or_default()
}

/// Refuse to push `branch` (the tree's default branch) when the
/// config-as-code files of its tip have errors: the server would reject
/// the push anyway. Returns the warnings.
pub fn check_before_push(
    repo: &Repository,
    tree: &TreeInfo,
    branch: &str,
) -> Result<Vec<Diagnostic>> {
    if branch != tree.default_branch {
        return Ok(Vec::new());
    }
    let list = validate(&snapshot_files(repo, &tree.name, branch)?);
    let n = errors(&list);
    if n == 0 {
        return Ok(list);
    }
    let lines: Vec<String> = list
        .iter()
        .filter(|d| d.is_error())
        .map(|d| format!("  {d}"))
        .collect();
    Err(CliError::failure(format!(
        "the .wt-tree configuration of {} has {n} error(s); the server would reject this push:\n{}",
        branch,
        lines.join("\n")
    ))
    .with_hint(
        "fix the files, snapshot them, then push again (`wt trees config validate` checks them)",
    ))
}

// ---------------------------------------------------------------------------
// wt trees config

/// `TreeConfigState` of the server (GET {tree}/config).
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct State {
    default_branch: String,
    applied: Option<Applied>,
    last_attempt: Option<Attempt>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Applied {
    snapshot_id: String,
    applied_by: String,
    applied_at: String,
    #[serde(deserialize_with = "nullable")]
    files: Vec<FileInfo>,
    #[serde(deserialize_with = "nullable")]
    branches: Vec<BranchDecl>,
    #[serde(deserialize_with = "nullable")]
    protections: Vec<Value>,
    #[serde(deserialize_with = "nullable")]
    managed_patterns: Vec<String>,
    access_hash: String,
    #[serde(deserialize_with = "nullable")]
    changes: Vec<String>,
    #[serde(deserialize_with = "nullable")]
    warnings: Vec<ServerDiagnostic>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Attempt {
    snapshot_id: String,
    by: String,
    at: String,
    accepted: bool,
    #[serde(deserialize_with = "nullable")]
    errors: Vec<ServerDiagnostic>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct FileInfo {
    path: String,
    size: u64,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct BranchDecl {
    name: String,
    description: String,
    owner: String,
}

/// A diagnostic as the server encodes it.
#[derive(Debug, Default, Clone, Deserialize, Serialize)]
#[serde(default)]
struct ServerDiagnostic {
    code: String,
    severity: String,
    file: String,
    line: usize,
    col: usize,
    message: String,
}

impl ServerDiagnostic {
    fn to_local(&self) -> Diagnostic {
        let mut d = Diagnostic::new(
            &self.code,
            &self.file,
            as_code::Pos {
                line: self.line,
                col: self.col,
            },
            self.message.clone(),
        );
        if self.severity == "warning" {
            d.severity = Severity::Warning;
        }
        d
    }
}

fn locals(list: &[ServerDiagnostic]) -> Vec<Diagnostic> {
    list.iter().map(ServerDiagnostic::to_local).collect()
}

pub fn run(ctx: &Context, command: ConfigCommand) -> Result<()> {
    match command {
        ConfigCommand::Show { tree } => {
            let api = Api::connect(ctx)?;
            let tree = tree_or_default(ctx, tree)?;
            let value = api.get(&format!("{}/config", tree.api()))?;
            render(&ctx.out, &value, |t, st: State| show(t, &tree, &st))
        }
        ConfigCommand::Validate {
            path,
            online,
            remote_tree,
        } => validate_cmd(ctx, path, online, remote_tree),
    }
}

fn show(t: &mut Text, tree: &TreeRef, st: &State) {
    let Some(applied) = &st.applied else {
        t.line(format_args!(
            "No .wt-tree configuration has been applied to {} (push .wt-tree/branches.toml, protections.toml or access/ to {}).",
            tree.to_string().bold(),
            st.default_branch
        ));
        show_attempt(t, st);
        return;
    };
    t.line(format_args!(
        "Configuration of {} from {} ({}), applied by {} at {}",
        tree.to_string().bold(),
        short(&applied.snapshot_id).yellow(),
        st.default_branch,
        applied.applied_by,
        applied.applied_at
    ));
    if !applied.files.is_empty() {
        t.blank();
        t.line("Files:");
        for f in &applied.files {
            t.line(format_args!("  {} ({} bytes)", f.path, f.size));
        }
    }
    if !applied.branches.is_empty() {
        t.blank();
        t.line("Branches:");
        for b in &applied.branches {
            let mut extra = Vec::new();
            if !b.owner.is_empty() {
                extra.push(format!("owner {}", b.owner));
            }
            if !b.description.is_empty() {
                extra.push(b.description.lines().next().unwrap_or_default().to_string());
            }
            t.line(format_args!("  {}  {}", b.name.green(), extra.join(" — ")));
        }
    }
    if !applied.protections.is_empty() {
        t.blank();
        t.line("Protections:");
        for p in &applied.protections {
            let pattern = p.get("pattern").and_then(Value::as_str).unwrap_or_default();
            let managed = applied.managed_patterns.iter().any(|m| m == pattern);
            let note = if managed {
                ""
            } else {
                " (ignored: managed through the API)"
            };
            t.line(format_args!("  {}{note}", pattern.cyan()));
        }
    }
    if !applied.access_hash.is_empty() {
        t.blank();
        t.line(format_args!(
            "Access document applied ({}).",
            short(&applied.access_hash)
        ));
    }
    if !applied.changes.is_empty() {
        t.blank();
        t.line("Last changes:");
        for c in &applied.changes {
            t.line(format_args!("  {c}"));
        }
    }
    if !applied.warnings.is_empty() {
        t.blank();
        t.line("Warnings:");
        render_diagnostics(t, &locals(&applied.warnings));
    }
    show_attempt(t, st);
}

fn show_attempt(t: &mut Text, st: &State) {
    let Some(a) = &st.last_attempt else { return };
    if a.accepted {
        return;
    }
    t.blank();
    t.line(format_args!(
        "{} the configuration of {} (pushed by {} at {}) was rejected:",
        "Last attempt:".red().bold(),
        short(&a.snapshot_id).yellow(),
        a.by,
        a.at
    ));
    render_diagnostics(t, &locals(&a.errors));
}

/// Resolve PATH (a tree directory or its `.wt-tree/`) or the current tree.
fn validate_dir(ctx: &Context, path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = path {
        let p = if p.is_absolute() {
            p
        } else {
            ctx.cwd().join(p)
        };
        if p.file_name().is_some_and(|n| n == as_code::DIR) {
            return Ok(p.parent().map(Path::to_path_buf).unwrap_or(p));
        }
        if !p.is_dir() {
            return Err(CliError::usage(format!(
                "'{}' is not a directory",
                p.display()
            )));
        }
        return Ok(p);
    }
    let (repo, tree) = ctx.open_tree()?;
    Ok(repo.tree_dir(&tree.name)?)
}

fn validate_cmd(
    ctx: &Context,
    path: Option<PathBuf>,
    online: bool,
    remote_tree: Option<TreeRef>,
) -> Result<()> {
    let dir = validate_dir(ctx, path)?;
    let files = local_files(&dir)?;
    let mut list = validate(&files);
    let mut access_changes: Vec<String> = Vec::new();
    if online && !files.is_empty() {
        let tree = match remote_tree {
            Some(t) => t,
            None => {
                let (repo, info) = ctx.open_tree()?;
                default_tree(&repo, &info)?
            }
        };
        let api = Api::connect(ctx)?;
        let body =
            json!({ "files": files.iter().cloned().collect::<std::collections::BTreeMap<_, _>>() });
        let value = api.post(&format!("{}/config/validate", tree.api()), &body)?;
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Res {
            #[serde(deserialize_with = "nullable")]
            diagnostics: Vec<ServerDiagnostic>,
            #[serde(deserialize_with = "nullable")]
            access_changes: Vec<String>,
        }
        let res: Res = decode(&value)?;
        // The server repeats the local checks; keep the local access
        // diagnostics (semantic E20xx codes) it does not produce itself.
        let mut merged = locals(&res.diagnostics);
        for d in list.drain(..) {
            if !merged
                .iter()
                .any(|m| m.code == d.code && m.file == d.file && m.line == d.line)
            {
                merged.push(d);
            }
        }
        as_code::sort_diagnostics(&mut merged);
        list = merged;
        access_changes = res.access_changes;
    }
    let n = errors(&list);
    let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
    ctx.out.emit(
        &json!({
            "dir": dir, "files": paths, "valid": n == 0, "diagnostics": list,
            "access_changes": access_changes, "online": online,
        }),
        |t| {
            if files.is_empty() {
                t.line(format_args!(
                    "No config-as-code files in '{}' (.wt-tree/{{config,branches,protections}}.toml, .wt-tree/access/).",
                    dir.display()
                ));
                return;
            }
            if n == 0 {
                t.line(format_args!(
                    "The .wt-tree configuration in '{}' is valid ({} file(s){}).",
                    dir.display(),
                    files.len(),
                    if online { ", checked by the server" } else { "" }
                ));
            } else {
                t.line(format_args!(
                    "The .wt-tree configuration in '{}' has {n} error(s):",
                    dir.display()
                ));
            }
            render_diagnostics(t, &list);
            for c in &access_changes {
                t.line(format_args!("  access plan: {c}"));
            }
        },
    )?;
    if n > 0 {
        return Err(CliError::failure(format!(
            "{n} error(s) in the .wt-tree configuration"
        )));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// wt branch describe / owner

/// `PATCH {tree}/branches/{name}` with `body`; renders the branch.
pub fn patch_branch(ctx: &Context, name: &str, body: Value) -> Result<()> {
    let (repo, info) = ctx.open_tree()?;
    let tree = default_tree(&repo, &info)?;
    let api = Api::connect(ctx)?;
    let path = format!("{}/branches/{}", tree.api(), path_segs(name));
    let value = api.patch(&path, &body).map_err(|e| {
        if e.to_string().contains("config_managed") || e.to_string().contains("branches.toml") {
            e.with_hint(format!(
                "{name} is declared in .wt-tree/branches.toml: edit the file and push it to {}",
                info.default_branch
            ))
        } else {
            e
        }
    })?;
    #[derive(Deserialize, Default)]
    #[serde(default)]
    struct Branch {
        name: String,
        description: String,
        owner: String,
    }
    render(&ctx.out, &value, |t, b: Branch| {
        let desc = if b.description.is_empty() {
            "(none)".dimmed().to_string()
        } else {
            b.description.clone()
        };
        let owner = if b.owner.is_empty() {
            "(none)".dimmed().to_string()
        } else {
            b.owner.clone()
        };
        t.line(format_args!(
            "Branch {} of {tree}: description {desc}, owner {owner}",
            b.name.green().bold()
        ));
    })
}

/// Validate an owner argument locally (`""` clears).
pub fn owner_arg(owner: &str) -> Result<()> {
    if owner.is_empty() {
        return Ok(());
    }
    match check_owner(owner) {
        None => Ok(()),
        Some(msg) => Err(CliError::usage(msg).with_hint("use user:NAME or team:SLUG (or --clear)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_local_files() {
        let dir = std::env::temp_dir().join(format!("wt-cac-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".wt-tree/access/sub")).unwrap();
        std::fs::write(
            dir.join(".wt-tree/branches.toml"),
            "[[branch]]\nname = \"a b\"\n",
        )
        .unwrap();
        std::fs::write(dir.join(".wt-tree/ignore"), "x").unwrap();
        std::fs::write(dir.join(".wt-tree/access/policies.toml"), "").unwrap();
        std::fs::write(dir.join(".wt-tree/access/sub/notes.txt"), "x").unwrap();
        let files = local_files(&dir).unwrap();
        let paths: Vec<&str> = files.iter().map(|(p, _)| p.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                ".wt-tree/access/policies.toml",
                ".wt-tree/access/sub/notes.txt",
                ".wt-tree/branches.toml"
            ]
        );
        let list = validate(&files);
        assert_eq!(errors(&list), 1, "{list:?}");
        let mut t = Text::default();
        render_diagnostics(&mut t, &list);
        assert!(t.as_str().contains("E3004"), "{}", t.as_str());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn parses_the_commands() {
        use clap::Parser;
        for args in [
            &[
                "wt", "--server", "http://x", "trees", "config", "validate", "dir", "--online",
            ][..],
            &["wt", "trees", "config", "validate", "--remote-tree", "a/b"],
            &["wt", "--tree", "web", "trees", "config", "show", "a/b"],
            &["wt", "branch", "describe", "main", "text"],
            &["wt", "branch", "owner", "main", "user:x"],
            &["wt", "branch", "owner", "main", "--clear"],
        ] {
            crate::cli::Cli::try_parse_from(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
        }
        assert!(crate::cli::Cli::try_parse_from(["wt", "branch", "owner", "main"]).is_err());
    }

    #[test]
    fn owner_arguments() {
        assert!(owner_arg("").is_ok());
        assert!(owner_arg("team:core").is_ok());
        assert!(owner_arg("core").is_err());
    }
}
