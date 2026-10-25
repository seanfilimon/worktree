//! `wt grant` — Per-tree access grants for users, teams and service accounts.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct GrantArgs {
    #[command(subcommand)]
    pub command: GrantCommand,
}

#[derive(Debug, Subcommand)]
pub enum GrantCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: GrantArgs) -> Result<()> {
    Err(CliError::failure("`wt grant` is not implemented yet"))
}
