//! `wt rebase` — replay the current branch's snapshots onto another base
//! (ENGINEERING_PLAN §14.5).

use clap::{ArgGroup, Args};
use worktree_sdk::repo::{RebaseOptions, RewriteOp};

use crate::commands::cherry_pick::{emit_abort, emit_outcome};
use crate::context::Context;
use crate::error::Result;

/// `wt rebase ONTO [--force] | --continue | --skip | --abort`
#[derive(Debug, Args)]
#[command(
    group(ArgGroup::new("operation").required(true).args(["onto", "resume", "abort", "skip"])),
    after_long_help = "\
The branch's first-parent snapshots since it forked from ONTO are re-applied
on top of ONTO as new snapshots (new ids; author, message, dependencies and
`reverts` kept; signed again when signing is configured) and the branch is
moved to the result. Snapshots whose changes ONTO already has are dropped.
Needs a clean working tree.

Snapshots that a remote-tracking ref of the branch (`origin/BRANCH`) already
contains are pushed history: rebasing them is refused unless `--force`.

Conflicts stop the rebase like `wt merge`: resolve them (`wt merge status`,
`wt merge resolve PATH --ours|--theirs|--file F`), then run `wt rebase
--continue`, `--skip` (drop that snapshot) or `--abort` (return the branch
and working tree to their state before the rebase)."
)]
pub struct RebaseArgs {
    /// Revision to replay the branch onto (branch, origin/BRANCH, id, ...)
    #[arg(value_name = "ONTO")]
    pub onto: Option<String>,

    /// Rewrite snapshots even if they were already pushed
    #[arg(long, conflicts_with_all = ["resume", "abort", "skip"])]
    pub force: bool,

    /// Finish the stopped step after resolving its conflicts and replay the rest
    #[arg(long = "continue")]
    pub resume: bool,

    /// Drop the stopped step and replay the rest
    #[arg(long)]
    pub skip: bool,

    /// Abandon the rebase and restore the branch and working tree
    #[arg(long)]
    pub abort: bool,
}

pub fn run(ctx: &Context, args: RebaseArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let outcome = if args.abort {
        repo.rebase_abort(&tree.name)?;
        return emit_abort(ctx, &tree.name, RewriteOp::Rebase);
    } else if args.resume {
        repo.rebase_continue(&tree.name)?
    } else if args.skip {
        repo.rebase_skip(&tree.name)?
    } else {
        let onto = args.onto.unwrap_or_default();
        repo.rebase(&tree.name, &onto, &RebaseOptions { force: args.force })?
    };
    emit_outcome(ctx, &repo, &outcome)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::*;

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(flatten)]
        args: RebaseArgs,
    }

    fn parse(args: &[&str]) -> std::result::Result<RebaseArgs, clap::Error> {
        let argv = std::iter::once("wt").chain(args.iter().copied());
        Harness::try_parse_from(argv).map(|h| h.args)
    }

    #[test]
    fn parses_operations() {
        let a = parse(&["main", "--force"]).unwrap();
        assert_eq!(a.onto.as_deref(), Some("main"));
        assert!(a.force);
        assert!(parse(&["--continue"]).unwrap().resume);
        assert!(parse(&["--skip"]).unwrap().skip);
        assert!(parse(&["--abort"]).unwrap().abort);
        assert!(parse(&[]).is_err());
        assert!(parse(&["main", "--abort"]).is_err());
        assert!(parse(&["--continue", "--force"]).is_err());
    }
}
