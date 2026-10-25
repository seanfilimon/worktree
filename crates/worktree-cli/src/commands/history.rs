//! `wt log`, `wt show`, `wt reflog` (and `wt reflog expire`)

use std::collections::HashMap;

use clap::{Args, Subcommand};
use colored::{ColoredString, Colorize};
use serde::Serialize;
use worktree_sdk::diff as textdiff;
use worktree_sdk::signing::{self, SignatureStatus};
use worktree_sdk::{
    ChangeKind, Dependency, DiffOptions, FileDiff, ReflogEntry, Repository, Snapshot, SnapshotId,
};

use crate::commands::diff;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{ago, date, plural, short, short_or, subject};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct LogArgs {
    /// Show at most N snapshots
    #[arg(short = 'n', long = "max-count", value_name = "N")]
    pub max_count: Option<usize>,

    /// Start from this revision (default: HEAD)
    #[arg(value_name = "REV")]
    pub rev: Option<String>,

    /// One snapshot per line
    #[arg(long)]
    pub oneline: bool,

    /// Verify and show each snapshot's signature
    #[arg(long)]
    pub show_signature: bool,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// Snapshot to show
    #[arg(value_name = "REV")]
    pub rev: String,

    /// Show a diffstat instead of the patch
    #[arg(long)]
    pub stat: bool,
}

#[derive(Debug, Args)]
#[command(args_conflicts_with_subcommands = true)]
pub struct ReflogArgs {
    #[command(subcommand)]
    pub action: Option<ReflogCommand>,

    /// Show at most N entries
    #[arg(short = 'n', long = "max-count", alias = "limit", value_name = "N")]
    pub max_count: Option<usize>,

    /// Only entries of this branch (numbered as `BRANCH@{N}`)
    #[arg(short, long, value_name = "BRANCH")]
    pub branch: Option<String>,
}

#[derive(Debug, Subcommand)]
pub enum ReflogCommand {
    /// Drop old reflog entries (snapshots are never deleted)
    Expire {
        /// Drop entries older than this (`90d`, `12h`, `30m`, `2w`, `now`)
        #[arg(long, value_name = "AGE", default_value = "90d", value_parser = parse_age)]
        older_than: chrono::Duration,
        /// Keep at most N newest entries per tree
        #[arg(long, value_name = "N", default_value_t = Repository::REFLOG_DEFAULT_KEEP)]
        keep: usize,
        /// Expire the reflogs of every tree, not just the current one
        #[arg(long)]
        all_trees: bool,
    },
}

/// `90d`, `12h`, `30m`, `45s`, `2w`, `0` or `now` -> duration.
fn parse_age(value: &str) -> std::result::Result<chrono::Duration, String> {
    let v = value.trim();
    if v == "now" || v == "0" {
        return Ok(chrono::Duration::zero());
    }
    let split = v.find(|c: char| !c.is_ascii_digit()).unwrap_or(v.len());
    let (digits, unit) = v.split_at(split);
    let bad = || format!("expected an age like 90d, 12h, 30m, 2w or now, got {value:?}");
    let n: i64 = digits.parse().map_err(|_| bad())?;
    let duration = match unit {
        "s" => chrono::Duration::try_seconds(n),
        "m" => chrono::Duration::try_minutes(n),
        "h" => chrono::Duration::try_hours(n),
        "d" | "" => chrono::Duration::try_days(n),
        "w" => chrono::Duration::try_weeks(n),
        _ => None,
    };
    duration.ok_or_else(bad)
}

/// A snapshot with its verified signature (`wt log --show-signature`).
#[derive(Serialize)]
struct SignedSnapshot<'a> {
    #[serde(flatten)]
    snapshot: &'a Snapshot,
    signature_status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    signature_key_id: Option<String>,
}

pub fn log(ctx: &Context, args: LogArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let snapshots = repo.log(&tree.name, args.rev.as_deref(), args.max_count)?;
    let refs = if ctx.out.is_json() {
        RefLabels::default()
    } else {
        RefLabels::load(&repo, &tree.name)?
    };
    let signatures: Vec<Option<SignatureStatus>> = if args.show_signature {
        snapshots
            .iter()
            .map(|s| signing::verify_local(&repo, s).map(Some))
            .collect::<std::result::Result<_, _>>()?
    } else {
        vec![None; snapshots.len()]
    };
    let render = |t: &mut Text| {
        for (i, (snap, sig)) in snapshots.iter().zip(&signatures).enumerate() {
            if args.oneline {
                oneline(t, snap, &refs, sig.as_ref());
            } else {
                if i > 0 {
                    t.blank();
                }
                header(t, snap, &refs, sig.as_ref());
            }
        }
    };
    if args.show_signature {
        let signed: Vec<SignedSnapshot> = snapshots
            .iter()
            .zip(&signatures)
            .map(|(snapshot, sig)| {
                let (status, key) =
                    signature_fields(sig.as_ref().unwrap_or(&SignatureStatus::Unsigned));
                SignedSnapshot {
                    snapshot,
                    signature_status: status.to_string(),
                    signature_key_id: key,
                }
            })
            .collect();
        ctx.out.emit(&signed, render)
    } else {
        ctx.out.emit(&snapshots, render)
    }
}

