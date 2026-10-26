//! Top-level `wt --help` with the commands grouped by topic (the same
//! groups as docs/cli-reference.md) instead of one flat list of 70+
//! commands. Only the root help changes; `wt CMD --help` is clap's default.
//!
//! Commands not listed in [`GROUPS`] (added later by other workstreams)
//! still appear, under "Other commands", so nothing is ever hidden.

use std::fmt::Write;

use clap::Command;

/// (heading, commands) in display order.
pub const GROUPS: &[(&str, &[&str])] = &[
    (
        "Local history",
        &[
            "init", "status", "snapshot", "log", "show", "diff", "reflog", "blame",
        ],
    ),
    (
        "Branches and merging",
        &[
            "branch",
            "switch",
            "checkout",
            "merge",
            "revert",
            "restore",
            "cherry-pick",
            "rebase",
            "stash",
            "undo",
            "tag",
        ],
    ),
    (
        "Worktree structure and configuration",
        &["tree", "sparse", "ignore", "config"],
    ),
    (
        "Servers and sync",
        &[
            "remote",
            "register",
            "login",
            "logout",
            "whoami",
            "password-reset",
            "push",
            "fetch",
            "pull",
            "sync",
            "clone",
            "stage",
            "unstage",
        ],
    ),
    ("Export and interop", &["archive", "git", "mirror"]),
    ("Background daemon", &["daemon"]),
    (
        "Collaboration",
        &[
            "tenant", "trees", "fork", "mr", "owners", "ci", "protect", "license", "todo", "deps",
            "release", "search", "events", "webhook", "inbox", "open",
        ],
    ),
    ("Administration", &["admin", "audit", "compliance", "api"]),
    (
        "Identity and access",
        &[
            "team", "role", "policy", "grant", "access", "sa", "token", "session", "mfa", "invite",
            "iam", "attr", "sso", "identity",
        ],
    ),
    (
        "Maintenance and setup",
        &["gc", "fsck", "doctor", "completions", "help"],
    ),
];

const HELP_ABOUT: &str = "Print this message or the help of the given subcommand(s)";

/// `command` with the grouped root help template.
pub fn grouped(command: Command) -> Command {
    let template = template(&command);
    command.help_template(template)
}

/// The grouped command list, as (heading, [(name, about)]).
pub fn groups(command: &Command) -> Vec<(String, Vec<(String, String)>)> {
    let visible: Vec<(String, String)> = command
        .get_subcommands()
        .filter(|c| !c.is_hide_set())
        .map(|c| {
            (
                c.get_name().to_string(),
                c.get_about().map(|a| a.to_string()).unwrap_or_default(),
            )
        })
        .chain(std::iter::once((
            "help".to_string(),
            HELP_ABOUT.to_string(),
        )))
        .collect();
    let mut out = Vec::new();
    let mut placed = std::collections::BTreeSet::new();
    for (heading, names) in GROUPS {
        let rows: Vec<(String, String)> = names
            .iter()
            .filter_map(|n| visible.iter().find(|(name, _)| name == n).cloned())
            .collect();
        placed.extend(rows.iter().map(|(n, _)| n.clone()));
        if !rows.is_empty() {
            out.push((heading.to_string(), rows));
        }
    }
    let rest: Vec<(String, String)> = visible
        .into_iter()
        .filter(|(n, _)| !placed.contains(n))
        .collect();
    if !rest.is_empty() {
        out.push(("Other commands".to_string(), rest));
    }
    out
}

fn template(command: &Command) -> String {
    let styles = command.get_styles();
    let header = styles.get_header();
    let literal = styles.get_literal();
    let groups = groups(command);
    let width = groups
        .iter()
        .flat_map(|(_, rows)| rows.iter().map(|(n, _)| n.len()))
        .max()
        .unwrap_or(0);
    // Template text is literal except for {tags}; command descriptions
    // never contain braces, but escape-proof them anyway.
    let clean = |s: &str| s.replace(['{', '}'], "");
    let mut t = String::from("{before-help}{about-with-newline}\n{usage-heading} {usage}\n");
    for (heading, rows) in &groups {
        let _ = write!(
            t,
            "\n{}{heading}:{}\n",
            header.render(),
            header.render_reset()
        );
        for (name, about) in rows {
            let pad = " ".repeat(width - name.len());
            let _ = writeln!(
                t,
                "  {}{name}{}{pad}  {}",
                literal.render(),
                literal.render_reset(),
                clean(about)
            );
        }
    }
    let _ = write!(
        t,
        "\n{}Options:{}\n{{options}}{{after-help}}",
        header.render(),
        header.render_reset()
    );
    t
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::*;
    use crate::cli::Cli;

    fn render(long: bool) -> String {
        let mut cmd = grouped(Cli::command());
        let help = if long {
            cmd.render_long_help()
        } else {
            cmd.render_help()
        };
        help.to_string()
    }

    #[test]
    fn every_command_is_listed_exactly_once() {
        let cmd = Cli::command();
        let listed: Vec<String> = groups(&cmd)
            .into_iter()
            .flat_map(|(_, rows)| rows.into_iter().map(|(n, _)| n))
            .collect();
        for sub in cmd.get_subcommands().filter(|c| !c.is_hide_set()) {
            let n = listed.iter().filter(|l| *l == sub.get_name()).count();
            assert_eq!(n, 1, "{} listed {n} times", sub.get_name());
        }
        assert!(listed.contains(&"help".to_string()));
        // Every group entry names a real command (no stale names).
        for (_, names) in GROUPS {
            for name in *names {
                assert!(
                    *name == "help" || cmd.find_subcommand(name).is_some(),
                    "{name}"
                );
            }
        }
    }

    #[test]
    fn root_help_is_grouped() {
        for long in [false, true] {
            let help = render(long);
            assert!(help.contains("Usage: wt [OPTIONS] <COMMAND>"), "{help}");
            for heading in ["Local history:", "Servers and sync:", "Options:"] {
                assert!(help.contains(heading), "{heading} missing:\n{help}");
            }
            assert!(help.contains("  status "), "{help}");
            assert!(help.contains("--json"), "{help}");
            assert!(!help.contains("{options}"), "{help}");
            let local = help.find("Local history:").unwrap();
            let sync = help.find("Servers and sync:").unwrap();
            let options = help.find("Options:").unwrap();
            assert!(local < sync && sync < options);
        }
        assert!(render(true).contains("multi-tenant servers"));
    }

    #[test]
    fn subcommand_help_is_unchanged() {
        let mut cmd = grouped(Cli::command());
        cmd.build();
        let status = cmd.find_subcommand_mut("status").unwrap();
        let help = status.render_help().to_string();
        assert!(!help.contains("Local history:"), "{help}");
        assert!(help.contains("--team"), "{help}");
    }
}
