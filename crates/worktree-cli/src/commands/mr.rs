//! `wt mr` — Merge requests: list, open, review, approve, merge, close.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct MrArgs {
    #[command(subcommand)]
    pub command: MrCommand,
}

#[derive(Debug, Subcommand)]
pub enum MrCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: MrArgs) -> Result<()> {
    Err(CliError::failure("`wt mr` is not implemented yet"))
}
