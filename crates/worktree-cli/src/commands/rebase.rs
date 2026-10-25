//! `wt rebase` — Replay the current branch's snapshots onto another base.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct RebaseArgs {}

pub fn run(_ctx: &Context, _args: RebaseArgs) -> Result<()> {
    Err(CliError::failure("`wt rebase` is not implemented yet"))
}
