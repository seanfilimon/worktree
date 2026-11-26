//! `wt completions SHELL` — print the shell's completion hook.
//!
//! The hook is dynamic ([`crate::complete`]): on every <Tab> the shell asks
//! `wt` itself (`WT_COMPLETE=SHELL wt -- WORDS...`), so commands, flags,
//! aliases and the names of branches, tags, remotes and trees are always
//! current — no regeneration after adding aliases or branches.

use std::io::Write;

use clap::Args;
use clap_complete::Shell;

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
Installing (load the hook when the shell starts):
  bash        echo 'source <(wt completions bash)' >> ~/.bashrc
  zsh         echo 'source <(wt completions zsh)' >> ~/.zshrc   (after compinit)
  fish        wt completions fish > ~/.config/fish/completions/wt.fish
  powershell  Add-Content $PROFILE 'wt completions powershell | Out-String | Invoke-Expression'
  elvish      echo 'eval (wt completions elvish | slurp)' >> ~/.config/elvish/rc.elv

The script calls back into wt on every completion, so branch, tag, remote
and tree names and aliases are always current. Saving the script to a file
(bash-completion directory, zsh $fpath as _wt) works too; regenerate it
after upgrading wt.";

pub fn run(_ctx: &Context, args: CompletionsArgs) -> Result<()> {
    let script = crate::complete::registration(args.shell, &crate::complete::completer_program())
        .map_err(|e| CliError::failure(format!("cannot generate the script: {e}")))?;
    let mut stdout = std::io::stdout().lock();
    match stdout.write_all(&script).and_then(|()| stdout.flush()) {
        Err(e) if e.kind() != std::io::ErrorKind::BrokenPipe => {
            Err(CliError::failure(format!("cannot write the script: {e}")))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use crate::commands::server_common::testing::{parse, parse_err};

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
