//! `wt mr conflicts` and `wt mr resolve` (v0.8 conflict-ui, ENGINEERING_PLAN
//! §16.9): inspect the conflicts of a merge request on the server and write
//! a merge-resolution snapshot on its source branch from resolved local
//! files or `--take ours|theirs|base PATH`.
//!
//! The server looks at the merge the way `wt merge <target>` on the source
//! branch would: "ours" is the source branch, "theirs" the target branch.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use clap::Args;
use colored::Colorize;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::commands::server_common::{decode, nullable, render, tree_or_default, Api, TreeRef};
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{plural, short};
use crate::output::Text;

/// `wt mr conflicts N [TREE] [--markers] [--write [--dir DIR]]`.
#[derive(Debug, Args)]
pub struct ConflictsArgs {
    pub number: u32,
    #[arg(id = "tree_ref", value_name = "TREE")]
    pub tree: Option<TreeRef>,
    /// Also print the diff3 marker preview of every text conflict
    #[arg(long)]
    pub markers: bool,
    /// Write the marker preview of every text conflict into the local tree
    /// directory (or --dir) to edit it, then run `wt mr resolve N`
    #[arg(long)]
    pub write: bool,
    /// Directory --write writes into (default: the local tree directory)
    #[arg(long = "dir", id = "files_dir", value_name = "DIR", requires = "write")]
    pub files_dir: Option<PathBuf>,
}

/// `wt mr resolve N [TREE] [--take SIDE PATH]... [--delete PATH]... [--dir DIR]`.
#[derive(Debug, Args)]
pub struct ResolveArgs {
    pub number: u32,
    #[arg(id = "tree_ref", value_name = "TREE")]
    pub tree: Option<TreeRef>,
    /// Resolve PATH with one version: ours (the source branch), theirs (the
    /// target branch) or base (the merge base); repeatable
    #[arg(long, num_args = 2, value_names = ["SIDE", "PATH"], action = clap::ArgAction::Append)]
    pub take: Vec<String>,
    /// Resolve PATH by deleting it; repeatable
    #[arg(long = "delete", value_name = "PATH")]
    pub delete: Vec<String>,
    /// Read the resolved files from DIR (default: the local tree directory)
    #[arg(long = "dir", id = "files_dir", value_name = "DIR")]
    pub files_dir: Option<PathBuf>,
    /// Message of the resolution snapshot (default: "Merge branch 'TGT'
    /// into SRC (!N, conflicts resolved)")
    #[arg(short, long, value_name = "MSG")]
    pub message: Option<String>,
    /// Accept resolved files that still contain conflict markers
    #[arg(long)]
    pub allow_markers: bool,
}

/// `review.ConflictReport`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ConflictReport {
    number: u32,
    source_branch: String,
    target_branch: String,
    source_tip: String,
    target_tip: String,
    merge_base: String,
    resolvable: bool,
    reason: String,
    total: usize,
    truncated: bool,
    #[serde(deserialize_with = "nullable")]
    files: Vec<ConflictFile>,
}

/// `review.ConflictFile`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct ConflictFile {
    path: String,
    kind: String,
    binary: bool,
    too_large: bool,
    restricted: bool,
    base: Side,
    ours: Side,
    theirs: Side,
    markers: String,
}

/// `review.ConflictSide`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub(crate) struct Side {
    exists: bool,
    text: String,
}

/// `review.Resolved`.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct Resolved {
    snapshot: ResolvedSnapshot,
    #[serde(deserialize_with = "nullable")]
    paths: Vec<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct ResolvedSnapshot {
    id: String,
    branch_name: String,
    #[serde(deserialize_with = "nullable")]
    parents: Vec<String>,
}

fn conflicts_path(tree: &TreeRef, number: u32) -> String {
    format!("{}/merge-requests/{number}/conflicts", tree.api())
}

fn resolve_path(tree: &TreeRef, number: u32) -> String {
    format!("{}/merge-requests/{number}/resolve", tree.api())
}

/// The directory resolved files are read from / written to.
fn local_dir(ctx: &Context, dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = dir {
        return Ok(ctx.absolute(dir));
    }
    let (repo, tree) = ctx.open_tree().map_err(|e| {
        e.with_hint("run inside the worktree of the merge request's tree, or pass --dir DIR")
    })?;
    Ok(repo.tree_dir(&tree.name)?)
}

