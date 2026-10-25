//! `wt search` — Search tenants, trees, branches and accounts on the server.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct SearchArgs {
    #[command(subcommand)]
    pub command: SearchCommand,
}

#[derive(Debug, Subcommand)]
pub enum SearchCommand {
    /// Not implemented yet
    List,
}

pub fn run(ctx: &Context, _args: SearchArgs) -> Result<()> {
    ctx.rest()?;
    Err(CliError::failure("`wt search` is not implemented yet"))
}
