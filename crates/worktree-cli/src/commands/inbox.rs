//! `wt inbox` — Your notifications: list, read, mark done, watch settings.
//!
//! v0.7 stub (docs/ENGINEERING_PLAN.md §15): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct InboxArgs {}

pub fn run(_ctx: &Context, _args: InboxArgs) -> Result<()> {
    Err(CliError::failure("`wt inbox` is not implemented yet"))
}
