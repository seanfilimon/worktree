//! `wt sync` — fetch, integrate (merge or rebase) and push in one step;
//! manage the offline queue (ENGINEERING_PLAN §15.5).

use clap::{ArgGroup, Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::config::SyncStrategy;
use worktree_sdk::remote::{
    self, FlushReport, SyncIntegration, SyncOptions, SyncReport, SyncStatus, TreeSync,
};
use worktree_sdk::repo::OutboxEntry;
use worktree_sdk::{MergeOutcome, Repository};

use crate::commands::merge::render_outcome;
use crate::commands::remote::remote_name;
use crate::context::{block_on, Context};
use crate::error::{CliError, Result};
use crate::output::format::{ago, plural, short, short_or};
use crate::output::Text;

/// `wt sync [--merge|--rebase] [--no-push] [--all-trees] | --continue |
/// --abort | --status | queue | flush | drop N`
#[derive(Debug, Args)]
#[command(
    args_conflicts_with_subcommands = true,
    group(ArgGroup::new("mode").args(["resume", "abort", "status"])),
    after_long_help = "\
`wt sync` fetches every branch of the remote, integrates the remote copy of
the current branch, then pushes the result:

  * the remote has nothing new: just push (if there is anything to push);
  * only the remote moved: fast-forward;
  * both moved: merge (`sync.strategy = \"merge\"`, the default; one merge
    snapshot) or rebase (`sync.strategy = \"rebase\"`; your unpushed
    snapshots are replayed on top of the remote branch). `--merge` and
    `--rebase` override the configured strategy.

Integrating needs a clean working tree (snapshot or stash your changes
first). Conflicts stop the sync with exit code 3, exactly like `wt merge` /
`wt rebase`: fix the files (`wt merge status`, `wt merge resolve PATH
--ours|--theirs`), then run `wt sync --continue` to finish and push, or `wt
sync --abort` to undo the integration.

When the server cannot be reached, unpushed snapshots are queued in the
offline outbox (`.wt/trees/<tree>/outbox.jsonl`) instead: `wt sync queue`
(or `--status`) lists queued pushes, `wt sync flush` retries them, `wt sync
drop N` forgets one. `wt push --queue` queues explicitly. With
`sync.auto_flush = true` (the default) the background daemon delivers queued
pushes as soon as the server is reachable again, and with
`sync.auto_sync_secs = N` it runs `wt sync` every N seconds for each clean
tree (conflicting integrations are undone and left for you)."
)]
pub struct SyncArgs {
    #[command(subcommand)]
    pub action: Option<SyncCommand>,

    /// Integrate by replaying your unpushed snapshots onto the remote branch
    #[arg(long, conflicts_with = "merge")]
    pub rebase: bool,

    /// Integrate with a three-way merge (the default unless `sync.strategy = "rebase"`)
    #[arg(long)]
    pub merge: bool,

    /// Fetch and integrate only; do not push
    #[arg(long)]
    pub no_push: bool,

    /// Sync every tree of the worktree (each on its current branch)
    #[arg(long, conflicts_with_all = ["resume", "abort"])]
    pub all_trees: bool,

    /// Remote to sync with (default: [sync].remote, usually origin)
    #[arg(long, value_name = "R")]
    pub remote: Option<String>,

    /// Finish a sync stopped by conflicts (after resolving them) and push
    #[arg(long = "continue", conflicts_with_all = ["rebase", "merge"])]
    pub resume: bool,

    /// Undo the merge or rebase of a sync stopped by conflicts
    #[arg(long, conflicts_with_all = ["rebase", "merge", "no_push"])]
    pub abort: bool,

    /// List queued pushes (same as `wt sync queue`)
    #[arg(long, conflicts_with_all = ["rebase", "merge", "no_push"])]
    pub status: bool,
}

#[derive(Debug, Subcommand)]
pub enum SyncCommand {
    /// List pushes queued while the server was unreachable
    #[command(alias = "status")]
    Queue {
        /// List the queues of every tree
        #[arg(long)]
        all_trees: bool,
    },
    /// Retry queued pushes now
    Flush {
        /// Flush the queues of every tree
        #[arg(long)]
        all_trees: bool,
    },
    /// Forget queued push N (as numbered by `wt sync queue`) without pushing it
    Drop {
        /// Entry number (1 = oldest)
        #[arg(value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
        n: u64,
    },
}

