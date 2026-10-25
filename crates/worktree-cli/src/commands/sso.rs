//! `wt sso` — Single sign-on (OpenID Connect) configuration.
//!
//! STUB: implemented by the cli-iam workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct SsoArgs {
    #[command(subcommand)]
    pub command: SsoCommand,
}

#[derive(Debug, Subcommand)]
pub enum SsoCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: SsoArgs) -> Result<()> {
    Err(CliError::failure("`wt sso` is not implemented yet"))
}
