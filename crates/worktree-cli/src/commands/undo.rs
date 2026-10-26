//! `wt undo [N]`, `wt undo --list` and `wt redo [N]` — the local operation
//! log (ENGINEERING_PLAN §16.10, specs/tree/Undo.md).

use clap::Args;
use colored::Colorize;
use worktree_sdk::repo::{OpLogEntry, UndoOptions, UndoReport};

use crate::context::Context;
use crate::error::Result;
use crate::output::format::{ago, short_or};
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
Pushes are not undone on the server (see `wt push --force`).")]
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
    let report = repo.undo(&tree.name, options(args.count, args.force))?;
    ctx.out.emit(&report, |t| render_report(t, &report))
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
    }
}
