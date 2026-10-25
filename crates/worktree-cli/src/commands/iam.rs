//! `wt iam` — Inspect access: simulate requests, effective permissions, action catalog.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct IamArgs {
    #[command(subcommand)]
    pub command: IamCommand,
}

#[derive(Debug, Subcommand)]
pub enum IamCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: IamArgs) -> Result<()> {
    Err(CliError::failure("`wt iam` is not implemented yet"))
}
