//! `wt undo` — Undo the last local operations (snapshot, merge, rebase, branch moves) from the operation log.
//!
//! v0.8 stub (docs/ENGINEERING_PLAN.md §16): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct UndoArgs {}

pub fn run(_ctx: &Context, _args: UndoArgs) -> Result<()> {
    Err(CliError::failure("`wt undo` is not implemented yet"))
}
