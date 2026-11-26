//! `wt undo [N]`, `wt undo --list` and `wt redo [N]` — the local operation
//! log (ENGINEERING_PLAN §16.10, specs/tree/Undo.md).

use clap::Args;
use colored::Colorize;
use worktree_sdk::remote::{self, PushUndoPlan};
use worktree_sdk::repo::{OpLogEntry, PushUndo, UndoOptions, UndoReport};
use worktree_sdk::{Repository, SdkError};

use crate::commands::server_common::confirm;
use crate::context::{block_on, Context};
use crate::error::{CliError, Result};
use crate::output::format::{ago, plural, short_or, subject};
use crate::output::Text;

/// `wt undo`
#[derive(Debug, Args)]
#[command(after_long_help = "\
Every local operation that moves refs (snapshot, merge, revert, rebase,
cherry-pick, branch switch/create/archive/restore, tags, stash push/pop/
drop/clear) is recorded in .wt/oplog.jsonl with the before and after value
of each ref it changed. `wt undo` restores the before values of the newest
operation (`wt undo 3`: the newest three) and checks the restored head out;
`wt redo` re-applies what was undone until a new operation is recorded.
Undoing a stash push gives the stashed changes back to the working tree.
Both refuse while the working tree has uncommitted changes; --force
discards them. Snapshots are never deleted, so nothing undone is lost.

Pushes are recorded too. When the newest operation is a push, `wt undo`
moves the server branch back to the tip it had before the push (a restore
from the server reflog): it shows what will change and asks first (--yes
skips the question), refuses when someone pushed on top since, and needs
branch:force_push on the server (admin role), like a force push. Your local
branch keeps the snapshots: `wt push` publishes them again, and `wt revert`
is the append-only way to take pushed changes back. Undo a push on its own
(no N).")]
pub struct UndoArgs {
    /// How many operations to undo (default 1)
    #[arg(value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub count: Option<u32>,
    /// List the operations that can be undone, newest first (N = position)
    #[arg(long, conflicts_with_all = ["count", "force"])]
    pub list: bool,
    /// Proceed although the working tree has uncommitted changes (they are
    /// discarded) or refs moved outside the operation log
    #[arg(long)]
    pub force: bool,
    /// Undo a push without asking for confirmation
    #[arg(short, long, conflicts_with = "list")]
    pub yes: bool,
    /// Undoing a push: ask the server to bypass branch protection (needs
    /// branch:bypass_protection)
    #[arg(long, conflicts_with = "list")]
    pub bypass_protection: bool,
}

/// `wt redo`
#[derive(Debug, Args)]
pub struct RedoArgs {
    /// How many undone operations to re-apply (default 1)
    #[arg(value_name = "N", value_parser = clap::value_parser!(u32).range(1..))]
    pub count: Option<u32>,
    /// List the operations that can be redone, next first
    #[arg(long, conflicts_with_all = ["count", "force"])]
    pub list: bool,
    /// Proceed although the working tree has uncommitted changes (they are
    /// discarded) or refs moved outside the operation log
    #[arg(long)]
    pub force: bool,
}

pub fn run(ctx: &Context, args: UndoArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    if args.list {
        let ops = repo.oplog(&tree.name)?;
        return ctx
            .out
            .emit(&ops, |t| render_list(t, &ops, "Nothing to undo."));
    }
    if let Some(push) = repo.push_to_undo(&tree.name)? {
        return undo_push(ctx, &repo, push, &args);
    }
    let report = repo.undo(&tree.name, options(args.count, args.force))?;
    ctx.out.emit(&report, |t| render_report(t, &report))
}

