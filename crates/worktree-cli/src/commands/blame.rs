//! `wt blame` — Show which snapshot last changed each line of a file.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct BlameArgs {}

pub fn run(_ctx: &Context, _args: BlameArgs) -> Result<()> {
    Err(CliError::failure("`wt blame` is not implemented yet"))
}
