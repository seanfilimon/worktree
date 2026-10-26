//! staged-v2 (ENGINEERING_PLAN §16.8): `wt stage list`, `wt stage show`,
//! `wt checkout --staged USER[/LABEL]` and the push-time advisory printed
//! by `wt push`.

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::remote::{self, StagedHistoryEntry, StagedInfo, StagedOverlap};
use worktree_sdk::Repository;

use crate::commands::remote::{normalize_server, remote_name, remote_url};
use crate::context::{block_on, Context};
use crate::error::{CliError, Result};
use crate::output::format::{ago, plural, short, subject};
use crate::output::Text;

#[derive(Debug, Subcommand)]
pub enum StageCommand {
    /// List the team's staged snapshots (every user, branch and label)
    List(StageListArgs),
    /// Show one of your staged snapshots (or another user's with --user)
    Show(StageShowArgs),
}

#[derive(Debug, Args)]
pub struct StageListArgs {
    /// Remote to read from (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Only this branch (default: every branch)
    #[arg(long, value_name = "B")]
    pub branch: Option<String>,

    /// Only your own staged snapshots
    #[arg(long)]
    pub mine: bool,

    /// Also list ended entries of the last 30 days (pushed, cleared, expired)
    #[arg(long)]
    pub history: bool,
}

#[derive(Debug, Args)]
pub struct StageShowArgs {
    /// Label of the slot (default: the default slot)
    #[arg(value_name = "LABEL")]
    pub label: Option<String>,

    /// Remote to read from (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Branch the snapshot is staged on (default: the current branch)
    #[arg(long, value_name = "B")]
    pub branch: Option<String>,

    /// Show this user's staged snapshot instead of yours
    #[arg(long, value_name = "USER")]
    pub user: Option<String>,

    /// Also list every file of the staged working state (not only the changes)
    #[arg(long)]
    pub files: bool,
}

#[derive(Debug, Args)]
pub struct CheckoutArgs {
    /// Check out a teammate's staged work: USER or USER/LABEL
    #[arg(long, value_name = "USER[/LABEL]", required = true)]
    pub staged: String,

    /// Name of the new local branch (default: staged/USER[/LABEL])
    #[arg(short = 'b', long = "name", value_name = "NAME")]
    pub name: Option<String>,

    /// Branch the work is staged on (default: wherever it is staged, if unique)
    #[arg(long, value_name = "B")]
    pub branch: Option<String>,

    /// Remote to read from (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Discard uncommitted local changes when switching
    #[arg(long)]
    pub force: bool,
}

/// `wt stage list|show`.
pub fn run(ctx: &Context, command: StageCommand) -> Result<()> {
    match command {
        StageCommand::List(args) => list(ctx, args),
        StageCommand::Show(args) => show(ctx, args),
    }
}

/// Username of the saved login for `remote`, if any.
fn my_username(repo: &Repository, remote: &str) -> Option<String> {
    let url = remote_url(repo, remote).ok()?;
    remote::logged_in_username(&normalize_server(&url))
}

/// "label" or "(default)".
fn slot_name(label: &str) -> String {
    if label.is_empty() {
        "(default)".to_string()
    } else {
        label.to_string()
    }
}

#[derive(Serialize)]
struct ListResult<'a> {
    remote: &'a str,
    staged: &'a [StagedInfo],
    #[serde(skip_serializing_if = "Option::is_none")]
    history: Option<&'a [StagedHistoryEntry]>,
}

fn list(ctx: &Context, args: StageListArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let mut staged = block_on(remote::list_staged(
        &repo,
        &tree.name,
        &remote,
        args.branch.as_deref(),
    ))??;
    if args.mine {
        let me = my_username(&repo, &remote).ok_or_else(|| {
            CliError::usage("--mine needs a login").with_hint("run `wt login <server>`")
        })?;
        staged.retain(|s| s.author == me);
    }
    staged.sort_by(|a, b| (&a.author, &a.branch, &a.label).cmp(&(&b.author, &b.branch, &b.label)));
    let history = if args.history {
        Some(block_on(remote::staged_history(
            &repo,
            &tree.name,
            &remote,
            args.branch.as_deref(),
            None,
        ))??)
    } else {
        None
    };
    let result = ListResult {
        remote: &remote,
        staged: &staged,
        history: history.as_deref(),
    };
    ctx.out.emit(&result, |t| {
        render_list(t, &remote, &staged);
        if let Some(h) = &history {
            t.blank();
            render_history(t, h);
        }
    })
}

fn render_list(t: &mut Text, remote: &str, staged: &[StagedInfo]) {
    if staged.is_empty() {
        t.line(format_args!("No staged work on {remote}."));
        return;
    }
    t.line(format_args!("Staged work on {remote}:"));
    let mut current_author: Option<&str> = None;
    for s in staged {
        if current_author != Some(s.author.as_str()) {
            t.line(format_args!("  {}", s.author.cyan().bold()));
            current_author = Some(&s.author);
        }
        let message = s.message.as_deref().map(subject).unwrap_or_default();
        t.line(format_args!(
            "    {:<16} {:<20} {:<9} {} +{} ~{} -{}  {}  {}",
            slot_name(&s.label),
            s.branch.green(),
            plural(s.files_changed.len(), "file", "files"),
            short(&s.snapshot_id).yellow(),
            s.files_added,
            s.files_modified,
            s.files_deleted,
            ago(s.staged_at),
            message
        ));
    }
}

