//! `wt attr` — ABAC attributes of tenants, accounts and trees: show, set, unset.
//!
//! v0.8 stub (docs/ENGINEERING_PLAN.md §16): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct AttrArgs {}

pub fn run(_ctx: &Context, _args: AttrArgs) -> Result<()> {
    Err(CliError::failure("`wt attr` is not implemented yet"))
}
