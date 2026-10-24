//! `wt config`

use std::collections::BTreeMap;

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show every configuration value
    List,
    /// Print one value (dotted key, e.g. user.name or remote.origin.url)
    Get { key: String },
    /// Set one value
    Set { key: String, value: String },
}

#[derive(Serialize)]
struct KeyValue<'a> {
    key: &'a str,
    value: &'a str,
}

pub fn run(ctx: &Context, args: ConfigArgs) -> Result<()> {
    let mut repo = ctx.open()?;
    match args.command {
        ConfigCommand::List => {
            let entries: BTreeMap<String, String> = repo.config().entries().into_iter().collect();
            ctx.out.emit(&entries, |t| {
                for (key, value) in &entries {
                    t.line(format_args!("{}={value}", key.cyan()));
                }
            })
        }
        ConfigCommand::Get { key } => {
            let value = repo
                .config()
                .get(&key)
                .ok_or_else(|| CliError::failure(format!("config key not set: {key}")))?;
            ctx.out.emit(
                &KeyValue {
                    key: &key,
                    value: &value,
                },
                |t| t.line(&value),
            )
        }
        ConfigCommand::Set { key, value } => {
            let mut config = repo.config().clone();
            config.set(&key, &value)?;
            repo.set_config(config)?;
            ctx.out.emit(
                &KeyValue {
                    key: &key,
                    value: &value,
                },
                |t| {
                    t.line(format_args!("{} = {value}", key.cyan()));
                },
            )
        }
    }
}
