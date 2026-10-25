//! `wt events` — Stream live server events.
//!
//! STUB: implemented by the cli-collab workstream (docs/ENGINEERING_PLAN.md §12).

use clap::{Args, Subcommand};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct EventsArgs {
    #[command(subcommand)]
    pub command: EventsCommand,
}

#[derive(Debug, Subcommand)]
pub enum EventsCommand {
    /// Not implemented yet
    List,
}

pub fn run(_ctx: &Context, _args: EventsArgs) -> Result<()> {
    Err(CliError::failure("`wt events` is not implemented yet"))
}