/// Machine status name and key id of a signature status.
fn signature_fields(status: &SignatureStatus) -> (&'static str, Option<String>) {
    match status {
        SignatureStatus::Unsigned => ("unsigned", None),
        SignatureStatus::Verified { key_id } => ("verified", Some(key_id.clone())),
        SignatureStatus::Invalid(_) => ("invalid", None),
        SignatureStatus::UnknownKey(key) => ("unverified", Some(key.clone())),
    }
}

/// Human description of a signature status.
fn signature_text(status: &SignatureStatus) -> ColoredString {
    match status {
        SignatureStatus::Unsigned => "unsigned".dimmed(),
        SignatureStatus::Verified { key_id } => format!("verified (key {key_id})").green(),
        SignatureStatus::Invalid(reason) => format!("INVALID: {reason}").red().bold(),
        SignatureStatus::UnknownKey(key) => format!("unverified (unknown key {key})").yellow(),
    }
}

#[derive(Serialize)]
struct ShowResult<'a> {
    snapshot: &'a Snapshot,
    files: &'a [FileDiff],
}

pub fn show(ctx: &Context, args: ShowArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let snapshot = repo.resolve(&tree.name, &args.rev)?;
    let opts = DiffOptions::default();
    let files = match snapshot.parents.first() {
        Some(parent) => repo.diff_revs(&tree.name, parent, &snapshot.id, &opts)?,
        None => root_diff(&repo, &snapshot, &opts)?,
    };
    let refs = if ctx.out.is_json() {
        RefLabels::default()
    } else {
        RefLabels::load(&repo, &tree.name)?
    };
    let result = ShowResult {
        snapshot: &snapshot,
        files: &files,
    };
    ctx.out.emit(&result, |t| {
        header(t, &snapshot, &refs, None);
        dependencies(t, &snapshot.dependencies);
        if files.is_empty() {
            return;
        }
        t.blank();
        if args.stat {
            diff::render_stat(t, &files);
        } else {
            diff::render_patches(t, &files);
        }
    })
}

/// `Depends on:` block of `wt show`.
fn dependencies(t: &mut Text, deps: &[Dependency]) {
    if deps.is_empty() {
        return;
    }
    t.blank();
    t.line("Depends on:");
    for dep in deps {
        t.line(format_args!("    {}", dependency_line(dep)));
    }
}

/// `tenant/tree@branch: title [high, blocking, linked]`.
fn dependency_line(dep: &Dependency) -> String {
    let mut target = String::new();
    if !dep.tenant.is_empty() {
        target.push_str(&dep.tenant);
        target.push('/');
    }
    target.push_str(&dep.tree);
    if !dep.branch.is_empty() {
        target.push('@');
        target.push_str(&dep.branch);
    }
    let mut flags = vec![if dep.priority.is_empty() {
        "medium".to_string()
    } else {
        dep.priority.clone()
    }];
    if dep.blocking {
        flags.push("blocking".into());
    }
    if dep.linked {
        flags.push("linked".into());
    }
    let mut line = format!("{target}: {} [{}]", dep.title, flags.join(", "));
    if !dep.details.is_empty() {
        line.push_str(&format!(" - {}", subject(&dep.details)));
    }
    line
}

/// A root snapshot has no parent to diff against: every file is added.
fn root_diff(repo: &Repository, snapshot: &Snapshot, opts: &DiffOptions) -> Result<Vec<FileDiff>> {
    let manifest = repo.manifest_of(snapshot)?;
    let mut files = Vec::with_capacity(manifest.entries.len());
    for entry in &manifest.entries {
        let data = repo.read_file_at(snapshot, &entry.path)?;
        let binary = textdiff::is_binary(&data);
        let (patch, added) = if binary {
            (None, 0)
        } else {
            let text = String::from_utf8_lossy(&data);
            let patch = textdiff::unified_diff("/dev/null", &entry.path, "", &text, opts.context);
            (Some(patch), textdiff::diff_stat("", &text).added)
        };
        files.push(FileDiff {
            path: entry.path.clone(),
            kind: ChangeKind::Added,
            old_hash: None,
            new_hash: Some(entry.hash),
            binary,
            patch,
            added,
            removed: 0,
            old_path: None,
            similarity: None,
        });
    }
    Ok(files)
}

