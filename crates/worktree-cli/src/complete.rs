//! Dynamic shell completion (CliErgonomics spec §2).
//!
//! The scripts printed by `wt completions SHELL` are small shims that call
//! back into `wt` on every <Tab>: the shell runs
//! `WT_COMPLETE=SHELL wt -- <words of the command line>` and `wt` answers
//! with the candidates (clap_complete's `CompleteEnv` engine, intercepted
//! at the very start of `main`). Commands, subcommands, flags and
//! enumerated values come from the clap definition, aliases from the
//! `[alias]` tables as they are *now*, and the arguments that name refs
//! carry an [`ArgValueCompleter`] from this module:
//!
//! | Kind | Candidates |
//! |---|---|
//! | [`Kind::Branch`] | non-archived local branches |
//! | [`Kind::ArchivedBranch`] | archived branches (`wt branch restore`) |
//! | [`Kind::Tag`] | tags |
//! | [`Kind::Rev`] | non-archived branches, tags, remote-tracking `REMOTE/BRANCH` |
//! | [`Kind::Remote`] | remotes of `.wt/config.toml` |
//! | [`Kind::Tree`] | trees of the worktree |
//!
//! Completion must be fast and invisible: candidates are read straight from
//! the ref directories through the SDK's name listings (no snapshot, tip or
//! working-directory reads), without taking a lock, touching the network or
//! the daemon, and nothing but candidates is ever written to stdout. Outside
//! a worktree (or with an unreadable one) ref completion is simply empty.
//! The worktree and tree are found like a real run would: from the shell's
//! current directory, or the `-C DIR` / `--tree NAME` already typed on the
//! command line.

use std::collections::{BTreeMap, HashSet};
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use clap::CommandFactory;
use clap_complete::engine::{
    ArgValueCompleter, CompletionCandidate, PathCompleter, ValueCompleter,
};
use clap_complete::env::Shells;
use clap_complete::{CompleteEnv, Shell};
use worktree_sdk::Repository;

use crate::cli::Cli;

/// Environment variable that switches `wt` into completion mode; its value
/// names the shell (`bash`, `zsh`, `fish`, `powershell`, `elvish`). Empty or
/// `0` means a normal run.
pub const ENV_VAR: &str = "WT_COMPLETE";

/// Answer a completion request and exit, when `WT_COMPLETE` is set; return
/// otherwise. Must run before anything is written to stdout.
pub fn intercept() {
    CompleteEnv::with_factory(command)
        .var(ENV_VAR)
        .bin("wt")
        .complete();
}

/// The command tree completed by the engine: the clap definition plus the
/// configured aliases (read from the `-C` directory, else the current one).
fn command() -> clap::Command {
    let dir = Scope::from_process().map(|s| s.dir).unwrap_or_default();
    with_aliases(Cli::command(), &worktree_sdk::config::load_aliases(&dir))
}

/// `command` plus one subcommand per alias (name → command line); an alias
/// named like a built-in command is left out (it never runs either).
pub fn with_aliases(
    mut command: clap::Command,
    aliases: &BTreeMap<String, String>,
) -> clap::Command {
    for (name, line) in aliases {
        if crate::alias::is_builtin(&command, name) {
            continue;
        }
        // clap's builder wants 'static names (no "string" feature); the
        // process ends right after answering the completion request.
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
    command
}

/// The registration script for `shell`: it calls `completer` (the program
/// to run, normally `wt`) with `WT_COMPLETE` set on every completion.
pub fn registration(shell: Shell, completer: &str) -> std::io::Result<Vec<u8>> {
    let name = shell.to_string();
    let shells = Shells::builtins();
    let env_shell = shells.completer(&name).ok_or_else(|| {
        std::io::Error::other(format!("no dynamic completion support for {name}"))
    })?;
    let mut out = Vec::new();
    env_shell.write_registration(ENV_VAR, "wt", "wt", completer, &mut out)?;
    if shell == Shell::Zsh {
        // Installed as `_wt` in $fpath, the file is the body of the
        // autoloaded `_wt`: complete right away on that first call (later
        // calls go straight to the function registered with compdef).
        out.extend_from_slice(ZSH_AUTOLOAD_TRAILER.as_bytes());
    }
    Ok(out)
}

const ZSH_AUTOLOAD_TRAILER: &str = r#"
if [[ "${funcstack[1]}" == "_wt" ]]; then
    _clap_dynamic_completer_wt "$@"
fi
"#;

/// What a completer offers (see the module docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Branch,
    ArchivedBranch,
    Tag,
    Rev,
    Remote,
    Tree,
}

