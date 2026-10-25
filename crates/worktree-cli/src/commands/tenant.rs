//! `wt tenant` — Manage tenants (organizations) and their members on the server.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct TenantArgs {
    #[command(subcommand)]
    pub command: TenantCommand,
}

#[derive(Debug, Subcommand)]
pub enum TenantCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: TenantArgs) -> Result<()> {
    Err(CliError::failure("`wt tenant` is not implemented yet"))
}
