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

    /// If the server is unreachable, queue the push in the offline outbox
    /// instead of failing (`wt sync queue` lists it, `wt sync flush` retries;
    /// the daemon delivers it automatically with `sync.auto_flush`)
    #[arg(long)]
    pub queue: bool, // sync workstream (§15.5)

    // ---- history-v2 (§16.10) ----
    /// Rewind the server branch when it has snapshots your branch lacks
    /// (needs branch:force_push; refused by no_force_push rules). Lease
    /// semantics: refused unless the server tip is the last remote tip you
    /// fetched, so you never overwrite work you have not seen
    #[arg(long, visible_alias = "force-with-lease", conflicts_with = "queue")]
    pub force: bool,

    /// Push past branch protection (owners: branch:bypass_protection;
    /// always audited on the server)
    #[arg(long, conflicts_with = "queue")]
    pub bypass_protection: bool,
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
    /// Sparse clone: check out and download only these paths or patterns
    /// (other content is downloaded on demand; see `wt sparse --help`)
    #[arg(long, value_name = "PATTERN", num_args = 1..)]
    pub sparse: Vec<String>,
    /// Start the background daemon (if needed) and auto-snapshot the clone
    #[arg(long)]
    pub watch: bool,
}

#[derive(Debug, Args)]
pub struct StageArgs {
    /// Remote to stage on (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Remove your staged snapshot of the current branch instead (same as `wt unstage`)
    #[arg(long)]
    pub discard: bool,
}

#[derive(Debug, Args)]
pub struct UnstageArgs {
    /// Remote to unstage from (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Branch whose staged snapshot to remove (default: the current branch)
    #[arg(long, value_name = "B", conflicts_with = "all")]
    pub branch: Option<String>,

    /// Remove your staged snapshots of every branch of the tree
    #[arg(long)]
    pub all: bool,
}

pub fn push(ctx: &Context, args: PushArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    // ---- sync workstream (§15.5): `wt push --queue` ----
    if args.queue {
        let out = block_on(remote::push_or_queue(
            &repo,
            &tree.name,
            &remote,
            args.branch.as_deref(),
        ))??;
        if let Some(entry) = &out.queued {
            return ctx.out.emit(&out, |t| {
                t.line(format_args!(
                    "Server unreachable; queued push of {} ({}) to {remote}.",
                    entry.branch.green(),
                    short(&entry.tip).yellow()
                ));
                t.line("Run `wt sync flush` to retry (the daemon retries automatically).");
            });
        }
        if let Some(report) = out.push {
            return render_push(ctx, &remote, &report);
        }
        return Ok(());
    }
    // history-v2 (§16.10): --force / --bypass-protection.
    let opts = remote::PushOptions {
        force: args.force,
        bypass_protection: args.bypass_protection,
    };
    let report = block_on(remote::push_with_options(
        &repo,
        &tree.name,
        &remote,
        args.branch.as_deref(),
        &opts,
    ))?
    .map_err(|e| {
        let unreachable = remote::is_unreachable(&e);
        let err = CliError::from(e);
        if unreachable {
            err.with_hint("use `wt push --queue` (or `wt sync`) to queue the push until the server is reachable")
        } else {
            err
        }
    })?;
    // ---- end sync workstream ----
    render_push(ctx, &remote, &report)
}

fn render_push(ctx: &Context, remote: &str, report: &remote::PushReport) -> Result<()> {
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
            "  {}{}{}  {} -> {remote}/{}{}",
            short_or(report.old_tip.as_deref(), "(new)"),
            if report.forced { "..." } else { ".." },
            short(&report.new_tip),
            report.branch.green(),
            report.branch,
            if report.forced {
                " (forced update)"
            } else {
                ""
            }
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
    /// sparse (§15.4): sparse checkout state of a `--sparse` clone.
    #[serde(skip_serializing_if = "Option::is_none")]
    sparse: Option<&'a worktree_sdk::repo::SparseStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    watching: Option<&'a str>,
}

