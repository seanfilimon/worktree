//! `wt push`, `wt fetch`, `wt pull`, `wt clone`, `wt stage`

use std::path::{Path, PathBuf};

use clap::Args;
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::remote::{self, FetchOptions, FetchReport};
use worktree_sdk::Repository;

use crate::commands::merge::{conflict_status, render_outcome};
use crate::commands::remote::{normalize_server, remote_name};
use crate::context::{block_on, Context};
use crate::error::{CliError, Result};
use crate::output::format::{plural, short, short_or};
use crate::output::Text;

#[derive(Debug, Args)]
pub struct PushArgs {
    /// Remote to push to (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Branch to push (default: the current branch)
    #[arg(long, value_name = "B")]
    pub branch: Option<String>,
}

#[derive(Debug, Args)]
pub struct FetchArgs {
    /// Remote to fetch from (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Download at most N new snapshots per branch (shallow history)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub depth: Option<u32>,

    /// Extend a shallow history by N snapshots per branch
    #[arg(long, value_name = "N", conflicts_with = "depth", value_parser = clap::value_parser!(u32).range(1..))]
    pub deepen: Option<u32>,
}

#[derive(Debug, Args)]
pub struct PullArgs {
    /// Remote to pull from (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Branch to integrate (default: the current branch)
    #[arg(long, value_name = "B")]
    pub branch: Option<String>,

    /// Fetch at most N new snapshots per branch (shallow history)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub depth: Option<u32>,
}

#[derive(Debug, Args)]
pub struct CloneArgs {
    /// Server URL, e.g. http://localhost:7420
    pub url: String,

    /// Tree to clone
    #[arg(value_name = "TENANT/TREE")]
    pub spec: String,

    /// Destination directory (default: ./TREE)
    pub dest: Option<PathBuf>,

    /// Clone only the newest N snapshots of each branch (shallow clone)
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub depth: Option<u32>,
}

#[derive(Debug, Args)]
pub struct StageArgs {
    /// Remote to stage on (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,
}

pub fn push(ctx: &Context, args: PushArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let report = block_on(remote::push(
        &repo,
        &tree.name,
        &remote,
        args.branch.as_deref(),
    ))??;
    ctx.out.emit(&report, |t| {
        if report.up_to_date {
            t.line(format_args!(
                "Everything up to date ({remote}/{} at {})",
                report.branch,
                short(&report.new_tip).yellow()
            ));
            return;
        }
        t.line(format_args!(
            "  {}..{}  {} -> {remote}/{}",
            short_or(report.old_tip.as_deref(), "(new)"),
            short(&report.new_tip),
            report.branch.green(),
            report.branch
        ));
        t.line(format_args!(
            "Pushed {} and {}.",
            plural(report.snapshots_sent, "snapshot", "snapshots"),
            plural(report.objects_sent, "object", "objects")
        ));
    })
}

pub fn fetch(ctx: &Context, args: FetchArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let opts = FetchOptions {
        depth: args.depth,
        deepen: args.deepen,
    };
    let report = block_on(remote::fetch_with_options(&repo, &tree.name, &remote, opts))??;
    let shallow = shallow_note(&repo, &tree.name)?;
    ctx.out.emit(&report, |t| {
        render_fetch(t, &remote, &report);
        if let Some(note) = &shallow {
            t.line(note);
        }
    })
}

pub fn pull(ctx: &Context, args: PullArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let opts = FetchOptions {
        depth: args.depth,
        deepen: None,
    };
    let report = block_on(remote::pull_with_options(
        &repo,
        &tree.name,
        &remote,
        args.branch.as_deref(),
        opts,
    ))??;
    ctx.out.emit(&report, |t| {
        render_fetch(t, &remote, &report.fetch);
        render_outcome(t, &report.outcome);
    })?;
    conflict_status(&report.outcome)
}

/// "History is shallow (N boundary snapshots)..." when the tree is shallow.
fn shallow_note(repo: &Repository, tree: &str) -> Result<Option<String>> {
    let boundary = repo.shallow_boundary(tree)?;
    Ok((!boundary.is_empty()).then(|| {
        format!(
            "History is shallow ({}); use `wt fetch --deepen N` for more.",
            plural(boundary.len(), "boundary snapshot", "boundary snapshots")
        )
    }))
}

fn render_fetch(t: &mut Text, remote: &str, report: &FetchReport) {
    if report.updated.is_empty() {
        t.line(format_args!("Already up to date with {remote}."));
        return;
    }
    for (branch, old, new) in &report.updated {
        let target = format!("{remote}/{branch}");
        match (old, new) {
            (None, Some(_)) => t.line(format_args!(
                " * {:<18}  {branch} -> {}",
                "[new branch]",
                target.red()
            )),
            (Some(_), None) => t.line(format_args!(" - {:<18}  {}", "[deleted]", target.red())),
            (old, new) => t.line(format_args!(
                "   {:<18}  {branch} -> {}",
                format!(
                    "{}..{}",
                    short_or(old.as_deref(), ""),
                    short_or(new.as_deref(), "")
                ),
                target.red()
            )),
        }
    }
    t.line(format_args!(
        "Received {}, {} and {}.",
        plural(report.snapshots_received, "snapshot", "snapshots"),
        plural(report.objects_received, "object", "objects"),
        plural(report.tags_received, "tag", "tags")
    ));
}

#[derive(Serialize)]
struct CloneResult<'a> {
    root: &'a Path,
    server: &'a str,
    tenant: &'a str,
    tree: &'a str,
    branch: &'a str,
    shallow: bool,
    restricted: &'a [String],
}

