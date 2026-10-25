//! `wt todo` — TODOs created from snapshot dependencies: list, show, claim, complete, cancel.
//!
//! v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct TodoArgs {}

pub fn run(_ctx: &Context, _args: TodoArgs) -> Result<()> {
    Err(CliError::failure("`wt todo` is not implemented yet"))
}