pub fn run(ctx: &Context, args: SyncArgs) -> Result<()> {
    match args.action {
        Some(SyncCommand::Queue { all_trees }) => return queue(ctx, all_trees),
        Some(SyncCommand::Flush { all_trees }) => return flush(ctx, all_trees),
        Some(SyncCommand::Drop { n }) => return drop_entry(ctx, n),
        None => {}
    }
    if args.status {
        return queue(ctx, args.all_trees);
    }
    let (repo, tree) = ctx.open_tree()?;
    let remote = remote_name(&repo, args.remote);
    if args.abort {
        let op = remote::sync_abort(&repo, &tree.name)?;
        let result = AbortResult {
            tree: &tree.name,
            aborted: op,
        };
        return ctx.out.emit(&result, |t| {
            t.line(format_args!(
                "Sync aborted: the {} was undone; {} is back where it was.",
                op,
                tree.current_branch.green()
            ))
        });
    }
    let opts = SyncOptions {
        strategy: if args.rebase {
            Some(SyncStrategy::Rebase)
        } else if args.merge {
            Some(SyncStrategy::Merge)
        } else {
            None
        },
        no_push: args.no_push,
        abort_on_conflict: false,
    };
    if args.all_trees {
        let all = block_on(remote::sync_all(&repo, &remote, opts))??;
        ctx.out.emit(&all, |t| {
            for (i, entry) in all.iter().enumerate() {
                if i > 0 {
                    t.blank();
                }
                render_tree_sync(t, entry);
            }
        })?;
        return all_trees_status(&all);
    }
    let report = if args.resume {
        block_on(remote::sync_continue(&repo, &tree.name, &remote, opts))??
    } else {
        block_on(remote::sync(&repo, &tree.name, &remote, opts))??
    };
    ctx.out.emit(&report, |t| render_report(t, &report))?;
    report_status(&report)
}

#[derive(Serialize)]
struct AbortResult<'a> {
    tree: &'a str,
    /// The integration that was undone.
    aborted: SyncStrategy,
}

/// Exit status of one sync: 3 on conflicts, 1 when offline with nothing queued.
fn report_status(report: &SyncReport) -> Result<()> {
    match report.status {
        SyncStatus::Conflicts => Err(CliError::conflicts(format!(
            "sync stopped: integrating {}/{} produced {}",
            report.remote,
            report.branch,
            plural(report.conflicts.len(), "conflict", "conflicts")
        ))
        .with_hint(
            "fix the conflicts (`wt merge status`, `wt merge resolve PATH --ours|--theirs`), \
             then run `wt sync --continue` (or `wt sync --abort`)",
        )),
        SyncStatus::Offline => Err(CliError::failure(format!(
            "cannot reach {}: {}",
            report.remote,
            report
                .offline_error
                .as_deref()
                .unwrap_or("server unreachable")
        ))
        .with_hint("nothing was queued (no unpushed snapshots); try again when the server is up")),
        _ => Ok(()),
    }
}

fn all_trees_status(all: &[TreeSync]) -> Result<()> {
    let reports = || all.iter().filter_map(|t| t.report.as_ref());
    let conflicted: Vec<&str> = reports()
        .filter(|r| r.status == SyncStatus::Conflicts)
        .map(|r| r.tree.as_str())
        .collect();
    if !conflicted.is_empty() {
        return Err(CliError::conflicts(format!(
            "sync stopped with conflicts in {}",
            conflicted.join(", ")
        ))
        .with_hint("resolve them, then run `wt --tree TREE sync --continue` for each tree"));
    }
    let failed: Vec<String> = all
        .iter()
        .filter_map(|t| t.error.as_ref().map(|e| format!("{}: {e}", t.tree)))
        .chain(
            reports()
                .filter(|r| r.status == SyncStatus::Offline)
                .map(|r| format!("{}: server unreachable", r.tree)),
        )
        .collect();
    if failed.is_empty() {
        Ok(())
    } else {
        Err(CliError::failure(format!(
            "sync failed for {}",
            failed.join("; ")
        )))
    }
}

