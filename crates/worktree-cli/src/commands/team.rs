//! `wt team` — Teams within a tenant and their members.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct TeamArgs {
    #[command(subcommand)]
    pub command: TeamCommand,
}

#[derive(Debug, Subcommand)]
pub enum TeamCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: TeamArgs) -> Result<()> {
    Err(CliError::failure("`wt team` is not implemented yet"))
}
