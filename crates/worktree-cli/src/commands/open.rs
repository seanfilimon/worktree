//! `wt open` — Open the current tree (or a merge request, snapshot or file) in the admin panel.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct OpenArgs {}

pub fn run(_ctx: &Context, _args: OpenArgs) -> Result<()> {
    Err(CliError::failure("`wt open` is not implemented yet"))
}