fn render_tree_sync(t: &mut Text, entry: &TreeSync) {
    match (&entry.report, &entry.error) {
        (Some(report), _) => render_report(t, report),
        (None, Some(error)) => t.line(format_args!(
            "{} {}: {error}",
            "error".red().bold(),
            entry.tree.bold()
        )),
        (None, None) => {}
    }
}

fn render_report(t: &mut Text, r: &SyncReport) {
    t.line(format_args!(
        "Sync {} ({}) with {} [{}]",
        r.tree.bold(),
        r.branch.green(),
        r.remote,
        r.strategy
    ));
    // Fetch.
    match &r.fetch {
        Some(f) if f.updated.is_empty() => t.line("  fetch   nothing new"),
        Some(f) => t.line(format_args!(
            "  fetch   {} updated; received {} and {}",
            plural(f.updated.len(), "branch", "branches"),
            plural(f.snapshots_received, "snapshot", "snapshots"),
            plural(f.objects_received, "object", "objects")
        )),
        None if r.offline_error.is_some() && r.integration == SyncIntegration::None => {
            t.line(format_args!(
                "  fetch   {} ({})",
                "server unreachable".yellow(),
                r.offline_error.as_deref().unwrap_or_default()
            ))
        }
        None if r.integration == SyncIntegration::None => {}
        None => t.line("  fetch   skipped (not on the server yet)"),
    }
    // Integration.
    let source = format!("{}/{}", r.remote, r.branch);
    let new = short_or(r.new_tip.as_deref(), "(none)");
    match r.integration {
        SyncIntegration::None => {}
        SyncIntegration::UpToDate => {
            t.line(format_args!("  {:<7} up to date with {source}", r.strategy))
        }
        SyncIntegration::FastForward => t.line(format_args!(
            "  {:<7} fast-forward to {}",
            r.strategy,
            new.yellow()
        )),
        SyncIntegration::Merged => t.line(format_args!(
            "  merge   merged {source} (merge snapshot {})",
            new.yellow()
        )),
        SyncIntegration::Rebased => t.line(format_args!(
            "  rebase  replayed {} onto {source} (now {})",
            plural(r.replayed, "snapshot", "snapshots"),
            new.yellow()
        )),
        SyncIntegration::Conflicts => {
            t.line(format_args!(
                "  {:<7} {} with {source}",
                r.strategy,
                "stopped by conflicts".red()
            ));
            render_outcome(
                t,
                &MergeOutcome::Conflicts {
                    conflicts: r.conflicts.clone(),
                },
            );
        }
        SyncIntegration::Aborted => t.line(format_args!(
            "  {:<7} conflicted with {source}; undone",
            r.strategy
        )),
    }
    // Push.
    if let Some(q) = &r.queued {
        t.line(format_args!(
            "  push    {}: queued {} -> {}/{} ({}); run `wt sync flush` to retry",
            "server unreachable".yellow(),
            short(&q.tip),
            r.remote,
            q.branch,
            r.offline_error.as_deref().unwrap_or("offline")
        ));
    } else if let Some(p) = &r.push {
        if p.up_to_date {
            t.line("  push    nothing to push");
        } else {
            t.line(format_args!(
                "  push    {}..{} -> {}/{}; sent {} and {}",
                short_or(p.old_tip.as_deref(), "(new)"),
                short(&p.new_tip),
                r.remote,
                p.branch,
                plural(p.snapshots_sent, "snapshot", "snapshots"),
                plural(p.objects_sent, "object", "objects")
            ));
        }
    } else if !matches!(r.status, SyncStatus::Conflicts | SyncStatus::Offline) {
        t.line("  push    nothing to push");
    }
    t.line(match r.status {
        SyncStatus::UpToDate => "Everything up to date.".normal(),
        SyncStatus::Synced => "Synced.".green(),
        SyncStatus::Conflicts => "Sync stopped by conflicts.".red(),
        SyncStatus::Aborted => "Sync undone (conflicts).".yellow(),
        SyncStatus::Queued => "Push queued (offline).".yellow(),
        SyncStatus::Offline => "Server unreachable; nothing to queue.".yellow(),
    });
}