/// `wt mr conflicts`.
pub fn conflicts(ctx: &Context, args: ConflictsArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let tree = tree_or_default(ctx, args.tree)?;
    let value = api.get(&conflicts_path(&tree, args.number))?;
    if !args.write {
        let markers = args.markers;
        return render(&ctx.out, &value, |t, r: ConflictReport| {
            report(t, &r, markers)
        });
    }
    let r: ConflictReport = decode(&value)?;
    let dir = local_dir(ctx, args.files_dir.as_deref())?;
    let written = write_markers(&dir, &r)?;
    let doc = json!({ "conflicts": value, "written": written });
    ctx.out.emit(&doc, |t| {
        report(t, &r, args.markers);
        t.blank();
        if written.is_empty() {
            t.line("No text conflicts to write.");
        } else {
            t.line(format_args!(
                "Wrote {} with conflict markers to {}",
                plural(written.len(), "file", "files"),
                dir.display()
            ));
            for path in &written {
                t.line(format_args!("  {}", path.cyan()));
            }
            t.line(format_args!(
                "Edit them, then run `wt mr resolve {}`",
                r.number
            ));
        }
    })
}

/// Writes the marker preview of each text conflict under dir; returns the
/// written paths.
fn write_markers(dir: &Path, r: &ConflictReport) -> Result<Vec<String>> {
    let mut written = Vec::new();
    for f in r.files.iter().filter(|f| !f.markers.is_empty()) {
        let dest = safe_join(dir, &f.path)?;
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                CliError::failure(format!("cannot create {}: {e}", parent.display()))
            })?;
        }
        fs::write(&dest, &f.markers)
            .map_err(|e| CliError::failure(format!("cannot write {}: {e}", dest.display())))?;
        written.push(f.path.clone());
    }
    Ok(written)
}

/// Joins a server path (slash-separated, relative) onto dir, refusing
/// absolute paths and `..` components.
fn safe_join(dir: &Path, path: &str) -> Result<PathBuf> {
    let mut out = dir.to_path_buf();
    for part in path.split('/') {
        if part.is_empty()
            || part == "."
            || part == ".."
            || part.contains('\\')
            || part.contains(':')
        {
            return Err(CliError::failure(format!(
                "refusing the unsafe path '{path}' from the server"
            )));
        }
        out.push(part);
    }
    Ok(out)
}

/// `wt mr resolve`.
pub fn resolve(ctx: &Context, args: ResolveArgs) -> Result<()> {
    let api = Api::connect(ctx)?;
    let tree = tree_or_default(ctx, args.tree)?;
    let number = args.number;
    let r: ConflictReport = decode(&api.get(&conflicts_path(&tree, number))?)?;
    if !r.resolvable {
        return Err(CliError::failure(format!(
            "merge request !{number} cannot be resolved on the server: {}",
            r.reason
        )));
    }
    if r.truncated {
        return Err(CliError::failure(format!(
            "merge request !{number} has {} conflicting paths, more than the server lists",
            r.total
        ))
        .with_hint(format!(
            "merge '{}' into '{}' locally (`wt merge`), resolve and push",
            r.target_branch, r.source_branch
        )));
    }
    let takes = parse_takes(&args.take)?;
    let deletes: BTreeSet<String> = args.delete.iter().cloned().collect();
    let conflicting: BTreeSet<&str> = r.files.iter().map(|f| f.path.as_str()).collect();
    let unknown: Vec<&str> = takes
        .keys()
        .chain(deletes.iter())
        .map(String::as_str)
        .filter(|p| !conflicting.contains(p))
        .collect();
    if !unknown.is_empty() {
        return Err(CliError::usage(format!(
            "not conflicting in !{number}: {}",
            unknown.join(", ")
        ))
        .with_hint(format!(
            "`wt mr conflicts {number}` lists the conflicting paths"
        )));
    }
    if let Some(p) = takes.keys().find(|p| deletes.contains(*p)) {
        return Err(CliError::usage(format!(
            "{p} is given to both --take and --delete"
        )));
    }

    let needs_local = r
        .files
        .iter()
        .any(|f| !takes.contains_key(&f.path) && !deletes.contains(&f.path));
    let dir = if needs_local {
        Some(local_dir(ctx, args.files_dir.as_deref())?)
    } else {
        None
    };
    let files = resolutions(&r, &takes, &deletes, dir.as_deref(), args.allow_markers)?;

    let mut body = json!({
        "files": files,
        "source_tip": r.source_tip,
        "target_tip": r.target_tip,
    });
    if let Some(message) = &args.message {
        body["message"] = json!(message);
    }
    if args.allow_markers {
        body["allow_markers"] = json!(true);
    }
    let value = api
        .post(&resolve_path(&tree, number), &body)
        .map_err(|e| resolve_error(e, number))?;
    render(&ctx.out, &value, |t, res: Resolved| {
        t.line(format_args!(
            "Resolved {} of {} on {} as {}",
            plural(res.paths.len(), "conflict", "conflicts"),
            format!("!{number}").bold(),
            res.snapshot.branch_name.green(),
            short(&res.snapshot.id).yellow()
        ));
        if res.snapshot.parents.len() == 2 {
            t.line(format_args!(
                "  parents: {} ({}) and {} ({})",
                short(&res.snapshot.parents[0]),
                r.source_branch,
                short(&res.snapshot.parents[1]),
                r.target_branch
            ));
        }
        t.line(format_args!(
            "Merge it with `wt mr merge {number}`; `wt pull` brings the resolution into this worktree"
        ));
    })
}

