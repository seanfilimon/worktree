//! `wt license` — Path license rules of a server tree.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct LicenseArgs {
    #[command(subcommand)]
    pub command: LicenseCommand,
}

#[derive(Debug, Subcommand)]
pub enum LicenseCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: LicenseArgs) -> Result<()> {
    Err(CliError::failure("`wt license` is not implemented yet"))
}
