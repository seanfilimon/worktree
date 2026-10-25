//! `wt api` — Call any REST API endpoint of the server with your credentials.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct ApiArgs {}

pub fn run(_ctx: &Context, _args: ApiArgs) -> Result<()> {
    Err(CliError::failure("`wt api` is not implemented yet"))
}
