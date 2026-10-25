//! `wt doctor` — Diagnose the local setup: config, credentials, server, daemon.
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct DoctorArgs {}

pub fn run(_ctx: &Context, _args: DoctorArgs) -> Result<()> {
    Err(CliError::failure("`wt doctor` is not implemented yet"))
}