#[derive(Serialize)]
struct ExpireResult {
    trees: Vec<String>,
    removed: usize,
    kept: usize,
}

pub fn reflog(ctx: &Context, args: ReflogArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    if let Some(ReflogCommand::Expire {
        older_than,
        keep,
        all_trees,
    }) = args.action
    {
        let report = if all_trees {
            repo.expire_all_reflogs(older_than, keep)?
        } else {
            repo.expire_reflog(&tree.name, older_than, keep)?
        };
        let result = ExpireResult {
            trees: if all_trees {
                repo.trees()?.into_iter().map(|t| t.name).collect()
            } else {
                vec![tree.name.clone()]
            },
            removed: report.removed,
            kept: report.kept,
        };
        return ctx.out.emit(&result, |t| {
            t.line(format_args!(
                "Expired {}; {} kept.",
                plural(report.removed, "reflog entry", "reflog entries"),
                report.kept
            ))
        });
    }
    if let Some(branch) = &args.branch {
        if !repo
            .branches(&tree.name, true)?
            .iter()
            .any(|b| &b.name == branch)
        {
            return Err(CliError::failure(format!("no such branch: {branch}")));
        }
    }
    let entries = repo.reflog_entries(&tree.name, args.branch.as_deref(), args.max_count)?;
    ctx.out.emit(&entries, |t| {
        for (entry, label) in entries.iter().zip(reflog_labels(&entries)) {
            t.line(format_args!(
                "{} {}: {}: {} ({})",
                short_or(entry.new.as_deref(), "--------").yellow(),
                label.green(),
                entry.action.bold(),
                subject(&entry.message),
                ago(entry.at)
            ));
        }
    })
}

/// `BRANCH@{N}` for each entry (newest first), numbering each branch's
/// entries separately, so every label resolves as a revision.
fn reflog_labels(entries: &[ReflogEntry]) -> Vec<String> {
    let mut seen: HashMap<&str, usize> = HashMap::new();
    entries
        .iter()
        .map(|e| {
            let n = seen.entry(e.branch.as_str()).or_insert(0);
            let label = format!("{}@{{{n}}}", e.branch);
            *n += 1;
            label
        })
        .collect()
}

/// Branch, remote-branch and tag names pointing at each snapshot.
#[derive(Default)]
struct RefLabels {
    labels: HashMap<SnapshotId, Vec<ColoredString>>,
}

impl RefLabels {
    fn load(repo: &Repository, tree: &str) -> Result<Self> {
        let mut labels: HashMap<SnapshotId, Vec<ColoredString>> = HashMap::new();
        for branch in repo.branches(tree, false)? {
            if let Some(tip) = &branch.tip {
                let label = if branch.is_current {
                    format!("{} -> {}", "HEAD".cyan().bold(), branch.name.green().bold()).normal()
                } else {
                    branch.name.green().bold()
                };
                labels.entry(tip.clone()).or_default().push(label);
            }
            if let Some(upstream) = &branch.upstream_tip {
                labels
                    .entry(upstream.clone())
                    .or_default()
                    .push(format!("origin/{}", branch.name).red().bold());
            }
        }
        for tag in repo.tags(tree)? {
            labels
                .entry(tag.target.clone())
                .or_default()
                .push(format!("tag: {}", tag.name).yellow().bold());
        }
        Ok(Self { labels })
    }

    /// ` (HEAD -> main, tag: v1)` or "".
    fn suffix(&self, id: &str) -> String {
        match self.labels.get(id) {
            Some(labels) if !labels.is_empty() => {
                let joined = labels
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(" ({joined})")
            }
            _ => String::new(),
        }
    }
}

fn oneline(t: &mut Text, snap: &Snapshot, refs: &RefLabels, sig: Option<&SignatureStatus>) {
    let auto = if snap.auto_generated {
        " (auto)".dimmed().to_string()
    } else {
        String::new()
    };
    let sig = sig
        .map(|s| format!(" [{}]", signature_text(s)))
        .unwrap_or_default();
    t.line(format_args!(
        "{}{} {}{auto}{sig}",
        snap.short_id().yellow(),
        refs.suffix(&snap.id),
        subject(&snap.message)
    ));
}

