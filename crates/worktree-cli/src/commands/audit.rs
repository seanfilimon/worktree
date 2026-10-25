//! `wt audit` — Query the server audit log.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct AuditArgs {
    #[command(subcommand)]
    pub command: AuditCommand,
}

#[derive(Debug, Subcommand)]
pub enum AuditCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: AuditArgs) -> Result<()> {
    Err(CliError::failure("`wt audit` is not implemented yet"))
}
