//! `wt role` — Custom roles of a tenant.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct RoleArgs {
    #[command(subcommand)]
    pub command: RoleCommand,
}

#[derive(Debug, Subcommand)]
pub enum RoleCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: RoleArgs) -> Result<()> {
    Err(CliError::failure("`wt role` is not implemented yet"))
}