fn header(t: &mut Text, snap: &Snapshot, refs: &RefLabels, sig: Option<&SignatureStatus>) {
    t.line(format_args!(
        "{}{}",
        format!("snapshot {}", snap.id).yellow(),
        refs.suffix(&snap.id)
    ));
    if snap.is_merge() {
        let parents = snap
            .parents
            .iter()
            .map(|p| short(p))
            .collect::<Vec<_>>()
            .join(" ");
        t.line(format_args!("Merge:  {parents}"));
    }
    if let Some(sig) = sig {
        t.line(format_args!("Signature: {}", signature_text(sig)));
    }
    t.line(format_args!("Author: {}", snap.author));
    t.line(format_args!(
        "Date:   {} ({})",
        date(snap.created_at),
        ago(snap.created_at)
    ));
    if snap.auto_generated {
        t.line(format_args!("Type:   {}", "auto-snapshot".dimmed()));
    }
    if let Some(reverts) = &snap.reverts {
        t.line(format_args!("Reverts: {}", reverts.yellow()));
    }
    t.blank();
    for line in snap.message.lines() {
        t.line(format_args!("    {line}"));
    }
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn entry(branch: &str) -> ReflogEntry {
        ReflogEntry {
            at: Utc::now(),
            tree: "t".into(),
            branch: branch.into(),
            action: "snapshot".into(),
            old: None,
            new: None,
            message: String::new(),
        }
    }

    #[test]
    fn reflog_labels_number_each_branch() {
        let entries = [entry("main"), entry("feat"), entry("main"), entry("feat")];
        assert_eq!(
            reflog_labels(&entries),
            ["main@{0}", "feat@{0}", "main@{1}", "feat@{1}"]
        );
    }

    #[test]
    fn ages() {
        assert_eq!(parse_age("90d").unwrap(), chrono::Duration::days(90));
        assert_eq!(parse_age("12h").unwrap(), chrono::Duration::hours(12));
        assert_eq!(parse_age("30m").unwrap(), chrono::Duration::minutes(30));
        assert_eq!(parse_age("2w").unwrap(), chrono::Duration::weeks(2));
        assert_eq!(parse_age("7").unwrap(), chrono::Duration::days(7));
        assert_eq!(parse_age("now").unwrap(), chrono::Duration::zero());
        assert!(parse_age("soon").is_err());
        assert!(parse_age("5y").is_err());
    }

    #[test]
    fn dependency_lines() {
        let dep = Dependency {
            tenant: "acme".into(),
            tree: "api".into(),
            branch: "main".into(),
            title: "add endpoint".into(),
            details: "needed by web\nmore".into(),
            priority: "high".into(),
            blocking: true,
            linked: false,
        };
        assert_eq!(
            dependency_line(&dep),
            "acme/api@main: add endpoint [high, blocking] - needed by web"
        );
        let plain = Dependency {
            tree: "docs".into(),
            title: "t".into(),
            ..Dependency::default()
        };
        assert_eq!(dependency_line(&plain), "docs: t [medium]");
    }

    #[test]
    fn signature_rendering() {
        colored::control::set_override(false);
        assert_eq!(
            signature_text(&SignatureStatus::Unsigned).to_string(),
            "unsigned"
        );
        assert_eq!(
            signature_text(&SignatureStatus::UnknownKey("k1".into())).to_string(),
            "unverified (unknown key k1)"
        );
        assert_eq!(
            signature_fields(&SignatureStatus::Verified { key_id: "k".into() }),
            ("verified", Some("k".to_string()))
        );
        assert_eq!(
            signature_fields(&SignatureStatus::Invalid("bad".into())).0,
            "invalid"
        );
    }

    #[test]
    fn header_shows_reverts_and_signature() {
        colored::control::set_override(false);
        let snap = Snapshot {
            id: "0123456789abcdef".into(),
            tree_id: String::new(),
            branch: "main".into(),
            parents: vec!["p".into()],
            manifest: worktree_sdk::store::hash_bytes(b""),
            author: "A".into(),
            message: "Revert x".into(),
            created_at: Utc::now(),
            auto_generated: false,
            dependencies: Vec::new(),
            reverts: Some("fedcba9876543210".into()),
            signature: None,
        };
        let mut t = Text::default();
        header(
            &mut t,
            &snap,
            &RefLabels::default(),
            Some(&SignatureStatus::Unsigned),
        );
        let out = t.as_str();
        assert!(out.contains("Signature: unsigned\n"), "{out}");
        assert!(out.contains("Reverts: fedcba9876543210\n"), "{out}");
    }
}
