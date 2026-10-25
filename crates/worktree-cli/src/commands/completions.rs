//! `wt completions` — Print shell completion scripts (bash, zsh, fish, powershell, elvish).
//!
//! v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
//! this module.

use clap::Args;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct CompletionsArgs {}

pub fn run(_ctx: &Context, _args: CompletionsArgs) -> Result<()> {
    Err(CliError::failure("`wt completions` is not implemented yet"))
}
