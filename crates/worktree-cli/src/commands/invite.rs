//! `wt invite` — Tenant invitations.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct InviteArgs {
    #[command(subcommand)]
    pub command: InviteCommand,
}

#[derive(Debug, Subcommand)]
pub enum InviteCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: InviteArgs) -> Result<()> {
    Err(CliError::failure("`wt invite` is not implemented yet"))
}
