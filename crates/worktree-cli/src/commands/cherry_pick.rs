//! `wt cherry-pick` — Apply the changes of existing snapshots onto the current branch.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct CherryPickArgs {}

pub fn run(_ctx: &Context, _args: CherryPickArgs) -> Result<()> {
    Err(CliError::failure("`wt cherry-pick` is not implemented yet"))
}