/// Where a completion request runs: the directory the command would run in
/// and the `--tree` it would use.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub dir: PathBuf,
    pub tree: Option<String>,
}

impl Scope {
    /// The current directory plus the `-C` / `--tree` options among the
    /// words being completed (the process arguments after `--`).
    fn from_process() -> Option<Self> {
        let cwd = std::env::current_dir().ok()?;
        let args: Vec<OsString> = std::env::args_os().collect();
        let words = args
            .iter()
            .position(|a| a == "--")
            .map_or(&[][..], |i| &args[i + 1..]);
        Some(Self::from_words(cwd, words))
    }

    /// `words` is the command line being completed, program name first.
    /// `-C DIR` (also `-CDIR`, `-C=DIR`) is resolved against `cwd`; the last
    /// occurrence of each option wins. A `--` ends the options.
    pub fn from_words(cwd: PathBuf, words: &[OsString]) -> Self {
        let mut scope = Scope {
            dir: cwd.clone(),
            tree: None,
        };
        let mut words = words.iter().skip(1).map(|w| w.to_str());
        while let Some(word) = words.next() {
            let Some(word) = word else { continue };
            if word == "--" {
                break;
            } else if word == "-C" {
                if let Some(Some(dir)) = words.next() {
                    scope.dir = cwd.join(dir);
                }
            } else if let Some(dir) = word.strip_prefix("-C") {
                scope.dir = cwd.join(dir.strip_prefix('=').unwrap_or(dir));
            } else if word == "--tree" {
                if let Some(Some(tree)) = words.next() {
                    scope.tree = Some(tree.to_string());
                }
            } else if let Some(tree) = word.strip_prefix("--tree=") {
                scope.tree = Some(tree.to_string());
            }
        }
        scope
    }
}

/// Every candidate of `kind` in `scope` (not yet filtered by what was
/// typed). Empty outside a worktree or when anything cannot be read.
pub fn candidates(kind: Kind, scope: &Scope) -> Vec<CompletionCandidate> {
    let Ok(repo) = Repository::open(&scope.dir) else {
        return Vec::new();
    };
    match kind {
        Kind::Remote => repo
            .config()
            .remote
            .iter()
            .map(|(name, r)| candidate(name, &r.url))
            .collect(),
        Kind::Tree => repo
            .trees()
            .map(|trees| {
                trees
                    .iter()
                    .map(|t| {
                        let path = t.path.to_string_lossy().replace('\\', "/");
                        let help = if path.is_empty() {
                            "root tree".to_string()
                        } else {
                            format!("tree at {path}/")
                        };
                        candidate(&t.name, &help)
                    })
                    .collect()
            })
            .unwrap_or_default(),
        _ => {
            let tree = match &scope.tree {
                Some(tree) => tree.clone(),
                None => match repo.tree_for_path(&scope.dir) {
                    Ok(tree) => tree.name,
                    Err(_) => return Vec::new(),
                },
            };
            ref_candidates(&repo, &tree, kind).unwrap_or_default()
        }
    }
}

