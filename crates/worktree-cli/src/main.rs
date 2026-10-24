//! `wt` — the W0rkTree command-line client.
//!
//! A thin layer over [`worktree_sdk`]: argument parsing ([`cli`]), command
//! implementations ([`commands`]), and an output layer that renders either
//! human-readable text or one JSON document per invocation ([`output`]).
//!
//! Exit codes: 0 success, 1 error, 2 usage error, 3 merge conflicts.

mod cli;
mod commands;
mod context;
mod error;
mod output;
mod paths;

use std::io::IsTerminal;
use std::process::ExitCode;

use clap::Parser;

use crate::cli::Cli;
use crate::context::Context;
use crate::error::CliError;

fn main() -> ExitCode {
    let cli = Cli::parse();
    init_tracing();
    configure_color(cli.global.no_color);

    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            err.report();
            ExitCode::from(err.exit_code())
        }
    }
}

fn run(cli: Cli) -> Result<(), CliError> {
    if let Some(dir) = &cli.global.dir {
        std::env::set_current_dir(dir)
            .map_err(|e| CliError::failure(format!("cannot change to '{}': {e}", dir.display())))?;
    }
    let ctx = Context::new(&cli.global)?;
    commands::run(&ctx, cli.command)
}

/// Diagnostics go to stderr and are off unless `WT_LOG` (an `EnvFilter`
/// directive such as `debug` or `worktree_sdk=trace`) is set.
fn init_tracing() {
    if let Ok(filter) = tracing_subscriber::EnvFilter::try_from_env("WT_LOG") {
        tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_writer(std::io::stderr)
            .init();
    }
}

/// Colors are used only when stdout is a terminal and neither `--no-color`
/// nor `NO_COLOR` asks otherwise.
fn configure_color(no_color: bool) {
    let disabled = no_color || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
    colored::control::set_override(!disabled && std::io::stdout().is_terminal());
}
