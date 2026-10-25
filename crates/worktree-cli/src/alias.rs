//! Command aliases (ENGINEERING_PLAN §14.8): `[alias]` in the user and
//! worktree config, expanded on the raw command line before clap parses it.
//!
//! Rules:
//!
//! - Only the command word is expanded: the first argument after the global
//!   options (`--json`, `--no-color`, `-C PATH`, `--tree NAME`,
//!   `--server URL`). Everything after it is appended to the expansion, so
//!   with `lg = "log --oneline -n 20"`, `wt lg main` runs
//!   `wt log --oneline -n 20 main`.
//! - Built-in commands (and their clap aliases, and `help`) can never be
//!   shadowed: an alias with such a name is ignored.
//! - An expansion may start with another alias; at most [`MAX_DEPTH`]
//!   expansions happen, a deeper (or cyclic) chain is a usage error.
//! - A word that is neither a command nor an alias is left to clap, which
//!   reports it with its "did you mean" suggestion.
//! - Aliases are read from the directory given with `-C` (else the current
//!   directory); unreadable config files contribute no aliases.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use worktree_sdk::config::split_alias_command;

use crate::error::{CliError, Result};

/// Maximum number of alias expansions for one command line.
pub const MAX_DEPTH: usize = 5;

/// Global options that take a value (as separate argument or `--opt=value`).
const VALUE_OPTIONS: &[&str] = &["--tree", "--server"];
/// Global flags without a value.
const FLAGS: &[&str] = &["--json", "--no-color"];

/// What the scan of the leading global options found.
#[derive(Debug, PartialEq, Eq)]
struct Scan {
    /// Index of the command word, if the command line has one.
    command: Option<usize>,
    /// The `-C` directory, if given before the command word.
    dir: Option<PathBuf>,
}

/// Find the command word: skip `argv[0]` and the global options before it.
/// Help/version flags, `--` and unknown options end the scan without a
/// command (clap handles those command lines unchanged).
fn scan(args: &[OsString]) -> Scan {
    let mut dir = None;
    let mut i = 1;
    while let Some(arg) = args.get(i) {
        let Some(text) = arg.to_str() else {
            return Scan { command: None, dir };
        };
        if !text.starts_with('-') || text == "-" {
            return Scan {
                command: Some(i),
                dir,
            };
        }
        if FLAGS.contains(&text) {
            i += 1;
        } else if text == "-C" {
            dir = args.get(i + 1).map(PathBuf::from);
            i += 2;
        } else if let Some(value) = text.strip_prefix("-C") {
            dir = Some(PathBuf::from(value.strip_prefix('=').unwrap_or(value)));
            i += 1;
        } else if VALUE_OPTIONS.contains(&text) {
            i += 2;
        } else if VALUE_OPTIONS
            .iter()
            .any(|o| text.strip_prefix(o).is_some_and(|r| r.starts_with('=')))
        {
            i += 1;
        } else {
            return Scan { command: None, dir };
        }
    }
    Scan { command: None, dir }
}

/// Expand an alias in `args` (the full argv, `args[0]` = program name).
/// `is_builtin` tells whether a word names a built-in command; `aliases`
/// loads the alias table for a directory (called at most once, and only
/// when the command word is not built in).
pub fn expand_with(
    mut args: Vec<OsString>,
    is_builtin: impl Fn(&str) -> bool,
    aliases: impl FnOnce(&Path) -> BTreeMap<String, String>,
) -> Result<Vec<OsString>> {
    let mut table: Option<BTreeMap<String, String>> = None;
    let mut loader = Some(aliases);
    let mut chain: Vec<String> = Vec::new();
    loop {
        let found = scan(&args);
        let Some(index) = found.command else {
            return Ok(args);
        };
        let Some(word) = args[index].to_str().map(str::to_string) else {
            return Ok(args);
        };
        if is_builtin(&word) {
            return Ok(args);
        }
        let table = table.get_or_insert_with(|| {
            let dir = found.dir.unwrap_or_else(|| PathBuf::from("."));
            loader.take().map(|load| load(&dir)).unwrap_or_default()
        });
        let Some(command) = table.get(&word) else {
            return Ok(args);
        };
        chain.push(word.clone());
        if chain.len() > MAX_DEPTH {
            return Err(CliError::usage(format!(
                "alias '{}' expands more than {MAX_DEPTH} levels deep ({})",
                chain[0],
                chain.join(" -> ")
            ))
            .with_hint("check [alias] in `wt config list` for a cycle"));
        }
        let words = split_alias_command(command).map_err(|e| {
            CliError::usage(format!("alias '{word}' = {command:?}: {e}"))
                .with_hint(format!("fix it with `wt config set alias.{word} \"...\"`"))
        })?;
        args.splice(index..=index, words.into_iter().map(OsString::from));
    }
}

/// [`expand_with`] for the real `wt` command line: built-ins come from the
/// clap definition, aliases from the user and worktree config.
pub fn expand(args: Vec<OsString>, command: &clap::Command) -> Result<Vec<OsString>> {
    expand_with(
        args,
        |word| is_builtin(command, word),
        worktree_sdk::config::load_aliases,
    )
}

