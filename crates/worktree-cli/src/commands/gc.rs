//! `wt gc` — Remove unreachable local objects and compact the local store.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct GcArgs {}

pub fn run(_ctx: &Context, _args: GcArgs) -> Result<()> {
    Err(CliError::failure("`wt gc` is not implemented yet"))
}
