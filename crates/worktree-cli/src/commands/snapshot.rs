//! `wt snapshot`

use clap::Args;
use colored::Colorize;
use worktree_sdk::{Snapshot, SnapshotOptions};

use crate::context::Context;
use crate::error::Result;
use crate::output::format::subject;
use crate::output::Text;

#[derive(Debug, Args)]
pub struct SnapshotArgs {
    /// Snapshot message
    #[arg(short, long, value_name = "MSG")]
    pub message: String,

    /// Create a snapshot even if nothing changed
    #[arg(long)]
    pub allow_empty: bool,

    /// Skip the pre-snapshot secret scan
    #[arg(long)]
    pub no_scan: bool,

    // --- signing (ENGINEERING_PLAN §13.6) ---
    /// Sign this snapshot with the configured key ([signing] key, else the
    /// `default` key; see `wt identity generate`)
    #[arg(long)]
    pub sign: bool,
    // --- end signing ---
    // deps-releases (ENGINEERING_PLAN §13.5): --depend/--todo/... groups.
    #[command(flatten)]
    pub deps: deps::DependencyArgs,
}

pub fn run(ctx: &Context, args: SnapshotArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let dependencies = args.deps.resolve(&args.message);
    let snapshot = repo.snapshot(
        &tree.name,
        SnapshotOptions {
            message: args.message,
            allow_empty: args.allow_empty,
            skip_secret_scan: args.no_scan,
            sign: args.sign, // signing (§13.6)
            dependencies,
            ..SnapshotOptions::default()
        },
    )?;
    ctx.out.emit(&snapshot, |t| snapshot_line(t, &snapshot))
}

/// `[main 1a2b3c4d] message` (shared by revert/merge).
pub fn snapshot_line(t: &mut Text, snapshot: &Snapshot) {
    t.line(format_args!(
        "[{} {}] {}",
        snapshot.branch.green(),
        snapshot.short_id().yellow(),
        subject(&snapshot.message)
    ));
}

/// Dependency declarations of `wt snapshot` (deps-releases, §13.5).
///
/// Each `--depend TREE[@BRANCH]` starts a dependency; the `--depend-tenant`,
/// `--todo`, `--details`, `--priority`, `--blocking` and `--linked` flags
/// that follow it (before the next `--depend`) belong to it. Clap's derive
/// cannot express such positional groups, so the arguments are declared
/// and grouped by hand from their command-line indices.
pub mod deps {
    use clap::error::ErrorKind;
    use clap::{Arg, ArgAction, ArgMatches, Command, FromArgMatches};
    use worktree_sdk::Dependency;

    use crate::output::format::subject;

    const DEPEND: &str = "depend";
    const TENANT: &str = "depend_tenant";
    const TODO: &str = "todo";
    const DETAILS: &str = "details";
    const PRIORITY: &str = "priority";
    const BLOCKING: &str = "blocking";
    const LINKED: &str = "linked";

    /// Parsed dependency groups (titles may still be empty; see
    /// [`DependencyArgs::resolve`]).
    #[derive(Debug, Clone, Default, PartialEq, Eq)]
    pub struct DependencyArgs {
        pub dependencies: Vec<Dependency>,
    }

    impl DependencyArgs {
        /// The dependencies to record; an empty title becomes the first
        /// line of the snapshot message.
        pub fn resolve(&self, message: &str) -> Vec<Dependency> {
            self.dependencies
                .iter()
                .cloned()
                .map(|mut d| {
                    if d.title.trim().is_empty() {
                        d.title = subject(message).trim().to_string();
                    }
                    d
                })
                .collect()
        }
    }

