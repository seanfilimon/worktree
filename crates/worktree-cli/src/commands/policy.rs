//! `wt policy` — Access policies (allow/deny statements) of a tenant.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct PolicyArgs {
    #[command(subcommand)]
    pub command: PolicyCommand,
}

#[derive(Debug, Subcommand)]
pub enum PolicyCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: PolicyArgs) -> Result<()> {
    Err(CliError::failure("`wt policy` is not implemented yet"))
}
