//! `wt protect` — Branch protection rules of a server tree.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct ProtectArgs {
    #[command(subcommand)]
    pub command: ProtectCommand,
}

#[derive(Debug, Subcommand)]
pub enum ProtectCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: ProtectArgs) -> Result<()> {
    Err(CliError::failure("`wt protect` is not implemented yet"))
}
