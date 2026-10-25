//! `wt config`
//!
//! Values are layered: `<wt home>/config.toml` (user, `--global`) <
//! `.wt/config.toml` (worktree, the default) < `<tree>/.wt-tree/config.toml`
//! (tree, `--tree NAME`). `get` and `list` show the effective values for the
//! selected tree (`--tree`, else the tree containing the current directory).

use std::collections::BTreeMap;

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::config::{ConfigOrigin, ConfigScope};

use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show every effective configuration value
    List {
        /// Show which file set each value (default, user, worktree or tree)
        #[arg(long)]
        show_origin: bool,
    },
    /// Print one effective value (dotted key, e.g. user.name or remote.origin.url)
    Get {
        key: String,
        /// Also show which file set the value
        #[arg(long)]
        show_origin: bool,
    },
    /// Set one value in .wt/config.toml, the user config (--global) or a
    /// tree's .wt-tree/config.toml (global --tree NAME)
    Set {
        key: String,
        value: String,
        /// Write the user config (<wt home>/config.toml): [user], [signing],
        /// [secret_scanning], [alias] only (works outside a worktree)
        #[arg(long)]
        global: bool,
    },
    /// Remove one value from .wt/config.toml, the user config (--global) or
    /// a tree's .wt-tree/config.toml (global --tree NAME)
    Unset {
        key: String,
        /// Remove it from the user config
        #[arg(long)]
        global: bool,
    },
}

#[derive(Serialize)]
struct KeyValue<'a> {
    key: &'a str,
    value: &'a str,
}

#[derive(Serialize)]
struct WithOrigin<'a> {
    key: &'a str,
    value: &'a str,
    origin: &'a ConfigOrigin,
}

#[derive(Serialize)]
struct Written<'a> {
    key: &'a str,
    value: &'a str,
    scope: String,
}

#[derive(Serialize)]
struct Unset<'a> {
    key: &'a str,
    removed: bool,
    scope: String,
}

/// The file a write goes to: `--global`, global `--tree`, else the worktree.
fn scope(ctx: &Context, global: bool) -> Result<ConfigScope> {
    match (global, ctx.tree_flag()) {
        (true, Some(_)) => Err(CliError::usage("--global and --tree cannot be combined")),
        (true, None) => Ok(ConfigScope::User),
        (false, Some(tree)) => Ok(ConfigScope::Tree(tree.to_string())),
        (false, None) => Ok(ConfigScope::Worktree),
    }
}

fn scope_name(scope: &ConfigScope) -> String {
    match scope {
        ConfigScope::User => "user".into(),
        ConfigScope::Worktree => "worktree".into(),
        ConfigScope::Tree(t) => format!("tree {t}"),
    }
}

pub fn run(ctx: &Context, args: ConfigArgs) -> Result<()> {
    // --- cli-ux (ENGINEERING_PLAN §14.8): `--global` writes need no worktree
    // (e.g. `wt config set --global alias.st status` anywhere).
    if ctx.tree_flag().is_none()
        && matches!(
            worktree_sdk::Repository::open(ctx.cwd()),
            Err(worktree_sdk::SdkError::NotAWorktree)
        )
    {
        match &args.command {
            ConfigCommand::Set {
                key,
                value,
                global: true,
            } => {
                worktree_sdk::config::set_user_config_value(key, value)?;
                return ctx.out.emit(
                    &Written {
                        key,
                        value,
                        scope: "user".into(),
                    },
                    |t| t.line(format_args!("{} = {value}", key.cyan())),
                );
            }
            ConfigCommand::Unset { key, global: true } => {
                let removed = worktree_sdk::config::unset_user_config_value(key)?;
                return ctx.out.emit(
                    &Unset {
                        key,
                        removed,
                        scope: "user".into(),
                    },
                    |t| {
                        if removed {
                            t.line(format_args!("Unset {} (user)", key.cyan()));
                        } else {
                            t.line(format_args!(
                                "{} was not set in the user config",
                                key.cyan()
                            ));
                        }
                    },
                );
            }
            _ => {}
        }
    }
    // --- end cli-ux
    let mut repo = ctx.open()?;
    match args.command {
        ConfigCommand::List { show_origin } => {
            let tree = ctx.select_tree(&repo)?.name;
            let entries = repo.config_stack(Some(&tree))?.entries()?;
            if show_origin {
                let rows: Vec<WithOrigin> = entries
                    .iter()
                    .map(|e| WithOrigin {
                        key: &e.key,
                        value: &e.value,
                        origin: &e.origin,
                    })
                    .collect();
                return ctx.out.emit(&rows, |t| {
                    for e in &entries {
                        t.line(format_args!(
                            "{}\t{}={}",
                            e.origin.to_string().dimmed(),
                            e.key.cyan(),
                            e.value
                        ));
                    }
                });
            }
            let map: BTreeMap<&str, &str> = entries
                .iter()
                .map(|e| (e.key.as_str(), e.value.as_str()))
                .collect();
            ctx.out.emit(&map, |t| {
                for (key, value) in &map {
                    t.line(format_args!("{}={value}", key.cyan()));
                }
            })
        }
        ConfigCommand::Get { key, show_origin } => {
            let tree = ctx.select_tree(&repo)?.name;
            let entry = repo
                .config_stack(Some(&tree))?
                .get(&key)?
                .ok_or_else(|| CliError::failure(format!("config key not set: {key}")))?;
            if show_origin {
                ctx.out.emit(
                    &WithOrigin {
                        key: &entry.key,
                        value: &entry.value,
                        origin: &entry.origin,
                    },
                    |t| t.line(format_args!("{}\t{}", entry.origin, entry.value)),
                )
            } else {
                ctx.out.emit(
                    &KeyValue {
                        key: &key,
                        value: &entry.value,
                    },
                    |t| t.line(&entry.value),
                )
            }
        }
        ConfigCommand::Set { key, value, global } => {
            let scope = scope(ctx, global)?;
            repo.set_config_value(&scope, &key, &value)?;
            ctx.out.emit(
                &Written {
                    key: &key,
                    value: &value,
                    scope: scope_name(&scope),
                },
                |t| {
                    t.line(format_args!("{} = {value}", key.cyan()));
                },
            )
        }
        ConfigCommand::Unset { key, global } => {
            let scope = scope(ctx, global)?;
            let removed = repo.unset_config_value(&scope, &key)?;
            ctx.out.emit(
                &Unset {
                    key: &key,
                    removed,
                    scope: scope_name(&scope),
                },
                |t| {
                    if removed {
                        t.line(format_args!(
                            "Unset {} ({})",
                            key.cyan(),
                            scope_name(&scope)
                        ));
                    } else {
                        t.line(format_args!(
                            "{} was not set in the {} config",
                            key.cyan(),
                            scope_name(&scope)
                        ));
                    }
                },
            )
        }
    }
}
