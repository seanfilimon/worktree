//! `wt sa` — Service accounts and their keys.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct SaArgs {
    #[command(subcommand)]
    pub command: SaCommand,
}

#[derive(Debug, Subcommand)]
pub enum SaCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: SaArgs) -> Result<()> {
    Err(CliError::failure("`wt sa` is not implemented yet"))
}