fn render_history(t: &mut Text, history: &[StagedHistoryEntry]) {
    if history.is_empty() {
        t.line("No staged history in the last 30 days.");
        return;
    }
    t.line("Staged history (last 30 days):");
    for h in history {
        t.line(format_args!(
            "  {:<8} {} {:<16} {:<20} {}  by {}",
            h.status,
            h.author.cyan(),
            slot_name(&h.label),
            h.branch.green(),
            ago(h.ended_at),
            h.ended_by
        ));
    }
}

#[derive(Serialize)]
struct ShowResult<'a> {
    staged: &'a StagedInfo,
    #[serde(skip_serializing_if = "Option::is_none")]
    files: Option<&'a remote::StagedFiles>,
}

fn show(ctx: &Context, args: StageShowArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let label = args.label.unwrap_or_default();
    remote::validate_label(&label)?;
    let branch = match args.branch {
        Some(b) => b,
        None => repo.current_branch(&tree.name)?,
    };
    let user = match args.user {
        Some(u) => u,
        None => my_username(&repo, &remote).ok_or_else(|| {
            CliError::usage("not logged in: pass --user USER or run `wt login <server>`")
        })?,
    };
    let staged = block_on(remote::list_staged(
        &repo,
        &tree.name,
        &remote,
        Some(&branch),
    ))??;
    let entry = staged
        .into_iter()
        .find(|s| s.author == user && s.label == label)
        .ok_or_else(|| {
            CliError::usage(format!(
                "{user} has nothing staged on {branch} in slot {}",
                slot_name(&label)
            ))
            .with_hint("`wt stage list` shows the team's staged snapshots")
        })?;
    let files = if args.files {
        Some(block_on(remote::staged_files(
            &repo,
            &tree.name,
            &remote,
            &entry.snapshot_id,
        ))??)
    } else {
        None
    };
    let result = ShowResult {
        staged: &entry,
        files: files.as_ref(),
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Staged by {} on {} (slot {}) {}",
            entry.author.cyan(),
            entry.branch.green(),
            slot_name(&entry.label),
            ago(entry.staged_at)
        ));
        t.line(format_args!("Snapshot {}", entry.snapshot_id.yellow()));
        if let Some(m) = &entry.message {
            t.line(format_args!("Message  {}", subject(m)));
        }
        t.line(format_args!(
            "Changes  {} added, {} modified, {} deleted",
            entry.files_added, entry.files_modified, entry.files_deleted
        ));
        for f in &entry.files_changed {
            t.line(format_args!("  {f}"));
        }
        if let Some(files) = &files {
            t.blank();
            t.line(format_args!(
                "Working state: {}",
                plural(files.files.len(), "file", "files")
            ));
            for f in &files.files {
                t.line(format_args!("  {:>10}  {}", f.size, f.path));
            }
            if files.restricted_count > 0 {
                t.line(format_args!(
                    "  ({} not shown: you may not read them)",
                    plural(files.restricted_count, "file", "files")
                ));
            }
        }
        if entry.manifest_hash.is_some() {
            t.line(format_args!(
                "Check it out with `wt checkout --staged {}{}`.",
                entry.author,
                if entry.label.is_empty() {
                    String::new()
                } else {
                    format!("/{}", entry.label)
                }
            ));
        }
    })
}

/// Split `USER[/LABEL]`.
fn parse_staged_spec(spec: &str) -> Result<(String, String)> {
    let (user, label) = spec.split_once('/').unwrap_or((spec, ""));
    if user.trim().is_empty() || (spec.contains('/') && label.is_empty()) {
        return Err(CliError::usage(format!(
            "invalid staged spec '{spec}': expected USER or USER/LABEL"
        )));
    }
    remote::validate_label(label)?;
    Ok((user.to_string(), label.to_string()))
}

/// `wt checkout --staged USER[/LABEL]`.
pub fn checkout(ctx: &Context, args: CheckoutArgs) -> Result<()> {
    let (user, label) = parse_staged_spec(&args.staged)?;
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let opts = remote::CheckoutStaged {
        user,
        label,
        branch: args.branch,
        new_branch: args.name,
        force: args.force,
    };
    let report = block_on(remote::checkout_staged(&repo, &tree.name, &remote, &opts))??;
    ctx.out.emit(&report, |t| {
        t.line(format_args!(
            "Switched to a new branch {} with the work {} staged on {}{} ({})",
            report.branch.green(),
            report.author.cyan(),
            report.source_branch.green(),
            if report.label.is_empty() {
                String::new()
            } else {
                format!(" in slot {}", report.label)
            },
            ago(report.staged_at)
        ));
        t.line(format_args!(
            "  snapshot {} ({}, {} downloaded)",
            short(&report.snapshot_id).yellow(),
            plural(report.files, "file", "files"),
            plural(report.objects_downloaded, "object", "objects")
        ));
    })
}

