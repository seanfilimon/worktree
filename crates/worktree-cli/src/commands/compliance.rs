//! `wt compliance` — Secret scanning and license compliance: scan, findings, license audit.
//!
//! v0.8 stub (docs/ENGINEERING_PLAN.md §16): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct ComplianceArgs {}

pub fn run(_ctx: &Context, _args: ComplianceArgs) -> Result<()> {
    Err(CliError::failure("`wt compliance` is not implemented yet"))
}
