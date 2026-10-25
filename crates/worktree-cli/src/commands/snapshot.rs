//! `wt snapshot`

use clap::Args;
use colored::Colorize;
use worktree_sdk::{Snapshot, SnapshotOptions};

use crate::context::Context;
use crate::error::Result;
use crate::output::format::subject;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct SnapshotArgs {
    /// Snapshot message
    #[arg(short, long, value_name = "MSG")]
    pub message: String,

    /// Create a snapshot even if nothing changed
    #[arg(long)]
    pub allow_empty: bool,

    /// Skip the pre-snapshot secret scan
    #[arg(long)]
    pub no_scan: bool,

    // --- local-config (ENGINEERING_PLAN §13.9): hooks ---------------------
    /// Skip the pre-snapshot and post-snapshot hooks
    #[arg(long)]
    pub no_verify: bool,
    // --- end local-config --------------------------------------------------
}

pub fn run(ctx: &Context, args: SnapshotArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let snapshot = repo.snapshot(
        &tree.name,
        SnapshotOptions {
            message: args.message,
            allow_empty: args.allow_empty,
            skip_secret_scan: args.no_scan,
            // local-config (§13.9): `--no-verify` skips hooks.
            skip_hooks: args.no_verify,
            ..SnapshotOptions::default()
        },
    )?;
    ctx.out.emit(&snapshot, |t| snapshot_line(t, &snapshot))
}

/// `[main 1a2b3c4d] message` (shared by revert/merge).
pub fn snapshot_line(t: &mut Text, snapshot: &Snapshot) {
    t.line(format_args!(
        "[{} {}] {}",
        snapshot.branch.green(),
        snapshot.short_id().yellow(),
        subject(&snapshot.message)
    ));
}