    impl clap::Args for DependencyArgs {
        fn augment_args(cmd: Command) -> Command {
            cmd.arg(
                Arg::new(DEPEND)
                    .long("depend")
                    .value_name("TREE[@BRANCH]")
                    .action(ArgAction::Append)
                    .help_heading("Dependencies")
                    .help("Declare that this snapshot needs work in TREE (TENANT/TREE also accepted); creates a TODO there on push. Repeatable; the flags below apply to the preceding --depend"),
            )
            .arg(
                Arg::new(TENANT)
                    .long("depend-tenant")
                    .value_name("TENANT")
                    .action(ArgAction::Append)
                    .help_heading("Dependencies")
                    .help("Tenant of the dependency's tree (default: the tenant of this tree)"),
            )
            .arg(
                Arg::new(TODO)
                    .long("todo")
                    .value_name("TITLE")
                    .action(ArgAction::Append)
                    .help_heading("Dependencies")
                    .help("Title of the TODO (default: the first line of the message)"),
            )
            .arg(
                Arg::new(DETAILS)
                    .long("details")
                    .value_name("TEXT")
                    .action(ArgAction::Append)
                    .help_heading("Dependencies")
                    .help("Longer description of the TODO"),
            )
            .arg(
                Arg::new(PRIORITY)
                    .long("priority")
                    .value_name("PRIORITY")
                    .value_parser(["low", "medium", "high", "critical"])
                    .action(ArgAction::Append)
                    .help_heading("Dependencies")
                    .help("Priority of the TODO (default: medium)"),
            )
            .arg(
                flag(BLOCKING, "blocking")
                    .help("The dependent branch may not merge until the TODO is done"),
            )
            .arg(flag(LINKED, "linked").help("The branches must merge together (linked merge)"))
        }

        fn augment_args_for_update(cmd: Command) -> Command {
            Self::augment_args(cmd)
        }
    }

    impl FromArgMatches for DependencyArgs {
        fn from_arg_matches(m: &ArgMatches) -> Result<Self, clap::Error> {
            group(m).map_err(|msg| clap::Error::raw(ErrorKind::ArgumentConflict, msg + "\n"))
        }

        fn update_from_arg_matches(&mut self, m: &ArgMatches) -> Result<(), clap::Error> {
            *self = Self::from_arg_matches(m)?;
            Ok(())
        }
    }

    /// (command-line index, value) of every occurrence of a string option.
    fn occurrences(m: &ArgMatches, id: &str) -> Vec<(usize, String)> {
        match (m.indices_of(id), m.get_many::<String>(id)) {
            (Some(idx), Some(vals)) => idx.zip(vals.cloned()).collect(),
            _ => Vec::new(),
        }
    }

    /// A value-less flag that records every occurrence (and its index),
    /// unlike `SetTrue`/`Count`, which keep only one index.
    fn flag(id: &'static str, long: &'static str) -> Arg {
        Arg::new(id)
            .long(long)
            .num_args(0)
            .default_missing_value("true")
            .value_parser(clap::value_parser!(bool))
            .action(ArgAction::Append)
            .help_heading("Dependencies")
    }

    /// Command-line indices of every occurrence of a flag.
    fn flag_occurrences(m: &ArgMatches, id: &str) -> Vec<usize> {
        m.indices_of(id).map(|i| i.collect()).unwrap_or_default()
    }

    /// Parse `TREE[@BRANCH]` or `TENANT/TREE[@BRANCH]`.
    pub fn parse_target(spec: &str) -> Result<Dependency, String> {
        let (path, branch) = match spec.split_once('@') {
            Some((_, "")) => return Err(format!("--depend '{spec}': empty branch after '@'")),
            Some((p, b)) => (p, b),
            None => (spec, ""),
        };
        let (tenant, tree) = match path.split_once('/') {
            Some((t, n)) if !t.is_empty() && !n.is_empty() && !n.contains('/') => (t, n),
            Some(_) => {
                return Err(format!(
                    "--depend '{spec}' is not of the form [TENANT/]TREE[@BRANCH]"
                ))
            }
            None => ("", path),
        };
        if tree.is_empty() {
            return Err(format!("--depend '{spec}': the tree name is empty"));
        }
        Ok(Dependency {
            tenant: tenant.to_string(),
            tree: tree.to_string(),
            branch: branch.to_string(),
            ..Dependency::default()
        })
    }