/// Parses `--take SIDE PATH` pairs into PATH → SIDE.
fn parse_takes(raw: &[String]) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for pair in raw.chunks(2) {
        let [side, path] = pair else {
            return Err(CliError::usage("--take needs a SIDE and a PATH"));
        };
        let side = match side.as_str() {
            "ours" | "source" => "ours",
            "theirs" | "target" => "theirs",
            "base" => "base",
            other => {
                return Err(CliError::usage(format!(
                    "--take {other}: SIDE must be ours (the source branch), theirs (the target branch) or base"
                )))
            }
        };
        let path = path.replace('\\', "/");
        if out.insert(path.clone(), side.to_string()).is_some() {
            return Err(CliError::usage(format!("{path} is given to --take twice")));
        }
    }
    Ok(out)
}

/// The `files` of the resolve request: --take / --delete, else the file
/// from dir.
fn resolutions(
    r: &ConflictReport,
    takes: &BTreeMap<String, String>,
    deletes: &BTreeSet<String>,
    dir: Option<&Path>,
    allow_markers: bool,
) -> Result<Vec<Value>> {
    let mut files = Vec::with_capacity(r.files.len());
    let mut missing = Vec::new();
    let mut marked = Vec::new();
    for f in &r.files {
        if let Some(side) = takes.get(&f.path) {
            files.push(json!({ "path": f.path, "take": side }));
            continue;
        }
        if deletes.contains(&f.path) {
            files.push(json!({ "path": f.path, "delete": true }));
            continue;
        }
        let Some(dir) = dir else {
            missing.push(f.path.clone());
            continue;
        };
        let local = safe_join(dir, &f.path)?;
        let data = match fs::read(&local) {
            Ok(data) => data,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(f.path.clone());
                continue;
            }
            Err(e) => {
                return Err(CliError::failure(format!(
                    "cannot read {}: {e}",
                    local.display()
                )))
            }
        };
        let Ok(text) = String::from_utf8(data) else {
            return Err(CliError::usage(format!(
                "{} is not UTF-8 text: resolve it with --take ours|theirs {} or --delete {}",
                f.path, f.path, f.path
            )));
        };
        if !allow_markers && has_markers(&text) {
            marked.push(f.path.clone());
        }
        files.push(json!({ "path": f.path, "content": text }));
    }
    if !missing.is_empty() {
        return Err(CliError::usage(format!(
            "no resolved file for {}",
            missing.join(", ")
        ))
        .with_hint(format!(
            "write the resolved files (`wt mr conflicts {} --write`), or pass --take ours|theirs PATH / --delete PATH",
            r.number
        )));
    }
    if !marked.is_empty() {
        return Err(CliError::conflicts(format!(
            "{} still contain conflict markers",
            marked.join(", ")
        ))
        .with_hint("finish editing them, or pass --allow-markers to keep the markers"));
    }
    Ok(files)
}

