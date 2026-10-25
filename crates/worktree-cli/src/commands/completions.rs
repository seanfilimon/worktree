//! `wt completions SHELL` — print a shell completion script (clap_complete).
//!
//! The script completes every command, subcommand and flag, plus the
//! aliases configured when it was generated (regenerate it after adding
//! aliases).

use std::io::Write;

use clap::{Args, CommandFactory};
use clap_complete::Shell;

use crate::cli::Cli;
use crate::context::Context;
use crate::error::{CliError, Result};

#[derive(Debug, Args)]
#[command(after_long_help = INSTALL)]
pub struct CompletionsArgs {
    /// Shell to generate the script for
    #[arg(value_enum, value_name = "SHELL")]
    pub shell: Shell,
}

const INSTALL: &str = "\
Installing:
  bash        wt completions bash > ~/.local/share/bash-completion/completions/wt
  zsh         wt completions zsh > \"${fpath[1]}/_wt\"   (then restart zsh)
  fish        wt completions fish > ~/.config/fish/completions/wt.fish
  powershell  wt completions powershell >> $PROFILE   (or dot-source a saved copy)
  elvish      wt completions elvish >> ~/.config/elvish/rc.elv

Aliases from [alias] are included as of generation time; regenerate the
script after adding or removing aliases.";

pub fn run(ctx: &Context, args: CompletionsArgs) -> Result<()> {
    let aliases = worktree_sdk::config::load_aliases(ctx.cwd());
    let script = script(args.shell, &aliases);
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(&script).and_then(|()| stdout.flush()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(CliError::failure(format!("cannot write the script: {e}")))
        }
        _ => Ok(()),
    }
}

/// The completion script for `shell`; `aliases` (name → command line) are
/// completed as commands (built-in names are never shadowed).
pub fn script(shell: Shell, aliases: &std::collections::BTreeMap<String, String>) -> Vec<u8> {
    let mut command = Cli::command();
    for (name, line) in aliases {
        if crate::alias::is_builtin(&command, name) {
            continue;
        }
        // clap's builder wants 'static names (no "string" feature); the
        // process ends right after printing the script.
        let name: &'static str = Box::leak(name.clone().into_boxed_str());
        command = command.subcommand(
            clap::Command::new(name)
                .about(format!("Alias for `wt {line}`"))
                .arg(
                    clap::Arg::new("args")
                        .num_args(..)
                        .trailing_var_arg(true)
                        .allow_hyphen_values(true),
                ),
        );
    }
    let mut out = Vec::new();
    clap_complete::generate(shell, &mut command, "wt", &mut out);
    out
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use clap::ValueEnum;

    use super::*;
    use crate::commands::server_common::testing::{parse, parse_err};

    #[test]
    fn every_shell_gets_a_non_empty_script() {
        for shell in Shell::value_variants() {
            let text = String::from_utf8(script(*shell, &BTreeMap::new())).expect("utf-8");
            assert!(text.len() > 1000, "{shell}: {} bytes", text.len());
            assert!(text.contains("wt"), "{shell}");
            assert!(text.contains("snapshot"), "{shell} lacks subcommands");
        }
    }

    #[test]
    fn aliases_are_completed_but_never_shadow_builtins() {
        let aliases: BTreeMap<String, String> = [
            ("lgx".to_string(), "log --oneline".to_string()),
            ("status".to_string(), "log".to_string()),
        ]
        .into();
        let text = String::from_utf8(script(Shell::Bash, &aliases)).expect("utf-8");
        assert!(text.contains("lgx"), "alias missing");
        let fish = String::from_utf8(script(Shell::Fish, &aliases)).expect("utf-8");
        assert!(fish.contains("Alias for `wt log --oneline`"), "{fish}");
        assert!(!fish.contains("Alias for `wt log`"), "status was shadowed");
    }

    #[test]
    fn parses_shell_names() {
        for name in ["bash", "zsh", "fish", "powershell", "elvish"] {
            let crate::cli::Command::Completions(a) = parse(&["completions", name]) else {
                panic!()
            };
            assert_eq!(a.shell.to_string(), name);
        }
        assert_eq!(
            parse_err(&["completions", "tcsh"]),
            clap::error::ErrorKind::InvalidValue
        );
        assert_eq!(
            parse_err(&["completions"]),
            clap::error::ErrorKind::MissingRequiredArgument
        );
    }
}
