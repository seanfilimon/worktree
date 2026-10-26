//! `wt fork` — Fork a server tree into another tenant; list forks; sync a fork with its upstream.
//!
//! v0.8 stub (docs/ENGINEERING_PLAN.md §16): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct ForkArgs {}

pub fn run(_ctx: &Context, _args: ForkArgs) -> Result<()> {
    Err(CliError::failure("`wt fork` is not implemented yet"))
}
