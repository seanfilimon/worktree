//! `wt admin` — Server administration: accounts, system information, statistics.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct AdminArgs {
    #[command(subcommand)]
    pub command: AdminCommand,
}

#[derive(Debug, Subcommand)]
pub enum AdminCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: AdminArgs) -> Result<()> {
    Err(CliError::failure("`wt admin` is not implemented yet"))
}