/// Push-time advisory lines (never blocks the push).
pub fn render_overlaps(t: &mut Text, overlaps: &[StagedOverlap]) {
    const SHOWN: usize = 3;
    for o in overlaps {
        let mut paths = o
            .paths
            .iter()
            .take(SHOWN)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        if o.paths.len() > SHOWN {
            paths.push_str(&format!(" and {} more", o.paths.len() - SHOWN));
        }
        let slot = if o.label.is_empty() {
            String::new()
        } else {
            format!(", label {}", o.label)
        };
        t.line(format_args!(
            "{} {} has staged changes to {paths} (on {}{slot}, {})",
            "note:".yellow().bold(),
            o.author.cyan(),
            o.branch,
            ago(o.staged_at)
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn parse(args: &[&str]) -> std::result::Result<crate::cli::Command, clap::Error> {
        use clap::Parser;
        crate::cli::Cli::try_parse_from(std::iter::once("wt").chain(args.iter().copied()))
            .map(|c| c.command)
    }

    #[test]
    fn arguments() {
        use crate::cli::Command;
        let Ok(Command::Stage(a)) = parse(&["stage", "--label", "exp"]) else {
            panic!()
        };
        assert_eq!(a.label.as_deref(), Some("exp"));
        assert!(a.command.is_none());
        let Ok(Command::Stage(a)) = parse(&["stage", "list", "--mine", "--history"]) else {
            panic!()
        };
        assert!(matches!(
            a.command,
            Some(StageCommand::List(StageListArgs {
                mine: true,
                history: true,
                ..
            }))
        ));
        let Ok(Command::Stage(a)) = parse(&["stage", "show", "exp", "--user", "bob", "--files"])
        else {
            panic!()
        };
        let Some(StageCommand::Show(s)) = a.command else {
            panic!()
        };
        assert_eq!(
            (s.label.as_deref(), s.user.as_deref(), s.files),
            (Some("exp"), Some("bob"), true)
        );
        assert!(parse(&["stage", "--label", "x", "list"]).is_err());

        let Ok(Command::Unstage(u)) = parse(&["unstage", "--label", "exp"]) else {
            panic!()
        };
        assert_eq!(u.label.as_deref(), Some("exp"));
        let Ok(Command::Unstage(u)) = parse(&["unstage", "--all-labels", "--branch", "dev"]) else {
            panic!()
        };
        assert!(u.all_labels);
        assert!(parse(&["unstage", "--label", "x", "--all"]).is_err());
        assert!(parse(&["unstage", "--label", "x", "--all-labels"]).is_err());

        let Ok(Command::Checkout(c)) = parse(&["checkout", "--staged", "alice/exp", "-b", "try"])
        else {
            panic!()
        };
        assert_eq!(
            (c.staged.as_str(), c.name.as_deref()),
            ("alice/exp", Some("try"))
        );
        assert!(parse(&["checkout"]).is_err(), "--staged is required");
    }

    #[test]
    fn staged_specs() {
        assert_eq!(
            parse_staged_spec("alice").unwrap(),
            ("alice".into(), String::new())
        );
        assert_eq!(
            parse_staged_spec("alice/exp-1").unwrap(),
            ("alice".into(), "exp-1".into())
        );
        for bad in ["", "/x", "alice/", "alice/Bad"] {
            assert!(parse_staged_spec(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn overlap_lines() {
        colored::control::set_override(false);
        let o = StagedOverlap {
            author: "alice".into(),
            branch: "main".into(),
            label: "exp".into(),
            snapshot_id: "s".into(),
            message: String::new(),
            staged_at: Utc::now(),
            paths: vec!["a".into(), "b".into(), "c".into(), "d".into(), "e".into()],
        };
        let mut t = Text::default();
        render_overlaps(&mut t, &[o]);
        let out = t.as_str();
        assert!(
            out.contains(
                "note: alice has staged changes to a, b, c and 2 more (on main, label exp"
            ),
            "{out}"
        );
    }

    #[test]
    fn list_rendering_groups_by_author() {
        colored::control::set_override(false);
        let entry = |author: &str, label: &str| StagedInfo {
            snapshot_id: "0123456789abcdef".into(),
            author: author.into(),
            branch: "main".into(),
            files_changed: vec!["x".into()],
            files_added: 1,
            files_modified: 0,
            files_deleted: 0,
            message: Some("wip".into()),
            staged_at: Utc::now(),
            label: label.into(),
            manifest_hash: None,
        };
        let mut t = Text::default();
        render_list(
            &mut t,
            "origin",
            &[entry("alice", ""), entry("alice", "exp"), entry("bob", "")],
        );
        let out = t.as_str();
        assert_eq!(out.matches("alice").count(), 1, "{out}");
        assert!(out.contains("(default)") && out.contains("exp"), "{out}");
        let mut t = Text::default();
        render_list(&mut t, "origin", &[]);
        assert_eq!(t.as_str(), "No staged work on origin.\n");
    }
}