/// `wt undo` when the newest operation is a push: restore the server
/// branch to its tip before the push (specs/tree/Undo.md §2.2).
fn undo_push(ctx: &Context, repo: &Repository, push: PushUndo, args: &UndoArgs) -> Result<()> {
    if args.count.unwrap_or(1) > 1 {
        return Err(CliError::usage(format!(
            "the newest operation is a push (\"{}\"); undo it on its own",
            push.op.description
        ))
        .with_hint("run `wt undo` without a count"));
    }
    let plan = block_on(remote::plan_push_undo(repo, &push))?.map_err(push_undo_error)?;
    let target = format!("{}/{}", push.remote, push.branch);
    if !ctx.out.is_json() {
        let mut t = Text::default();
        render_plan(&mut t, &plan);
        eprint!("{}", t.as_str());
    }
    confirm(
        args.yes,
        &format!(
            "move {target} on {} back from {} to {}",
            plan.server_tree,
            short(&push.new_tip),
            short(&plan.restore_to)
        ),
    )?;
    let report = block_on(remote::undo_push(repo, &plan, args.bypass_protection))?
        .map_err(push_undo_error)?;
    ctx.out.emit(&report, |t| {
        t.line(format_args!(
            "Undid {}: {} moved back from {} to {} on {}{}.",
            push.op.op.yellow(),
            target.green(),
            short(&report.old_tip),
            short(&report.new_tip),
            plan.server_tree,
            if report.bypass {
                " (protection bypassed)"
            } else {
                ""
            }
        ));
        t.line(format_args!(
            "Your local {} still has the snapshots: `wt push` publishes them again, `wt revert` takes them back.",
            push.branch
        ));
    })
}

/// What undoing the push will change, printed before the confirmation.
fn render_plan(t: &mut Text, plan: &PushUndoPlan) {
    let push = &plan.undo;
    t.line(format_args!(
        "Undoing {}: \"{}\"",
        push.op.op.yellow(),
        push.op.description
    ));
    t.line(format_args!(
        "  {}/{} on {} ({}) moves back: {} -> {}",
        push.remote,
        push.branch,
        plan.server_tree,
        plan.server,
        short(&push.new_tip),
        short(&plan.restore_to)
    ));
    if push.op.op == "force-push" {
        t.line(format_args!(
            "  {} comes back (your force push had replaced it)",
            short(&plan.restore_to)
        ));
    }
    if !plan.removed.is_empty() {
        t.line(format_args!(
            "  {} leave the server branch:",
            plural(plan.removed.len(), "snapshot", "snapshots")
        ));
        for s in &plan.removed {
            t.line(format_args!(
                "    {} {}",
                s.short_id().yellow(),
                subject(&s.message)
            ));
        }
    }
    t.line("  They stay recoverable from the server reflog (`wt reflog --remote`).");
}

/// Hints for the refusals of an undone push.
fn push_undo_error(err: SdkError) -> CliError {
    let hint = match &err {
        SdkError::PermissionDenied(_) => {
            "undoing a push rewinds the server branch and needs branch:force_push (admin role); \
             take the changes back with `wt revert` and `wt push` instead"
        }
        SdkError::BranchProtection(_) => {
            "the branch is protected: owners can retry with --bypass-protection; \
             otherwise take the changes back with `wt revert` and `wt push`"
        }
        SdkError::PushRejected(_) => {
            "run `wt pull`, then `wt revert` the changes you want to take back"
        }
        _ => return err.into(),
    };
    CliError::failure(err.to_string()).with_hint(hint)
}

fn short(id: &str) -> &str {
    id.get(..8).unwrap_or(id)
}

pub fn run_redo(ctx: &Context, args: RedoArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    if args.list {
        let ops = repo.redo_list(&tree.name)?;
        return ctx
            .out
            .emit(&ops, |t| render_list(t, &ops, "Nothing to redo."));
    }
    let report = repo.redo(&tree.name, options(args.count, args.force))?;
    ctx.out.emit(&report, |t| render_report(t, &report))
}

fn options(count: Option<u32>, force: bool) -> UndoOptions {
    UndoOptions {
        count: count.unwrap_or(1) as usize,
        force,
    }
}

fn render_list(t: &mut Text, ops: &[OpLogEntry], empty: &str) {
    if ops.is_empty() {
        t.line(empty);
        return;
    }
    let width = ops.iter().map(|o| o.op.len()).max().unwrap_or(0);
    for (i, op) in ops.iter().enumerate() {
        t.line(format_args!(
            "{:>3}  {}  {}  {}",
            i + 1,
            format!("{:<width$}", op.op).yellow(),
            op.description,
            format!("({})", ago(op.at)).dimmed()
        ));
    }
}