pub fn clone(ctx: &Context, args: CloneArgs) -> Result<()> {
    let (tenant, tree_name) = parse_tree_spec(&args.spec)?;
    let server = normalize_server(&args.url);
    let dest = ctx.absolute(args.dest.as_deref().unwrap_or_else(|| Path::new(tree_name)));
    let opts = FetchOptions {
        depth: args.depth,
        deepen: None,
    };
    let repo = block_on(remote::clone_with_options(
        &server, tenant, tree_name, &dest, opts,
    ))??;
    let root_tree = repo.root_tree()?;
    let restricted = repo.restricted_paths(&root_tree.name)?;
    let result = CloneResult {
        root: repo.root(),
        server: &server,
        tenant,
        tree: tree_name,
        branch: &root_tree.current_branch,
        shallow: repo.is_shallow(&root_tree.name)?,
        restricted: &restricted,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Cloned {}/{} into {} (branch {})",
            tenant,
            tree_name.bold(),
            result.root.display(),
            result.branch.green()
        ));
        if result.shallow {
            t.line("History is shallow; use `wt fetch --deepen N` for more.");
        }
        if !restricted.is_empty() {
            t.line(format_args!(
                "{} not checked out (restricted by license rules); see `wt status`.",
                plural(restricted.len(), "file", "files")
            ));
        }
    })
}

/// Split `TENANT/TREE`.
fn parse_tree_spec(spec: &str) -> Result<(&str, &str)> {
    match spec.split_once('/') {
        Some((tenant, tree)) if !tenant.is_empty() && !tree.is_empty() && !tree.contains('/') => {
            Ok((tenant, tree))
        }
        _ => Err(
            CliError::usage(format!("invalid tree '{spec}': expected TENANT/TREE"))
                .with_hint("for example `wt clone http://localhost:7420 acme/web`"),
        ),
    }
}

pub fn stage(ctx: &Context, args: StageArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let staged = block_on(remote::stage(&repo, &tree.name, &remote))??;
    ctx.out.emit(&staged, |t| {
        t.line(format_args!(
            "Staged {} on {} to {remote} as {}",
            plural(staged.files_changed.len(), "changed file", "changed files"),
            staged.branch.green(),
            short(&staged.snapshot_id).yellow()
        ));
        t.line(format_args!(
            "  {} added, {} modified, {} deleted",
            staged.files_added, staged.files_modified, staged.files_deleted
        ));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> std::result::Result<crate::cli::Command, clap::Error> {
        use clap::Parser;
        crate::cli::Cli::try_parse_from(std::iter::once("wt").chain(args.iter().copied()))
            .map(|c| c.command)
    }

    #[test]
    fn shallow_flags() {
        use crate::cli::Command;
        match parse(&["fetch", "--depth", "3"]).expect("parse") {
            Command::Fetch(a) => assert_eq!((a.depth, a.deepen), (Some(3), None)),
            other => panic!("{other:?}"),
        }
        match parse(&["fetch", "--deepen", "2"]).expect("parse") {
            Command::Fetch(a) => assert_eq!((a.depth, a.deepen), (None, Some(2))),
            other => panic!("{other:?}"),
        }
        match parse(&["pull", "--depth", "1"]).expect("parse") {
            Command::Pull(a) => assert_eq!(a.depth, Some(1)),
            other => panic!("{other:?}"),
        }
        match parse(&["clone", "http://x", "a/b", "--depth", "5"]).expect("parse") {
            Command::Clone(a) => assert_eq!(a.depth, Some(5)),
            other => panic!("{other:?}"),
        }
        assert!(parse(&["fetch", "--depth", "1", "--deepen", "1"]).is_err());
        assert!(parse(&["fetch", "--depth", "0"]).is_err());
    }

    #[test]
    fn tree_specs() {
        assert_eq!(parse_tree_spec("acme/web").expect("ok"), ("acme", "web"));
        for bad in ["acme", "/web", "acme/", "a/b/c", ""] {
            let err = parse_tree_spec(bad).expect_err(bad);
            assert_eq!(err.exit_code(), 2, "{bad}");
        }
    }

    #[test]
    fn fetch_rendering() {
        colored::control::set_override(false);
        let report = FetchReport {
            tree: "web".into(),
            updated: vec![
                (
                    "main".into(),
                    Some("aaaaaaaa1111".into()),
                    Some("bbbbbbbb2222".into()),
                ),
                ("feat".into(), None, Some("cccccccc3333".into())),
                ("gone".into(), Some("dddddddd4444".into()), None),
            ],
            snapshots_received: 3,
            objects_received: 1,
            tags_received: 0,
        };
        let mut t = Text::default();
        render_fetch(&mut t, "origin", &report);
        let out = t.as_str();
        assert!(out.contains("aaaaaaaa..bbbbbbbb"), "{out}");
        assert!(out.contains("main -> origin/main"), "{out}");
        assert!(
            out.contains("[new branch]") && out.contains("feat -> origin/feat"),
            "{out}"
        );
        assert!(
            out.contains("[deleted]") && out.contains("origin/gone"),
            "{out}"
        );
        assert!(
            out.contains("Received 3 snapshots, 1 object and 0 tags."),
            "{out}"
        );

        let empty = FetchReport {
            updated: vec![],
            ..report
        };
        let mut t = Text::default();
        render_fetch(&mut t, "origin", &empty);
        assert_eq!(t.as_str(), "Already up to date with origin.\n");
    }
}