fn ref_candidates(
    repo: &Repository,
    tree: &str,
    kind: Kind,
) -> worktree_sdk::Result<Vec<CompletionCandidate>> {
    let branches = |archived: bool, help: &str| -> worktree_sdk::Result<Vec<CompletionCandidate>> {
        Ok(repo
            .branch_names(tree)?
            .into_iter()
            .filter(|(_, a)| *a == archived)
            .map(|(name, _)| candidate(&name, help))
            .collect())
    };
    let tags = || -> worktree_sdk::Result<Vec<CompletionCandidate>> {
        Ok(repo
            .tag_names(tree)?
            .iter()
            .map(|name| candidate(name, "tag"))
            .collect())
    };
    Ok(match kind {
        Kind::Branch => branches(false, "branch")?,
        Kind::ArchivedBranch => branches(true, "archived branch")?,
        Kind::Tag => tags()?,
        Kind::Rev => {
            let mut out = branches(false, "branch")?;
            out.extend(tags()?);
            out.extend(
                repo.remote_branch_names(tree)?
                    .iter()
                    .map(|name| candidate(name, "remote-tracking branch")),
            );
            // A tag named like a branch resolves to the branch; offer the
            // name once.
            let mut seen = HashSet::new();
            out.retain(|c| seen.insert(c.get_value().to_os_string()));
            out
        }
        Kind::Remote | Kind::Tree => Vec::new(),
    })
}

fn candidate(value: &str, help: &str) -> CompletionCandidate {
    CompletionCandidate::new(value).help(Some(help.to_string().into()))
}

/// The candidates of `all` that start with `typed`.
fn starting_with(all: Vec<CompletionCandidate>, typed: &OsStr) -> Vec<CompletionCandidate> {
    let Some(typed) = typed.to_str() else {
        return Vec::new();
    };
    all.into_iter()
        .filter(|c| c.get_value().to_str().is_some_and(|v| v.starts_with(typed)))
        .collect()
}

/// The completer of an argument holding a `kind` name.
pub fn of(kind: Kind) -> ArgValueCompleter {
    ArgValueCompleter::new(move |typed: &OsStr| match Scope::from_process() {
        Some(scope) => starting_with(candidates(kind, &scope), typed),
        None => Vec::new(),
    })
}

/// Non-archived branches.
pub fn branch() -> ArgValueCompleter {
    of(Kind::Branch)
}

/// Archived branches.
pub fn archived_branch() -> ArgValueCompleter {
    of(Kind::ArchivedBranch)
}

/// Tags.
pub fn tag() -> ArgValueCompleter {
    of(Kind::Tag)
}

/// Revisions spelled as names: branches, tags, remote-tracking branches.
pub fn rev() -> ArgValueCompleter {
    of(Kind::Rev)
}

/// Remotes.
pub fn remote() -> ArgValueCompleter {
    of(Kind::Remote)
}

/// Trees of the worktree.
pub fn tree() -> ArgValueCompleter {
    of(Kind::Tree)
}

/// Revisions followed by paths (`wt diff`): revision names, then files and
/// directories relative to the directory the command runs in.
pub fn rev_or_path() -> ArgValueCompleter {
    ArgValueCompleter::new(|typed: &OsStr| {
        let Some(scope) = Scope::from_process() else {
            return Vec::new();
        };
        let mut out = starting_with(candidates(Kind::Rev, &scope), typed);
        out.extend(PathCompleter::any().current_dir(&scope.dir).complete(typed));
        out
    })
}

