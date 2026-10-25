//! `wt cherry-pick` — apply the changes of existing snapshots onto the
//! current branch (ENGINEERING_PLAN §14.5), plus the renderer shared with
//! `wt rebase`.

use clap::{ArgGroup, Args};
use colored::Colorize;
use worktree_sdk::repo::{CherryPickOptions, RewriteOp, RewriteOutcome, RewriteStatus};
use worktree_sdk::{MergeOutcome, Repository};

use crate::commands::merge::render_outcome;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{plural, short, short_or, subject};
use crate::output::Text;

/// `wt cherry-pick REV... [--no-snapshot] | --continue | --skip | --abort`
#[derive(Debug, Args)]
#[command(
    group(ArgGroup::new("operation").required(true).args(["revs", "resume", "abort", "skip"])),
    after_long_help = "\
Each snapshot's change relative to its first parent is merged into the current
branch with the three-way merge engine and recorded as a new snapshot (same
author and message). Snapshots whose changes are already present are skipped.
Needs a clean working tree.

A conflict stops the cherry-pick like `wt merge`: conflict markers in the
files, `wt merge status` lists the conflicts and `wt merge resolve PATH
--ours|--theirs|--file F` resolves them; then run `wt cherry-pick --continue`
(or `--skip` to drop that snapshot, `--abort` to return to the state before
the cherry-pick)."
)]
pub struct CherryPickArgs {
    /// Snapshots to apply, in order (ids, branches, HEAD~N, ...)
    #[arg(value_name = "REV")]
    pub revs: Vec<String>,

    /// Apply the changes to the working tree only; create no snapshots
    #[arg(short = 'n', long, conflicts_with_all = ["resume", "abort", "skip"])]
    pub no_snapshot: bool,

    /// Finish the stopped step after resolving its conflicts and apply the rest
    #[arg(long = "continue")]
    pub resume: bool,

    /// Drop the stopped step and apply the rest
    #[arg(long)]
    pub skip: bool,

    /// Abandon the cherry-pick and restore the branch and working tree
    #[arg(long)]
    pub abort: bool,
}

#[derive(serde::Serialize)]
struct AbortResult<'a> {
    tree: &'a str,
    op: RewriteOp,
    aborted: bool,
}

pub fn run(ctx: &Context, args: CherryPickArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let outcome = if args.abort {
        repo.cherry_pick_abort(&tree.name)?;
        return emit_abort(ctx, &tree.name, RewriteOp::CherryPick);
    } else if args.resume {
        repo.cherry_pick_continue(&tree.name)?
    } else if args.skip {
        repo.cherry_pick_skip(&tree.name)?
    } else {
        let opts = CherryPickOptions {
            no_snapshot: args.no_snapshot,
        };
        repo.cherry_pick(&tree.name, &args.revs, &opts)?
    };
    emit_outcome(ctx, &repo, &outcome)
}

/// Print `outcome` and exit 3 when it stopped at a conflict.
pub fn emit_outcome(ctx: &Context, repo: &Repository, outcome: &RewriteOutcome) -> Result<()> {
    let subjects = if ctx.out.is_json() {
        Vec::new()
    } else {
        picked_subjects(repo, outcome)
    };
    ctx.out
        .emit(outcome, |t| render_rewrite(t, outcome, &subjects))?;
    rewrite_status(outcome)
}

pub fn emit_abort(ctx: &Context, tree: &str, op: RewriteOp) -> Result<()> {
    let result = AbortResult {
        tree,
        op,
        aborted: true,
    };
    ctx.out.emit(&result, |t| {
        t.line(format_args!(
            "{} aborted; the branch and working tree were restored.",
            capitalized(op)
        ))
    })
}

/// `(short new id, subject)` of each created snapshot.
fn picked_subjects(repo: &Repository, outcome: &RewriteOutcome) -> Vec<(String, String)> {
    outcome
        .picked
        .iter()
        .filter_map(|p| p.new.as_deref())
        .map(|id| {
            let subject = repo
                .get_snapshot(id)
                .map(|s| subject(&s.message).to_string())
                .unwrap_or_default();
            (short(id).to_string(), subject)
        })
        .collect()
}