pub fn clone(ctx: &Context, args: CloneArgs) -> Result<()> {
    let (tenant, tree_name) = parse_tree_spec(&args.spec)?;
    let server = normalize_server(&args.url);
    let dest = ctx.absolute(args.dest.as_deref().unwrap_or_else(|| Path::new(tree_name)));
    let opts = FetchOptions {
        depth: args.depth,
        deepen: None,
    };
    // sparse (§15.4): `--sparse PATTERN...` makes a partial clone.
    let repo = block_on(remote::clone_sparse(
        &server,
        tenant,
        tree_name,
        &dest,
        opts,
        &args.sparse,
    ))??;
    let root_tree = repo.root_tree()?;
    let restricted = repo.restricted_paths(&root_tree.name)?;
    let sparse = repo.sparse_status(&root_tree.name)?;
    let watching = if args.watch {
        Some(crate::commands::daemon::start_and_watch(repo.root())?)
    } else {
        None
    };
    let result = CloneResult {
        root: repo.root(),
        server: &server,
        tenant,
        tree: tree_name,
        branch: &root_tree.current_branch,
        shallow: repo.is_shallow(&root_tree.name)?,
        restricted: &restricted,
        sparse: sparse.as_ref(),
        watching: watching.as_deref(),
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
        if let Some(s) = &sparse {
            t.line(format_args!(
                "Sparse checkout ({}): {}",
                s.patterns.join(" "),
                crate::commands::sparse::summary(s)
            ));
        }
        if !restricted.is_empty() {
            t.line(format_args!(
                "{} not checked out (restricted by license rules); see `wt status`.",
                plural(restricted.len(), "file", "files")
            ));
        }
        if let Some(msg) = &watching {
            t.line(format_args!("Daemon: {msg}"));
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

#[derive(Serialize)]
struct UnstageResult<'a> {
    tree: &'a str,
    remote: &'a str,
    /// `None` = every branch.
    branch: Option<&'a str>,
    removed: bool,
}

/// `wt unstage` / `wt stage --discard`: remove the caller's staged snapshot.
pub fn unstage(ctx: &Context, args: UnstageArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    let branch = match (args.all, args.branch) {
        (true, _) => None,
        (false, Some(b)) => Some(b),
        (false, None) => Some(repo.current_branch(&tree.name)?),
    };
    let removed = block_on(remote::unstage(
        &repo,
        &tree.name,
        &remote,
        branch.as_deref(),
    ))??;
    let result = UnstageResult {
        tree: &tree.name,
        remote: &remote,
        branch: branch.as_deref(),
        removed,
    };
    ctx.out.emit(&result, |t| {
        let scope = match &branch {
            Some(b) => format!("on {}", b.green()),
            None => "on any branch".to_string(),
        };
        if removed {
            t.line(format_args!(
                "Removed your staged snapshot {scope} from {remote}"
            ));
        } else {
            t.line(format_args!("Nothing staged {scope} on {remote}"));
        }
    })
}

pub fn stage(ctx: &Context, args: StageArgs) -> Result<()> {
    if args.discard {
        return unstage(
            ctx,
            UnstageArgs {
                remote: args.remote,
                branch: None,
                all: false,
            },
        );
    }
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

    // history-v2 (§16.10)
    #[test]
    fn force_push_flags() {
        use crate::cli::Command;
        match parse(&["push", "--force", "--bypass-protection"]).expect("parse") {
            Command::Push(a) => assert!(a.force && a.bypass_protection && !a.queue),
            other => panic!("{other:?}"),
        }
        match parse(&["push", "--force-with-lease"]).expect("parse") {
            Command::Push(a) => assert!(a.force && !a.bypass_protection),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            parse(&["push", "--force", "--queue"]).unwrap_err().kind(),
            clap::error::ErrorKind::ArgumentConflict
        );
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
        // sparse (§15.4)
        match parse(&[
            "clone", "http://x", "a/b", "dest", "--sparse", "src", "docs",
        ])
        .expect("parse")
        {
            Command::Clone(a) => {
                assert_eq!(a.sparse, vec!["src", "docs"]);
                assert_eq!(a.dest.as_deref(), Some(Path::new("dest")));
            }
            other => panic!("{other:?}"),
        }
        match parse(&["clone", "http://x", "a/b"]).expect("parse") {
            Command::Clone(a) => assert!(a.sparse.is_empty()),
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
