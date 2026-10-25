//! `wt mfa` — Two-factor authentication (TOTP) and recovery codes.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct MfaArgs {
    #[command(subcommand)]
    pub command: MfaCommand,
}

#[derive(Debug, Subcommand)]
pub enum MfaCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: MfaArgs) -> Result<()> {
    Err(CliError::failure("`wt mfa` is not implemented yet"))
}
