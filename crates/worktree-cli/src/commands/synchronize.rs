//! `wt sync` — Fetch, integrate (merge or rebase) and push in one step; manage the offline queue.
//!
//! v0.7 stub (docs/ENGINEERING_PLAN.md §15): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct SyncArgs {}

pub fn run(_ctx: &Context, _args: SyncArgs) -> Result<()> {
    Err(CliError::failure("`wt sync` is not implemented yet"))
}