/// Whether `word` names a built-in subcommand (or one of its clap aliases).
pub fn is_builtin(command: &clap::Command, word: &str) -> bool {
    word == "help" || command.find_subcommand(word).is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(words: &[&str]) -> Vec<OsString> {
        std::iter::once("wt")
            .chain(words.iter().copied())
            .map(OsString::from)
            .collect()
    }

    fn strings(args: &[OsString]) -> Vec<String> {
        args.iter()
            .skip(1)
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn table(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    const BUILTINS: &[&str] = &["status", "log", "snapshot", "config"];

    fn run(words: &[&str], aliases: &[(&str, &str)]) -> Result<Vec<String>> {
        let aliases = table(aliases);
        expand_with(
            argv(words),
            |w| w == "help" || BUILTINS.contains(&w),
            move |_| aliases,
        )
        .map(|a| strings(&a))
    }

    #[test]
    fn expands_the_command_word_and_keeps_the_rest() {
        let aliases = [("st", "status"), ("lg", "log --oneline -n 20")];
        assert_eq!(run(&["st"], &aliases).unwrap(), ["status"]);
        assert_eq!(
            run(&["lg", "main", "--json"], &aliases).unwrap(),
            ["log", "--oneline", "-n", "20", "main", "--json"]
        );
        assert_eq!(
            run(&["-C", "dir", "--json", "--tree=web", "st"], &aliases).unwrap(),
            ["-C", "dir", "--json", "--tree=web", "status"]
        );
        assert_eq!(
            run(&["--server", "http://s", "--no-color", "lg"], &aliases).unwrap(),
            [
                "--server",
                "http://s",
                "--no-color",
                "log",
                "--oneline",
                "-n",
                "20"
            ]
        );
        // Quoted words survive as one argument.
        assert_eq!(
            run(&["wip"], &[("wip", "snapshot -m 'work in progress'")]).unwrap(),
            ["snapshot", "-m", "work in progress"]
        );
    }

    #[test]
    fn builtins_cannot_be_shadowed() {
        let aliases = [("status", "log"), ("help", "log"), ("log", "status")];
        assert_eq!(run(&["status"], &aliases).unwrap(), ["status"]);
        assert_eq!(run(&["help"], &aliases).unwrap(), ["help"]);
        assert_eq!(
            run(&["log", "-n", "1"], &aliases).unwrap(),
            ["log", "-n", "1"]
        );
    }

    #[test]
    fn aliases_may_chain_up_to_the_limit() {
        let chain = [
            ("a1", "a2"),
            ("a2", "a3"),
            ("a3", "a4"),
            ("a4", "a5 --json"),
            ("a5", "status"),
        ];
        assert_eq!(
            run(&["a1", "x"], &chain).unwrap(),
            ["status", "--json", "x"]
        );
        let too_deep = [
            ("a1", "a2"),
            ("a2", "a3"),
            ("a3", "a4"),
            ("a4", "a5"),
            ("a5", "a6"),
            ("a6", "status"),
        ];
        let err = run(&["a1"], &too_deep).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("more than 5 levels"), "{err}");
    }

    #[test]
    fn cycles_are_reported() {
        let err = run(&["ping"], &[("ping", "pong"), ("pong", "ping")]).unwrap_err();
        assert!(err.to_string().contains("ping -> pong -> ping"), "{err}");
        let err = run(&["me"], &[("me", "me --json")]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
    }

    #[test]
    fn unknown_words_and_odd_command_lines_pass_through() {
        let aliases = [("st", "status")];
        // clap reports unknown commands itself (with suggestions).
        assert_eq!(run(&["stauts"], &aliases).unwrap(), ["stauts"]);
        assert_eq!(run(&[], &aliases).unwrap(), Vec::<String>::new());
        assert_eq!(run(&["--help"], &aliases).unwrap(), ["--help"]);
        assert_eq!(
            run(&["--bogus", "st"], &aliases).unwrap(),
            ["--bogus", "st"]
        );
        assert_eq!(run(&["--", "st"], &aliases).unwrap(), ["--", "st"]);
        // Only the command word is expanded, never later arguments.
        assert_eq!(run(&["log", "st"], &aliases).unwrap(), ["log", "st"]);
    }

    #[test]
    fn bad_alias_values_are_usage_errors() {
        let err = run(&["q"], &[("q", "log 'open")]).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("unterminated"), "{err}");
    }

    #[test]
    fn aliases_are_loaded_lazily_from_the_c_directory() {
        let seen = std::cell::RefCell::new(None);
        let out = expand_with(
            argv(&["-C", "proj", "st"]),
            |w| w == "status",
            |dir| {
                *seen.borrow_mut() = Some(dir.to_path_buf());
                table(&[("st", "status")])
            },
        )
        .unwrap();
        assert_eq!(strings(&out), ["-C", "proj", "status"]);
        assert_eq!(seen.into_inner(), Some(PathBuf::from("proj")));

        // A built-in command never reads the config.
        expand_with(
            argv(&["status"]),
            |w| w == "status",
            |_| -> BTreeMap<_, _> { panic!("aliases loaded for a built-in command") },
        )
        .unwrap();

        assert_eq!(
            scan(&argv(&["-Cproj", "x"])),
            Scan {
                command: Some(2),
                dir: Some(PathBuf::from("proj"))
            }
        );
    }

    #[test]
    fn real_builtins_include_clap_aliases_and_help() {
        let cmd = <crate::cli::Cli as clap::CommandFactory>::command();
        for word in ["status", "log", "help", "completions", "doctor", "open"] {
            assert!(is_builtin(&cmd, word), "{word}");
        }
        assert!(!is_builtin(&cmd, "st"));
    }
}
