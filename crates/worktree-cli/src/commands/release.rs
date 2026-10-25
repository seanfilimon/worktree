//! `wt release` — Releases of a server tree: list, create, show, edit, delete, assets.
//!
//! v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct ReleaseArgs {}

pub fn run(_ctx: &Context, _args: ReleaseArgs) -> Result<()> {
    Err(CliError::failure("`wt release` is not implemented yet"))
}
