//! `wt mirror` — Live Git mirrors of a server tree: list, add, update, remove, sync now.
//!
//! v0.7 stub (docs/ENGINEERING_PLAN.md §15): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct MirrorArgs {}

pub fn run(_ctx: &Context, _args: MirrorArgs) -> Result<()> {
    Err(CliError::failure("`wt mirror` is not implemented yet"))
}
