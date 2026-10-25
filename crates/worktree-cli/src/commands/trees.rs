//! `wt trees` — Manage trees on the server: create, settings, branches, tags, compare, stats.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct TreesArgs {
    #[command(subcommand)]
    pub command: TreesCommand,
}

#[derive(Debug, Subcommand)]
pub enum TreesCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: TreesArgs) -> Result<()> {
    Err(CliError::failure("`wt trees` is not implemented yet"))
}
