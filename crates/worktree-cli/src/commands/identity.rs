//! `wt identity` — Snapshot signing keys: generate, show, register, list, revoke.
//!
//! v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct IdentityArgs {}

pub fn run(_ctx: &Context, _args: IdentityArgs) -> Result<()> {
    Err(CliError::failure("`wt identity` is not implemented yet"))
}
