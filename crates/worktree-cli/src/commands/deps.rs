//! `wt deps` — Dependencies declared by snapshots: status and graph.
//!
//! v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct DepsArgs {}

pub fn run(_ctx: &Context, _args: DepsArgs) -> Result<()> {
    Err(CliError::failure("`wt deps` is not implemented yet"))
}