fn render_report(t: &mut Text, report: &UndoReport) {
    let verb = if report.action == "undo" {
        "Undid"
    } else {
        "Redid"
    };
    for op in &report.operations {
        t.line(format_args!(
            "{verb} {}: {}",
            op.op.yellow(),
            op.description
        ));
        for change in &op.changes {
            t.line(format_args!("  {}", change.describe()));
        }
    }
    t.line(format_args!(
        "Now on {} at {}.",
        report.branch.green(),
        short_or(report.head.as_deref(), "(no snapshots)")
    ));
    if !report.restored_paths.is_empty() {
        t.line(format_args!(
            "Restored stashed changes: {}",
            report.restored_paths.join(", ")
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::{Cli, Command};
    use clap::error::ErrorKind;
    use clap::Parser;

    fn parse(args: &[&str]) -> Command {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        Cli::try_parse_from(argv).expect("parse").command
    }

    #[test]
    fn parses_undo_and_redo() {
        let Command::Undo(a) = parse(&["undo"]) else {
            panic!()
        };
        assert!(a.count.is_none() && !a.list && !a.force);
        let Command::Undo(a) = parse(&["undo", "3", "--force"]) else {
            panic!()
        };
        assert_eq!((a.count, a.force), (Some(3), true));
        let Command::Undo(a) = parse(&["undo", "--list"]) else {
            panic!()
        };
        assert!(a.list);
        let Command::Redo(a) = parse(&["redo", "2"]) else {
            panic!()
        };
        assert_eq!(a.count, Some(2));
        let err = Cli::try_parse_from(["wt", "undo", "0"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ValueValidation);
        let err = Cli::try_parse_from(["wt", "undo", "--list", "2"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
        assert_eq!(options(None, false).count, 1);
        let Command::Undo(a) = parse(&["undo", "-y", "--bypass-protection"]) else {
            panic!()
        };
        assert!(a.yes && a.bypass_protection);
        let err = Cli::try_parse_from(["wt", "undo", "--list", "--yes"]).unwrap_err();
        assert_eq!(err.kind(), ErrorKind::ArgumentConflict);
    }

    fn push_plan(op: &str) -> PushUndoPlan {
        let entry = OpLogEntry {
            id: "op1".into(),
            at: chrono::Utc::now(),
            tree: "t".into(),
            kind: worktree_sdk::repo::OpKind::Op,
            op: op.into(),
            description: "push main to origin/main (1 snapshot)".into(),
            target: None,
            changes: vec![],
        };
        PushUndoPlan {
            undo: PushUndo {
                op: entry,
                tree: "t".into(),
                remote: "origin".into(),
                branch: "main".into(),
                old_tip: Some("aaaaaaaa1111".into()),
                new_tip: "bbbbbbbb2222".into(),
            },
            server: "http://localhost:7420".into(),
            server_tree: "acme/web".into(),
            restore_to: "aaaaaaaa1111".into(),
            removed: vec![],
        }
    }

    #[test]
    fn push_undo_plan_and_errors() {
        colored::control::set_override(false);
        let mut t = Text::default();
        render_plan(&mut t, &push_plan("force-push"));
        let out = t.as_str();
        assert!(
            out.contains(
                "origin/main on acme/web (http://localhost:7420) moves back: bbbbbbbb -> aaaaaaaa"
            ),
            "{out}"
        );
        assert!(out.contains("your force push had replaced it"), "{out}");
        assert!(out.contains("wt reflog --remote"), "{out}");

        let err = push_undo_error(SdkError::PermissionDenied(
            "requires branch:force_push".into(),
        ));
        assert!(format!("{err:?}").contains("wt revert"), "{err:?}");
        let err = push_undo_error(SdkError::BranchProtection("no_force_push".into()));
        assert!(
            format!("{err:?}").contains("--bypass-protection"),
            "{err:?}"
        );
        let err = push_undo_error(SdkError::PushRejected("moved".into()));
        assert!(format!("{err:?}").contains("wt pull"), "{err:?}");
    }
}