/// The program the registration script should call: the name `wt` was
/// started with when it is a bare command name (found on PATH at completion
/// time), else the absolute path of this executable.
pub fn completer_program() -> String {
    let arg0 = std::env::args_os()
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("wt"));
    if Path::new(&arg0).components().count() > 1 {
        if let Ok(exe) = std::env::current_exe() {
            let exe = exe.to_string_lossy().into_owned();
            // Windows: no `\\?\` verbatim prefix in a script.
            return exe.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(exe);
        }
    }
    arg0.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &[&str]) -> Vec<OsString> {
        line.iter().map(OsString::from).collect()
    }

    #[test]
    fn scope_follows_dir_and_tree_options() {
        let cwd = PathBuf::from("base");
        let s = Scope::from_words(cwd.clone(), &words(&["wt", "switch", "fe"]));
        assert_eq!(
            s,
            Scope {
                dir: cwd.clone(),
                tree: None
            }
        );
        let s = Scope::from_words(
            cwd.clone(),
            &words(&["wt", "-C", "sub", "--tree", "lib", "switch", ""]),
        );
        assert_eq!(s.dir, cwd.join("sub"));
        assert_eq!(s.tree.as_deref(), Some("lib"));
        let s = Scope::from_words(cwd.clone(), &words(&["wt", "-Cother", "--tree=api", "x"]));
        assert_eq!(s.dir, cwd.join("other"));
        assert_eq!(s.tree.as_deref(), Some("api"));
        let s = Scope::from_words(
            cwd.clone(),
            &words(&["wt", "-C=d", "diff", "--", "--tree", "t"]),
        );
        assert_eq!(s.dir, cwd.join("d"));
        assert_eq!(s.tree, None, "options end at --");
        // A dangling option at the end is the word being completed.
        let s = Scope::from_words(cwd.clone(), &words(&["wt", "--tree"]));
        assert_eq!(s.tree, None);
    }

    #[test]
    fn candidates_are_filtered_by_prefix() {
        let all = || {
            vec![
                candidate("feature/login", "branch"),
                candidate("feature/logout", "branch"),
                candidate("main", "branch"),
            ]
        };
        let values = |c: Vec<CompletionCandidate>| -> Vec<String> {
            c.iter()
                .map(|c| c.get_value().to_string_lossy().into_owned())
                .collect()
        };
        assert_eq!(
            values(starting_with(all(), OsStr::new("feature/logi"))),
            ["feature/login"]
        );
        assert_eq!(values(starting_with(all(), OsStr::new(""))).len(), 3);
        assert!(starting_with(all(), OsStr::new("x")).is_empty());
    }

    #[test]
    fn nothing_outside_a_worktree() {
        let dir = tempfile::tempdir().unwrap();
        let scope = Scope {
            dir: dir.path().to_path_buf(),
            tree: None,
        };
        for kind in [
            Kind::Branch,
            Kind::ArchivedBranch,
            Kind::Tag,
            Kind::Rev,
            Kind::Remote,
            Kind::Tree,
        ] {
            assert!(candidates(kind, &scope).is_empty(), "{kind:?}");
        }
    }

    #[test]
    fn every_shell_gets_a_dynamic_registration() {
        use clap::ValueEnum;
        for shell in Shell::value_variants() {
            let text = String::from_utf8(registration(*shell, "wt").unwrap()).unwrap();
            assert!(text.contains(ENV_VAR), "{shell}: {text}");
            assert!(text.contains("wt"), "{shell}");
        }
        let zsh = String::from_utf8(registration(Shell::Zsh, "wt").unwrap()).unwrap();
        assert!(zsh.starts_with("#compdef wt"), "{zsh}");
        assert!(zsh.contains(r#"[[ "${funcstack[1]}" == "_wt" ]]"#), "{zsh}");
    }

    #[test]
    fn aliases_are_commands_but_never_shadow_builtins() {
        let aliases: BTreeMap<String, String> = [
            ("lgx".to_string(), "log --oneline".to_string()),
            ("status".to_string(), "log".to_string()),
        ]
        .into();
        let cmd = with_aliases(Cli::command(), &aliases);
        let lgx = cmd.find_subcommand("lgx").expect("alias completed");
        assert_eq!(
            lgx.get_about().map(ToString::to_string).as_deref(),
            Some("Alias for `wt log --oneline`")
        );
        let status = cmd.find_subcommand("status").unwrap();
        assert_ne!(
            status.get_about().map(ToString::to_string).as_deref(),
            Some("Alias for `wt log`")
        );
    }
}