// --------------------------------------------------------------------
// The outbox: queue / flush / drop
// --------------------------------------------------------------------

#[derive(Serialize)]
struct TreeQueue {
    tree: String,
    entries: Vec<OutboxEntry>,
}

/// The trees an outbox command covers: all, or the selected one.
fn outbox_trees(ctx: &Context, repo: &Repository, all_trees: bool) -> Result<Vec<String>> {
    Ok(if all_trees {
        repo.trees()?.into_iter().map(|t| t.name).collect()
    } else {
        vec![ctx.select_tree(repo)?.name]
    })
}

fn queue(ctx: &Context, all_trees: bool) -> Result<()> {
    let repo = ctx.open()?;
    let queues = outbox_trees(ctx, &repo, all_trees)?
        .into_iter()
        .map(|tree| {
            Ok(TreeQueue {
                entries: repo.outbox(&tree)?,
                tree,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    ctx.out.emit(&queues, |t| render_queues(t, &queues))
}

fn render_queues(t: &mut Text, queues: &[TreeQueue]) {
    let total: usize = queues.iter().map(|q| q.entries.len()).sum();
    if total == 0 {
        t.line("No queued pushes.");
        return;
    }
    for q in queues.iter().filter(|q| !q.entries.is_empty()) {
        t.line(format_args!(
            "Queued pushes of {} ({}):",
            q.tree.bold(),
            q.entries.len()
        ));
        for (i, e) in q.entries.iter().enumerate() {
            let mut line = format!(
                "  {:>2}  {} -> {}/{}  {}  queued {}",
                i + 1,
                e.branch.green(),
                e.remote,
                e.branch,
                short(&e.tip).yellow(),
                ago(e.queued_at)
            );
            if e.attempts > 0 {
                line.push_str(&format!(
                    "  ({})",
                    plural(e.attempts as usize, "attempt", "attempts")
                ));
            }
            t.line(line);
            if let Some(err) = &e.last_error {
                t.line(format_args!("        last error: {err}"));
            }
        }
    }
    t.line("Retry with `wt sync flush`; forget one with `wt sync drop N`.");
}

fn flush(ctx: &Context, all_trees: bool) -> Result<()> {
    let repo = ctx.open()?;
    let mut reports: Vec<FlushReport> = Vec::new();
    for tree in outbox_trees(ctx, &repo, all_trees)? {
        reports.push(block_on(remote::flush_outbox(&repo, &tree))??);
    }
    ctx.out.emit(&reports, |t| render_flush(t, &reports))?;
    let remaining: usize = reports.iter().map(|r| r.remaining).sum();
    if remaining == 0 {
        return Ok(());
    }
    let offline = reports.iter().any(FlushReport::offline);
    let err = CliError::failure(format!(
        "{} still queued",
        plural(remaining, "push", "pushes")
    ));
    Err(if offline {
        err.with_hint("the server is unreachable; try again later (the daemon retries automatically with `sync.auto_flush`)")
    } else {
        err.with_hint(
            "a rejected push usually means the remote moved: run `wt sync` to integrate and push",
        )
    })
}

fn render_flush(t: &mut Text, reports: &[FlushReport]) {
    let mut any = false;
    for r in reports {
        for d in &r.delivered {
            any = true;
            let p = &d.push;
            if p.up_to_date {
                t.line(format_args!(
                    "Delivered {} -> {}/{} (already up to date)",
                    d.entry.branch.green(),
                    d.entry.remote,
                    p.branch
                ));
            } else {
                t.line(format_args!(
                    "Delivered {} -> {}/{}: {}..{}, {}",
                    d.entry.branch.green(),
                    d.entry.remote,
                    p.branch,
                    short_or(p.old_tip.as_deref(), "(new)"),
                    short(&p.new_tip),
                    plural(p.snapshots_sent, "snapshot", "snapshots")
                ));
            }
        }
        for f in &r.failed {
            any = true;
            t.line(format_args!(
                "{} {} -> {}/{}: {}",
                if f.offline {
                    "Still offline".yellow()
                } else {
                    "Rejected".red()
                },
                f.entry.branch,
                f.entry.remote,
                f.entry.branch,
                f.error
            ));
        }
    }
    if !any {
        t.line("No queued pushes.");
    }
}

#[derive(Serialize)]
struct DropResult<'a> {
    tree: &'a str,
    dropped: &'a OutboxEntry,
    remaining: usize,
}

fn drop_entry(ctx: &Context, n: u64) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let n = usize::try_from(n).map_err(|_| CliError::usage("entry number too large"))?;
    let dropped = repo.outbox_drop(&tree.name, n)?;
    let remaining = repo.outbox(&tree.name)?.len();
    let result = DropResult {
        tree: &tree.name,
        dropped: &dropped,
        remaining,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "Dropped queued push {n} ({} -> {}/{} at {}); {} left.",
            dropped.branch,
            dropped.remote,
            dropped.branch,
            short(&dropped.tip),
            plural(remaining, "push", "pushes")
        ))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use worktree_sdk::remote::{FetchReport, PushReport};

    fn parse(args: &[&str]) -> std::result::Result<SyncArgs, clap::Error> {
        use clap::Parser;
        #[derive(Debug, Parser)]
        struct Harness {
            #[command(flatten)]
            args: SyncArgs,
        }
        Harness::try_parse_from(std::iter::once("wt").chain(args.iter().copied())).map(|h| h.args)
    }

    #[test]
    fn parses_flags_and_subcommands() {
        let a = parse(&[]).unwrap();
        assert!(!a.rebase && !a.merge && !a.no_push && !a.all_trees && a.action.is_none());
        let a = parse(&["--rebase", "--no-push", "--all-trees", "--remote", "up"]).unwrap();
        assert!(a.rebase && a.no_push && a.all_trees);
        assert_eq!(a.remote.as_deref(), Some("up"));
        assert!(parse(&["--rebase", "--merge"]).is_err());
        assert!(parse(&["--continue"]).unwrap().resume);
        assert!(parse(&["--continue", "--abort"]).is_err());
        assert!(parse(&["--continue", "--rebase"]).is_err());
        assert!(parse(&["--abort", "--all-trees"]).is_err());
        assert!(parse(&["--status"]).unwrap().status);
        assert!(matches!(
            parse(&["queue"]).unwrap().action,
            Some(SyncCommand::Queue { all_trees: false })
        ));
        assert!(matches!(
            parse(&["status", "--all-trees"]).unwrap().action,
            Some(SyncCommand::Queue { all_trees: true })
        ));
        assert!(matches!(
            parse(&["flush"]).unwrap().action,
            Some(SyncCommand::Flush { .. })
        ));
        assert!(matches!(
            parse(&["drop", "2"]).unwrap().action,
            Some(SyncCommand::Drop { n: 2 })
        ));
        assert!(parse(&["drop", "0"]).is_err());
        assert!(parse(&["drop"]).is_err());
        assert!(parse(&["--rebase", "flush"]).is_err());
    }

    fn report(status: SyncStatus) -> SyncReport {
        SyncReport {
            tree: "proj".into(),
            branch: "main".into(),
            remote: "origin".into(),
            strategy: SyncStrategy::Merge,
            status,
            fetch: Some(FetchReport {
                tree: "proj".into(),
                updated: vec![(
                    "main".into(),
                    Some("aaaaaaaa11".into()),
                    Some("bbbbbbbb22".into()),
                )],
                snapshots_received: 1,
                objects_received: 2,
                tags_received: 0,
            }),
            integration: SyncIntegration::Merged,
            replayed: 0,
            old_tip: Some("cccccccc33".into()),
            new_tip: Some("dddddddd44".into()),
            push: Some(PushReport {
                tree: "proj".into(),
                branch: "main".into(),
                old_tip: Some("bbbbbbbb22".into()),
                new_tip: "dddddddd44".into(),
                snapshots_sent: 2,
                objects_sent: 3,
                up_to_date: false,
            }),
            queued: None,
            conflicts: Vec::new(),
            offline_error: None,
        }
    }

    #[test]
    fn summary_rendering() {
        colored::control::set_override(false);
        let mut t = Text::default();
        render_report(&mut t, &report(SyncStatus::Synced));
        let out = t.as_str();
        assert!(
            out.contains("Sync proj (main) with origin [merge]"),
            "{out}"
        );
        assert!(
            out.contains("1 branch updated; received 1 snapshot and 2 objects"),
            "{out}"
        );
        assert!(
            out.contains("merged origin/main (merge snapshot dddddddd)"),
            "{out}"
        );
        assert!(
            out.contains("bbbbbbbb..dddddddd -> origin/main; sent 2 snapshots and 3 objects"),
            "{out}"
        );
        assert!(out.ends_with("Synced.\n"), "{out}");

        let mut queued = report(SyncStatus::Queued);
        queued.fetch = None;
        queued.integration = SyncIntegration::None;
        queued.push = None;
        queued.offline_error = Some("cannot connect".into());
        queued.queued = Some(OutboxEntry {
            id: "x".into(),
            remote: "origin".into(),
            branch: "main".into(),
            tip: "eeeeeeee55".into(),
            queued_at: chrono::Utc::now(),
            attempts: 0,
            last_attempt_at: None,
            last_error: None,
        });
        let mut t = Text::default();
        render_report(&mut t, &queued);
        let out = t.as_str();
        assert!(
            out.contains("fetch   server unreachable (cannot connect)"),
            "{out}"
        );
        assert!(out.contains("queued eeeeeeee -> origin/main"), "{out}");
        assert!(out.contains("wt sync flush"), "{out}");
        assert!(report_status(&queued).is_ok());

        let mut conflicts = report(SyncStatus::Conflicts);
        conflicts.integration = SyncIntegration::Conflicts;
        conflicts.push = None;
        let err = report_status(&conflicts).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        let mut offline = report(SyncStatus::Offline);
        offline.offline_error = Some("down".into());
        assert_eq!(report_status(&offline).unwrap_err().exit_code(), 1);
    }

    #[test]
    fn all_trees_exit_codes() {
        let ok = TreeSync {
            tree: "a".into(),
            report: Some(report(SyncStatus::Synced)),
            error: None,
        };
        let failed = TreeSync {
            tree: "b".into(),
            report: None,
            error: Some("boom".into()),
        };
        let mut conflicted = ok.clone();
        conflicted.report.as_mut().unwrap().status = SyncStatus::Conflicts;
        assert!(all_trees_status(std::slice::from_ref(&ok)).is_ok());
        let err = all_trees_status(&[ok.clone(), failed.clone()]).unwrap_err();
        assert_eq!(err.exit_code(), 1);
        assert!(err.to_string().contains("b: boom"));
        assert_eq!(
            all_trees_status(&[failed, conflicted])
                .unwrap_err()
                .exit_code(),
            3
        );
    }

    #[test]
    fn queue_rendering() {
        colored::control::set_override(false);
        let mut t = Text::default();
        render_queues(&mut t, &[]);
        assert_eq!(t.as_str(), "No queued pushes.\n");
        let q = TreeQueue {
            tree: "proj".into(),
            entries: vec![OutboxEntry {
                id: "x".into(),
                remote: "origin".into(),
                branch: "main".into(),
                tip: "eeeeeeee55".into(),
                queued_at: chrono::Utc::now(),
                attempts: 2,
                last_attempt_at: None,
                last_error: Some("offline".into()),
            }],
        };
        let mut t = Text::default();
        render_queues(&mut t, &[q]);
        let out = t.as_str();
        assert!(out.contains("Queued pushes of proj (1):"), "{out}");
        assert!(out.contains(" 1  main -> origin/main  eeeeeeee"), "{out}");
        assert!(
            out.contains("(2 attempts)") && out.contains("last error: offline"),
            "{out}"
        );
    }
}
