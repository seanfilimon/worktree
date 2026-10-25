//! `wt stash` — Set working changes aside and restore them later.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct StashArgs {}

pub fn run(_ctx: &Context, _args: StashArgs) -> Result<()> {
    Err(CliError::failure("`wt stash` is not implemented yet"))
}
