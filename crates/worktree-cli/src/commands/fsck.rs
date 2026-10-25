//! `wt fsck` — Verify the integrity of the local store and history.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct FsckArgs {}

pub fn run(_ctx: &Context, _args: FsckArgs) -> Result<()> {
    Err(CliError::failure("`wt fsck` is not implemented yet"))
}