/// Whether text holds a conflict block (`<<<<<<<`, `=======`, `>>>>>>>`).
fn has_markers(text: &str) -> bool {
    let mut state = 0;
    for line in text.lines() {
        match state {
            0 if line.starts_with("<<<<<<<") => state = 1,
            1 if line == "=======" => state = 2,
            2 if line.starts_with(">>>>>>>") => return true,
            _ => {}
        }
    }
    false
}

/// Adds hints to refusals of `wt mr resolve`.
fn resolve_error(err: CliError, number: u32) -> CliError {
    let text = err.to_string();
    if text.contains("resolve_outdated") {
        err.with_hint(format!(
            "a branch moved since the conflicts were loaded: run `wt mr conflicts {number}` and resolve again"
        ))
    } else if text.contains("branch_protection") {
        err.with_hint("the source branch does not accept direct pushes from you: merge the target into it locally and open the change through its owners")
    } else if text.contains("quota_exceeded") {
        err.with_hint("the tenant's storage quota is exhausted")
    } else {
        err
    }
}

fn report(t: &mut Text, r: &ConflictReport, markers: bool) {
    let head = format!("!{}", r.number);
    if r.total == 0 {
        t.line(format_args!(
            "{} has no conflicts ({} -> {}){}",
            head.bold().green(),
            r.source_branch,
            r.target_branch,
            if r.reason.is_empty() {
                String::new()
            } else {
                format!(": {}", r.reason)
            }
        ));
        return;
    }
    t.line(format_args!(
        "{} has {} merging {} into {}",
        head.bold().red(),
        plural(r.total, "conflict", "conflicts"),
        r.target_branch.green(),
        r.source_branch.green()
    ));
    t.line(format_args!(
        "  source {}, target {}{}",
        short(&r.source_tip).yellow(),
        short(&r.target_tip).yellow(),
        if r.merge_base.is_empty() {
            String::new()
        } else {
            format!(", base {}", short(&r.merge_base).yellow())
        }
    ));
    for f in &r.files {
        let mut notes = Vec::new();
        if f.binary {
            notes.push("binary".to_string());
        }
        if f.too_large {
            notes.push("too large to show".to_string());
        }
        if f.restricted {
            notes.push("restricted".to_string());
        }
        if !f.ours.exists && f.theirs.exists {
            notes.push(format!("deleted on {}", r.source_branch));
        }
        if f.ours.exists && !f.theirs.exists {
            notes.push(format!("deleted on {}", r.target_branch));
        }
        if !f.base.exists && f.kind != "file_directory" {
            notes.push("added on both".to_string());
        }
        let notes = if notes.is_empty() {
            String::new()
        } else {
            format!(" [{}]", notes.join(", "))
        };
        t.line(format_args!(
            "  {} {} ({}){notes}",
            "x".red(),
            f.path,
            f.kind
        ));
        if markers && !f.markers.is_empty() {
            for line in f.markers.lines() {
                t.line(format_args!("      {line}"));
            }
        } else if markers && f.ours.text != f.theirs.text && f.markers.is_empty() {
            t.line("      (no text preview)".dimmed());
        }
    }
    if r.truncated {
        t.line(format_args!(
            "  ... and {} more",
            r.total.saturating_sub(r.files.len())
        ));
    }
    if r.resolvable {
        t.line(format_args!(
            "Resolve: `wt mr conflicts {n} --write`, edit the files, `wt mr resolve {n}`; or `wt mr resolve {n} --take ours|theirs PATH`; or in the admin panel",
            n = r.number
        ));
    } else if !r.reason.is_empty() {
        t.line(format_args!("  {}", r.reason.dimmed()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ConflictReport {
        serde_json::from_value(json!({
            "number": 3, "state": "open", "source_branch": "feature", "target_branch": "main",
            "source_tip": "aaaaaaaaaaaaaaaa", "target_tip": "bbbbbbbbbbbbbbbb", "merge_base": "cccccccccccccccc",
            "resolvable": true, "total": 3, "truncated": false,
            "files": [
                {"path": "a.txt", "kind": "content", "binary": false, "too_large": false, "restricted": false,
                 "base": {"exists": true, "text": "1\n"}, "ours": {"exists": true, "text": "2\n"}, "theirs": {"exists": true, "text": "3\n"},
                 "markers": "<<<<<<< feature\n2\n||||||| base\n1\n=======\n3\n>>>>>>> main\n", "regions": null},
                {"path": "bin.dat", "kind": "binary", "binary": true, "base": {"exists": true}, "ours": {"exists": true}, "theirs": {"exists": true}},
                {"path": "dir/gone.txt", "kind": "modify_delete", "base": {"exists": true, "text": "x\n"}, "ours": {"exists": false}, "theirs": {"exists": true, "text": "y\n"}}
            ]
        }))
        .expect("report")
    }

    #[test]
    fn renders_conflict_reports() {
        colored::control::set_override(false);
        let mut t = Text::default();
        report(&mut t, &sample(), true);
        let s = t.as_str();
        assert!(
            s.contains("!3 has 3 conflicts merging main into feature"),
            "{s}"
        );
        assert!(s.contains("x bin.dat (binary) [binary]"), "{s}");
        assert!(
            s.contains("dir/gone.txt (modify_delete) [deleted on feature]"),
            "{s}"
        );
        assert!(s.contains("<<<<<<< feature"), "{s}");
        assert!(s.contains("wt mr resolve 3"), "{s}");
        let mut t = Text::default();
        let none: ConflictReport = serde_json::from_value(json!({
            "number": 4, "source_branch": "f", "target_branch": "main", "total": 0,
            "reason": "merge request !4 has no conflicts", "files": null
        }))
        .expect("report");
        report(&mut t, &none, false);
        assert!(t.as_str().contains("!4 has no conflicts"), "{}", t.as_str());
    }

    #[test]
    fn builds_resolutions() {
        let r = sample();
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.txt"), "resolved\n").expect("write");
        let takes = parse_takes(&["theirs".into(), "bin.dat".into()]).expect("takes");
        let deletes: BTreeSet<String> = ["dir/gone.txt".to_string()].into();
        let files = resolutions(&r, &takes, &deletes, Some(dir.path()), false).expect("files");
        assert_eq!(
            Value::Array(files),
            json!([
                {"path": "a.txt", "content": "resolved\n"},
                {"path": "bin.dat", "take": "theirs"},
                {"path": "dir/gone.txt", "delete": true}
            ])
        );
        // Missing files and leftover markers are refused.
        let err = resolutions(&r, &takes, &BTreeSet::new(), Some(dir.path()), false)
            .expect_err("missing");
        assert!(err.to_string().contains("dir/gone.txt"), "{err}");
        fs::write(dir.path().join("a.txt"), &r.files[0].markers).expect("write");
        let err = resolutions(&r, &takes, &deletes, Some(dir.path()), false).expect_err("markers");
        assert_eq!(err.exit_code(), 3);
        assert!(resolutions(&r, &takes, &deletes, Some(dir.path()), true).is_ok());
        // Writing the markers creates the files.
        let out = tempfile::tempdir().expect("tempdir");
        assert_eq!(
            write_markers(out.path(), &r).expect("write"),
            vec!["a.txt".to_string()]
        );
        assert_eq!(
            fs::read_to_string(out.path().join("a.txt")).expect("read"),
            r.files[0].markers
        );
    }

    #[test]
    fn parses_takes_and_paths() {
        let t = parse_takes(&["ours".into(), "a".into(), "target".into(), "b\\c".into()])
            .expect("takes");
        assert_eq!(t.get("a").map(String::as_str), Some("ours"));
        assert_eq!(t.get("b/c").map(String::as_str), Some("theirs"));
        assert!(parse_takes(&["mine".into(), "a".into()]).is_err());
        assert!(parse_takes(&["ours".into(), "a".into(), "base".into(), "a".into()]).is_err());
        let dir = Path::new("/w");
        assert!(safe_join(dir, "../x").is_err());
        assert!(safe_join(dir, "/x").is_err());
        assert!(safe_join(dir, "a/./b").is_err());
        assert!(safe_join(dir, "a/b.txt").is_ok());
        assert!(has_markers("<<<<<<< a\nx\n=======\ny\n>>>>>>> b\n"));
        assert!(!has_markers("======= alone\n"));
        assert_eq!(
            resolve_error(CliError::failure("moved (resolve_outdated, HTTP 409)"), 3).exit_code(),
            1
        );
        assert!(format!(
            "{:?}",
            resolve_error(CliError::failure("x (resolve_outdated, HTTP 409)"), 3)
        )
        .contains("wt mr conflicts 3"));
    }
}
