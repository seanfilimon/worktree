//! `wt session` — Your sign-in sessions.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct SessionArgs {
    #[command(subcommand)]
    pub command: SessionCommand,
}

#[derive(Debug, Subcommand)]
pub enum SessionCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: SessionArgs) -> Result<()> {
    Err(CliError::failure("`wt session` is not implemented yet"))
}
