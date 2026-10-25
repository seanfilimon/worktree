//! `wt token` — Your personal API tokens.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct TokenArgs {
    #[command(subcommand)]
    pub command: TokenCommand,
}

#[derive(Debug, Subcommand)]
pub enum TokenCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: TokenArgs) -> Result<()> {
    Err(CliError::failure("`wt token` is not implemented yet"))
}
