//! `wt access` — Declarative access config (.wt/access): validate, plan, apply, pull.
//!
//! v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct AccessArgs {}

pub fn run(_ctx: &Context, _args: AccessArgs) -> Result<()> {
    Err(CliError::failure("`wt access` is not implemented yet"))
}
