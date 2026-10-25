//! `wt ci` — CI check results: report a status, show checks of a snapshot or branch.
//!
//! v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct CiArgs {}

pub fn run(_ctx: &Context, _args: CiArgs) -> Result<()> {
    Err(CliError::failure("`wt ci` is not implemented yet"))
}
