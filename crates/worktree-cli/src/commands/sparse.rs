//! `wt sparse` — Sparse checkout: check out and fetch only the paths you need.
//!
//! v0.7 stub (docs/ENGINEERING_PLAN.md §15): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct SparseArgs {}

pub fn run(_ctx: &Context, _args: SparseArgs) -> Result<()> {
    Err(CliError::failure("`wt sparse` is not implemented yet"))
}