    fn group(m: &ArgMatches) -> Result<DependencyArgs, String> {
        let depends = occurrences(m, DEPEND);
        let mut deps = depends
            .iter()
            .map(|(_, spec)| parse_target(spec))
            .collect::<Result<Vec<_>, _>>()?;
        // Which --depend an occurrence at `index` belongs to.
        let owner = |flag: &str, index: usize| -> Result<usize, String> {
            depends
                .iter()
                .rposition(|(i, _)| *i < index)
                .ok_or_else(|| format!("{flag} must follow a --depend"))
        };
        let mut seen = std::collections::HashSet::new();
        let mut once = |flag: &'static str, group: usize, deps: &[Dependency]| {
            if seen.insert((flag, group)) {
                Ok(())
            } else {
                Err(format!(
                    "{flag} given twice for the dependency on '{}'",
                    deps[group].tree
                ))
            }
        };
        for (flag, id) in [
            ("--depend-tenant", TENANT),
            ("--todo", TODO),
            ("--details", DETAILS),
            ("--priority", PRIORITY),
        ] {
            for (index, value) in occurrences(m, id) {
                let g = owner(flag, index)?;
                once(flag, g, &deps)?;
                let d = &mut deps[g];
                match id {
                    TENANT => {
                        if !d.tenant.is_empty() {
                            return Err(format!(
                                "--depend-tenant conflicts with the tenant in --depend '{}/{}'",
                                d.tenant, d.tree
                            ));
                        }
                        d.tenant = value;
                    }
                    TODO => d.title = value,
                    DETAILS => d.details = value,
                    _ => d.priority = value,
                }
            }
        }
        for (flag, id) in [("--blocking", BLOCKING), ("--linked", LINKED)] {
            for index in flag_occurrences(m, id) {
                let g = owner(flag, index)?;
                once(flag, g, &deps)?;
                if id == BLOCKING {
                    deps[g].blocking = true;
                } else {
                    deps[g].linked = true;
                }
            }
        }
        Ok(DependencyArgs { dependencies: deps })
    }
}

#[cfg(test)]
mod tests {
    use clap::error::ErrorKind;
    use worktree_sdk::Dependency;

    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::{parse, parse_err};

    fn snapshot(args: &[&str]) -> SnapshotArgs {
        let mut full = vec!["snapshot", "-m", "Add login\n\nbody"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Snapshot(a) => a,
            other => panic!("{other:?}"),
        }
    }

    fn dep(tenant: &str, tree: &str, branch: &str) -> Dependency {
        Dependency {
            tenant: tenant.into(),
            tree: tree.into(),
            branch: branch.into(),
            ..Dependency::default()
        }
    }

    #[test]
    fn no_dependencies_by_default() {
        let a = snapshot(&[]);
        assert!(a.deps.dependencies.is_empty());
        assert!(a.deps.resolve(&a.message).is_empty());
    }

    #[test]
    fn groups_flags_by_preceding_depend() {
        let a = snapshot(&[
            "--depend",
            "api@develop",
            "--todo",
            "Add endpoint",
            "--priority",
            "high",
            "--blocking",
            "--depend",
            "lib",
            "--depend-tenant",
            "other",
            "--linked",
            "--details",
            "more",
            "--depend",
            "acme/docs",
            "--blocking",
        ]);
        let got = a.deps.resolve(&a.message);
        assert_eq!(
            got,
            vec![
                Dependency {
                    title: "Add endpoint".into(),
                    priority: "high".into(),
                    blocking: true,
                    ..dep("", "api", "develop")
                },
                Dependency {
                    title: "Add login".into(),
                    details: "more".into(),
                    linked: true,
                    ..dep("other", "lib", "")
                },
                Dependency {
                    title: "Add login".into(),
                    blocking: true,
                    ..dep("acme", "docs", "")
                },
            ]
        );
    }

    #[test]
    fn rejects_misplaced_or_repeated_flags() {
        let base = ["snapshot", "-m", "m"];
        let cases: &[&[&str]] = &[
            &["--todo", "x"],
            &["--blocking", "--depend", "api"],
            &["--depend", "api", "--todo", "a", "--todo", "b"],
            &["--depend", "api", "--blocking", "--blocking"],
            &["--depend", "acme/api", "--depend-tenant", "other"],
            &["--depend", "api@"],
            &["--depend", "a/b/c"],
            &["--depend", "/api"],
            &["--depend", "@main"],
        ];
        for extra in cases {
            let mut argv = base.to_vec();
            argv.extend_from_slice(extra);
            assert_eq!(parse_err(&argv), ErrorKind::ArgumentConflict, "{extra:?}");
        }
        assert_eq!(
            parse_err(&[
                "snapshot",
                "-m",
                "m",
                "--depend",
                "a",
                "--priority",
                "urgent"
            ]),
            ErrorKind::InvalidValue
        );
    }

    #[test]
    fn parses_targets() {
        assert_eq!(deps::parse_target("api").unwrap(), dep("", "api", ""));
        assert_eq!(
            deps::parse_target("t/api@rel/1.0").unwrap(),
            dep("t", "api", "rel/1.0")
        );
        assert!(deps::parse_target("").is_err());
    }
}