/// Human rendering of a cherry-pick / rebase outcome.
pub fn render_rewrite(t: &mut Text, outcome: &RewriteOutcome, subjects: &[(String, String)]) {
    match outcome.status {
        RewriteStatus::UpToDate => {
            t.line(format_args!(
                "Current branch {} is up to date with {}.",
                outcome.branch.green(),
                short_or(outcome.onto.as_deref(), "?").yellow()
            ));
            return;
        }
        RewriteStatus::FastForward => {
            t.line(format_args!(
                "Fast-forwarded {} to {}.",
                outcome.branch.green(),
                short_or(outcome.new_tip.as_deref(), "?").yellow()
            ));
            return;
        }
        RewriteStatus::Completed | RewriteStatus::Conflicts => {}
    }
    for (id, subject) in subjects {
        t.line(format_args!(
            "[{} {}] {subject}",
            outcome.branch.green(),
            id.yellow()
        ));
    }
    let applied_only = outcome.picked.iter().filter(|p| p.new.is_none()).count();
    if applied_only > 0 {
        t.line(format_args!(
            "Applied the changes of {} to the working tree (no snapshots created).",
            plural(applied_only, "snapshot", "snapshots")
        ));
    }
    for id in &outcome.skipped {
        t.line(format_args!(
            "skipped {} (no changes left to apply)",
            short(id).yellow()
        ));
    }
    match outcome.status {
        RewriteStatus::Conflicts => {
            render_outcome(
                t,
                &MergeOutcome::Conflicts {
                    conflicts: outcome.conflicts.clone(),
                },
            );
        }
        _ if outcome.op == RewriteOp::Rebase => t.line(format_args!(
            "Rebased {} onto {}: {}.",
            outcome.branch.green(),
            short_or(outcome.onto.as_deref(), "?").yellow(),
            plural(
                outcome.picked.len(),
                "snapshot replayed",
                "snapshots replayed"
            )
        )),
        _ => {}
    }
}

/// Exit code 3 when the operation stopped at a conflicting step.
pub fn rewrite_status(outcome: &RewriteOutcome) -> Result<()> {
    if outcome.status != RewriteStatus::Conflicts {
        return Ok(());
    }
    let name = outcome.op.as_str();
    Err(CliError::conflicts(format!(
        "{name} stopped at {} with {}",
        short_or(outcome.stopped_at.as_deref(), "?"),
        plural(outcome.conflicts.len(), "conflict", "conflicts")
    ))
    .with_hint(format!(
        "fix the conflicts (or `wt merge resolve PATH --ours|--theirs`), then run \
         `wt {name} --continue` (or `wt {name} --skip` / `wt {name} --abort`)"
    )))
}

fn capitalized(op: RewriteOp) -> &'static str {
    match op {
        RewriteOp::CherryPick => "Cherry-pick",
        RewriteOp::Rebase => "Rebase",
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use worktree_sdk::repo::RewrittenSnapshot;
    use worktree_sdk::{ConflictInfo, ConflictKind};

    use super::*;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        args: CherryPickArgs,
    }

    fn parse(args: &[&str]) -> std::result::Result<CherryPickArgs, clap::Error> {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        Harness::try_parse_from(argv).map(|h| h.args)
    }

    #[test]
    fn parses_operations() {
        let a = parse(&["abc", "def", "-n"]).unwrap();
        assert_eq!(a.revs, ["abc", "def"]);
        assert!(a.no_snapshot);
        assert!(parse(&["--continue"]).unwrap().resume);
        assert!(parse(&["--abort"]).unwrap().abort);
        assert!(parse(&["--skip"]).unwrap().skip);
        assert!(parse(&[]).is_err());
        assert!(parse(&["abc", "--continue"]).is_err());
        assert!(parse(&["--continue", "--abort"]).is_err());
        assert!(parse(&["--continue", "--no-snapshot"]).is_err());
    }

    fn outcome(status: RewriteStatus) -> RewriteOutcome {
        RewriteOutcome {
            op: RewriteOp::CherryPick,
            tree: "proj".into(),
            branch: "main".into(),
            status,
            old_tip: None,
            new_tip: None,
            onto: None,
            picked: vec![RewrittenSnapshot {
                original: "aaaaaaaa11".into(),
                new: Some("bbbbbbbb22".into()),
            }],
            skipped: vec!["cccccccc33".into()],
            stopped_at: Some("dddddddd44".into()),
            conflicts: vec![ConflictInfo {
                path: "a.txt".into(),
                kind: ConflictKind::Content,
                hunks: Vec::new(),
            }],
            remaining: 0,
        }
    }

    #[test]
    fn renders_and_exits_3_on_conflicts() {
        colored::control::set_override(false);
        let o = outcome(RewriteStatus::Conflicts);
        let mut t = Text::default();
        render_rewrite(&mut t, &o, &[("bbbbbbbb".into(), "fix".into())]);
        assert_eq!(
            t.as_str(),
            "[main bbbbbbbb] fix\nskipped cccccccc (no changes left to apply)\n\
             CONFLICT (content): a.txt\n"
        );
        let err = rewrite_status(&o).unwrap_err();
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains("stopped at dddddddd"), "{err}");
        assert!(rewrite_status(&outcome(RewriteStatus::Completed)).is_ok());
    }

    #[test]
    fn renders_rebase_states() {
        colored::control::set_override(false);
        let mut o = outcome(RewriteStatus::UpToDate);
        o.op = RewriteOp::Rebase;
        o.onto = Some("eeeeeeee55".into());
        let mut t = Text::default();
        render_rewrite(&mut t, &o, &[]);
        assert_eq!(
            t.as_str(),
            "Current branch main is up to date with eeeeeeee.\n"
        );
        o.status = RewriteStatus::Completed;
        let mut t = Text::default();
        render_rewrite(&mut t, &o, &[]);
        assert!(
            t.as_str()
                .ends_with("Rebased main onto eeeeeeee: 1 snapshot replayed.\n"),
            "{}",
            t.as_str()
        );
    }
}
