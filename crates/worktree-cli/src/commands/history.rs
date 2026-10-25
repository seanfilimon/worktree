//! `wt log`, `wt show`, `wt reflog`

use std::collections::HashMap;

use clap::Args;
use colored::{ColoredString, Colorize};
use serde::Serialize;
use worktree_sdk::diff as textdiff;
use worktree_sdk::{ChangeKind, DiffOptions, FileDiff, Repository, Snapshot, SnapshotId};

use crate::commands::diff;
use crate::context::Context;
use crate::error::Result;
use crate::output::format::{ago, date, short, short_or, subject};
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
pub struct ReflogArgs {
    /// Show at most N entries
    #[arg(short = 'n', long = "max-count", value_name = "N")]
    pub max_count: Option<usize>,
}

pub fn log(ctx: &Context, args: LogArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let snapshots = repo.log(&tree.name, args.rev.as_deref(), args.max_count)?;
    let refs = if ctx.out.is_json() {
        RefLabels::default()
    } else {
        RefLabels::load(&repo, &tree.name)?
    };
    // v0.5 chunks workstream (§13.2): history of a shallow copy ends at
    // its boundary snapshots.
    let boundary = repo.shallow_boundary(&tree.name)?;
    ctx.out.emit(&snapshots, |t| {
        for (i, snap) in snapshots.iter().enumerate() {
            if args.oneline {
                oneline(t, snap, &refs);
            } else {
                if i > 0 {
                    t.blank();
                }
                header(t, snap, &refs);
            }
            if boundary.contains(&snap.id) {
                t.line(format_args!(
                    "{}",
                    "(shallow boundary: older history not fetched; `wt fetch --deepen N`)".dimmed()
                ));
            }
        }
    })
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
        header(t, &snapshot, &refs);
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
        });
    }
    Ok(files)
}

pub fn reflog(ctx: &Context, args: ReflogArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let entries = repo.reflog(&tree.name, args.max_count)?;
    ctx.out.emit(&entries, |t| {
        for (i, e) in entries.iter().enumerate() {
            t.line(format_args!(
                "{} {}: {}: {} ({})",
                short_or(e.new.as_deref(), "--------").yellow(),
                format!("{}@{{{i}}}", e.branch).green(),
                e.action.bold(),
                subject(&e.message),
                ago(e.at)
            ));
        }
    })
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

fn oneline(t: &mut Text, snap: &Snapshot, refs: &RefLabels) {
    let auto = if snap.auto_generated {
        " (auto)".dimmed().to_string()
    } else {
        String::new()
    };
    t.line(format_args!(
        "{}{} {}{auto}",
        snap.short_id().yellow(),
        refs.suffix(&snap.id),
        subject(&snap.message)
    ));
}

fn header(t: &mut Text, snap: &Snapshot, refs: &RefLabels) {
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
    t.line(format_args!("Author: {}", snap.author));
    t.line(format_args!(
        "Date:   {} ({})",
        date(snap.created_at),
        ago(snap.created_at)
    ));
    if snap.auto_generated {
        t.line(format_args!("Type:   {}", "auto-snapshot".dimmed()));
    }
    t.blank();
    for line in snap.message.lines() {
        t.line(format_args!("    {line}"));
    }
}
