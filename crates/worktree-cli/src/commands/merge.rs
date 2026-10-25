//! `wt merge` (and `wt merge resolve|abort|status`), `wt revert`, `wt restore`

use std::fs;
use std::path::PathBuf;

use clap::{ArgGroup, Args, Subcommand, ValueEnum};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::{
    ConflictInfo, ConflictKind, ConflictResolution, ConflictStatus, MergeOptions, MergeOutcome,
    MergeStatus, MergeStrategy, RevertOptions,
};

use crate::commands::snapshot::snapshot_line;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::{plural, short, short_or};
use crate::output::Text;

#[derive(Debug, Args)]
#[command(
    group(ArgGroup::new("operation").required(true).args(["source", "resume", "abort"])),
    subcommand_negates_reqs = true,
    args_conflicts_with_subcommands = true
)]
pub struct MergeArgs {
    #[command(subcommand)]
    pub action: Option<MergeCommand>,

    /// Branch or revision to merge into the current branch
    pub source: Option<String>,

    /// Message for the merge snapshot
    #[arg(short, long, value_name = "MSG", conflicts_with = "abort")]
    pub message: Option<String>,

    /// Always create a merge snapshot, even when a fast-forward is possible
    #[arg(long, conflicts_with_all = ["resume", "abort"])]
    pub no_ff: bool,

    /// How conflicting changes are resolved: `manual` writes conflict
    /// markers (labelled `ours (BRANCH ID)` / `theirs (SOURCE ID)`) and stops;
    /// `ours` / `theirs` resolve every conflicting hunk and file in favour of
    /// that side while still combining non-conflicting changes
    #[arg(
        short = 's',
        long,
        value_enum,
        value_name = "STRATEGY",
        default_value_t = StrategyArg::Manual,
        conflicts_with_all = ["resume", "abort"]
    )]
    pub strategy: StrategyArg,

    /// Finish a conflicted merge after resolving the conflicts
    #[arg(long = "continue")]
    pub resume: bool,

    /// Abandon a conflicted merge and restore the pre-merge state
    #[arg(long)]
    pub abort: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum StrategyArg {
    Manual,
    Ours,
    Theirs,
}

impl From<StrategyArg> for MergeStrategy {
    fn from(arg: StrategyArg) -> Self {
        match arg {
            StrategyArg::Manual => MergeStrategy::Manual,
            StrategyArg::Ours => MergeStrategy::Ours,
            StrategyArg::Theirs => MergeStrategy::Theirs,
        }
    }
}

#[derive(Debug, Subcommand)]
pub enum MergeCommand {
    /// Resolve conflicted paths of the merge in progress
    ///
    /// `--ours` / `--theirs` resolve the conflicting hunks of a text file in
    /// favour of that side (non-conflicting changes from the other side are
    /// kept) and take that side's version of binary or modify/delete
    /// conflicts; `--file F` uses the contents of F.
    #[command(group(ArgGroup::new("side").required(true).args(["ours", "theirs", "file"])))]
    Resolve {
        /// Conflicted paths to resolve
        #[arg(value_name = "PATH", required_unless_present = "all")]
        paths: Vec<PathBuf>,
        /// Resolve every unresolved conflict (with --ours or --theirs)
        #[arg(long, conflicts_with_all = ["paths", "file"])]
        all: bool,
        /// Resolve in favour of the current branch
        #[arg(long)]
        ours: bool,
        /// Resolve in favour of the merged source
        #[arg(long)]
        theirs: bool,
        /// Use the contents of this file as the resolution
        #[arg(long, value_name = "FILE")]
        file: Option<PathBuf>,
    },
    /// Abandon the merge in progress and restore the pre-merge state
    Abort,
    /// Show the merge in progress and the state of each conflict
    Status,
}

#[derive(Debug, Args)]
pub struct RevertArgs {
    /// Snapshot whose changes to undo
    #[arg(value_name = "REV")]
    pub rev: String,

    /// Message for the revert snapshot (default: `Revert "<subject>"`)
    #[arg(short, long, value_name = "MSG")]
    pub message: Option<String>,

    /// For a merge snapshot: the parent (1-based) whose line of history is
    /// kept; the changes the merge brought relative to it are undone
    #[arg(long, alias = "mainline", value_name = "N")]
    pub parent: Option<usize>,
}

#[derive(Debug, Args)]
pub struct RestoreArgs {
    /// Paths to restore (default: everything)
    #[arg(value_name = "PATH")]
    pub paths: Vec<PathBuf>,

    /// Revision to restore from (default: HEAD)
    #[arg(long, value_name = "REV")]
    pub from: Option<String>,
}

#[derive(Serialize)]
struct AbortResult<'a> {
    tree: &'a str,
    aborted: bool,
}

#[derive(Serialize)]
struct ResolveResult<'a> {
    tree: &'a str,
    resolved: &'a [String],
    remaining: Vec<&'a str>,
}

pub fn run(ctx: &Context, args: MergeArgs) -> Result<()> {
    match args.action {
        Some(MergeCommand::Abort) => return abort(ctx),
        Some(MergeCommand::Status) => return status(ctx),
        Some(MergeCommand::Resolve {
            paths,
            all,
            ours,
            theirs,
            file,
        }) => return resolve(ctx, paths, all, ours, theirs, file),
        None => {}
    }
    if args.abort {
        return abort(ctx);
    }
    let (repo, tree) = ctx.open_tree()?;
    if args.resume {
        let snapshot = repo.continue_merge(&tree.name, args.message.as_deref())?;
        return ctx.out.emit(&snapshot, |t| snapshot_line(t, &snapshot));
    }
    let source = args
        .source
        .ok_or_else(|| CliError::usage("nothing to merge"))?;
    let opts = MergeOptions {
        message: args.message,
        no_fast_forward: args.no_ff,
        strategy: args.strategy.into(),
        ..MergeOptions::default()
    };
    let outcome = repo.merge(&tree.name, &source, opts)?;
    ctx.out.emit(&outcome, |t| render_outcome(t, &outcome))?;
    conflict_status(&outcome)
}

fn abort(ctx: &Context) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    repo.abort_merge(&tree.name)?;
    let result = AbortResult {
        tree: &tree.name,
        aborted: true,
    };
    ctx.out.emit(&result, |t| {
        t.line("Merge aborted; the pre-merge state was restored.")
    })
}

fn status(ctx: &Context) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let status = repo.merge_status(&tree.name)?;
    ctx.out.emit(&status, |t| match &status {
        None => t.line("No merge in progress."),
        Some(status) => render_status(t, status),
    })
}

fn resolve(
    ctx: &Context,
    paths: Vec<PathBuf>,
    all: bool,
    ours: bool,
    theirs: bool,
    file: Option<PathBuf>,
) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let resolution = match (ours, theirs, file) {
        (true, _, _) => ConflictResolution::Ours,
        (_, true, _) => ConflictResolution::Theirs,
        (_, _, Some(file)) => {
            let path = ctx.absolute(&file);
            let data = fs::read(&path)
                .map_err(|e| CliError::failure(format!("cannot read {}: {e}", path.display())))?;
            ConflictResolution::Content(data)
        }
        _ => return Err(CliError::usage("pass --ours, --theirs or --file FILE")),
    };
    let resolved = if all {
        repo.resolve_all_conflicts(&tree.name, &resolution)?
    } else {
        let rels = ctx.tree_paths(&repo, &tree, &paths)?;
        if rels.is_empty() {
            return Err(CliError::usage(
                "name the conflicted paths to resolve, or pass --all",
            ));
        }
        for rel in &rels {
            repo.resolve_conflict(&tree.name, rel, &resolution)?;
        }
        rels
    };
    let status = repo.merge_status(&tree.name)?;
    let remaining: Vec<&str> = status
        .iter()
        .flat_map(|s| s.unresolved())
        .map(|c| c.path.as_str())
        .collect();
    let result = ResolveResult {
        tree: &tree.name,
        resolved: &resolved,
        remaining: remaining.clone(),
    };
    ctx.out.emit(&result, |t| {
        if resolved.is_empty() {
            t.line("Nothing to resolve.");
        }
        for path in &resolved {
            t.line(format_args!("resolved: {path}"));
        }
        if remaining.is_empty() {
            t.line("All conflicts resolved; run `wt merge --continue` to finish the merge.");
        } else {
            t.line(format_args!(
                "{} still unresolved: {}",
                plural(remaining.len(), "conflict", "conflicts"),
                remaining.join(", ")
            ));
        }
    })
}

/// Human rendering of a merge (shared with `wt pull`).
pub fn render_outcome(t: &mut Text, outcome: &MergeOutcome) {
    match outcome {
        MergeOutcome::UpToDate => t.line("Already up to date."),
        MergeOutcome::FastForward { to } => {
            t.line(format_args!("Fast-forward to {}", short(to).yellow()))
        }
        MergeOutcome::Merged { snapshot } => {
            t.line("Merge made by the three-way strategy.");
            snapshot_line(t, snapshot);
        }
        MergeOutcome::Conflicts { conflicts } => render_conflicts(t, conflicts),
    }
}

fn render_conflicts(t: &mut Text, conflicts: &[ConflictInfo]) {
    for conflict in conflicts {
        t.line(format_args!(
            "{} ({}): {}{}",
            "CONFLICT".red().bold(),
            conflict_label(conflict.kind),
            conflict.path,
            hunk_summary(conflict.hunks.len())
        ));
    }
}

fn hunk_summary(hunks: usize) -> String {
    if hunks == 0 {
        String::new()
    } else {
        format!(" ({})", plural(hunks, "hunk", "hunks"))
    }
}

fn render_status(t: &mut Text, status: &MergeStatus) {
    t.line(format_args!(
        "Merging {} ({}) into {} ({})",
        status.source_branch.green(),
        short(&status.source_tip).yellow(),
        status.branch.green(),
        short_or(status.target_tip.as_deref(), "empty").yellow()
    ));
    t.line(format_args!(
        "Base:    {}",
        short_or(status.base.as_deref(), "none")
    ));
    for conflict in &status.conflicts {
        t.line(format_args!(
            "  {:<12} {} ({}){}",
            conflict_state(conflict),
            conflict.path,
            conflict_label(conflict.kind),
            hunk_ranges(conflict)
        ));
    }
    let unresolved = status.unresolved().count();
    if unresolved == 0 {
        t.line("All conflicts resolved; run `wt merge --continue` to finish the merge.");
    } else {
        t.line(format_args!(
            "{} unresolved; use `wt merge resolve PATH --ours|--theirs|--file F`, \
             then `wt merge --continue` (or `wt merge abort`).",
            plural(unresolved, "conflict", "conflicts")
        ));
    }
}

fn conflict_state(conflict: &ConflictStatus) -> colored::ColoredString {
    if conflict.resolved {
        "resolved".green()
    } else {
        "unresolved".red()
    }
}

/// ` [ours 3,2 / theirs 3,1; ...]` line ranges of a text conflict.
fn hunk_ranges(conflict: &ConflictStatus) -> String {
    if conflict.hunks.is_empty() {
        return String::new();
    }
    let ranges: Vec<String> = conflict
        .hunks
        .iter()
        .map(|h| {
            format!(
                "ours {},{} / theirs {},{}",
                h.ours_start, h.ours_len, h.theirs_start, h.theirs_len
            )
        })
        .collect();
    format!(" [{}]", ranges.join("; "))
}

/// Exit code 3 when a merge stopped with conflicts.
pub fn conflict_status(outcome: &MergeOutcome) -> Result<()> {
    match outcome {
        MergeOutcome::Conflicts { conflicts } => Err(CliError::conflicts(format!(
            "automatic merge failed with {}",
            plural(conflicts.len(), "conflict", "conflicts")
        ))
        .with_hint(
            "fix the conflicts (or `wt merge resolve PATH --ours|--theirs`), then run \
             `wt merge --continue` (or `wt merge --abort`)",
        )),
        _ => Ok(()),
    }
}

pub fn conflict_label(kind: ConflictKind) -> &'static str {
    match kind {
        ConflictKind::Content => "content",
        ConflictKind::ModifyDelete => "modify/delete",
        ConflictKind::AddAdd => "add/add",
        ConflictKind::Binary => "binary",
    }
}

pub fn revert(ctx: &Context, args: RevertArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let opts = RevertOptions {
        message: args.message,
        parent: args.parent,
    };
    let snapshot = repo.revert_with(&tree.name, &args.rev, &opts)?;
    ctx.out.emit(&snapshot, |t| snapshot_line(t, &snapshot))
}

pub fn restore(ctx: &Context, args: RestoreArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let paths = ctx.tree_paths(&repo, &tree, &args.paths)?;
    let restored = repo.restore(&tree.name, &paths, args.from.as_deref())?;
    ctx.out.emit(&restored, |t| {
        if restored.is_empty() {
            t.line("Nothing to restore.");
            return;
        }
        for path in &restored {
            t.line(format_args!("restored: {path}"));
        }
        t.line(format_args!(
            "Restored {}.",
            plural(restored.len(), "file", "files")
        ));
    })
}

#[cfg(test)]
mod tests {
    use clap::Parser;
    use worktree_sdk::ConflictHunk;

    use super::*;

    fn info(path: &str, kind: ConflictKind) -> ConflictInfo {
        ConflictInfo {
            path: path.into(),
            kind,
            hunks: Vec::new(),
        }
    }

    #[test]
    fn conflicts_exit_with_code_3() {
        let outcome = MergeOutcome::Conflicts {
            conflicts: vec![
                info("a.txt", ConflictKind::Content),
                info("b.bin", ConflictKind::Binary),
            ],
        };
        let err = conflict_status(&outcome).expect_err("conflicts");
        assert_eq!(err.exit_code(), 3);
        assert!(err.to_string().contains("2 conflicts"), "{err}");
    }

    #[test]
    fn clean_outcomes_succeed() {
        assert!(conflict_status(&MergeOutcome::UpToDate).is_ok());
        assert!(conflict_status(&MergeOutcome::FastForward { to: "abc".into() }).is_ok());
    }

    #[test]
    fn renders_conflicts() {
        colored::control::set_override(false);
        let outcome = MergeOutcome::Conflicts {
            conflicts: vec![info("a.txt", ConflictKind::ModifyDelete)],
        };
        let mut t = Text::default();
        render_outcome(&mut t, &outcome);
        assert_eq!(t.as_str(), "CONFLICT (modify/delete): a.txt\n");

        let mut with_hunks = info("b.txt", ConflictKind::Content);
        with_hunks.hunks = vec![
            ConflictHunk {
                ours_start: 1,
                ours_len: 1,
                theirs_start: 1,
                theirs_len: 2,
            };
            2
        ];
        let mut t = Text::default();
        render_outcome(
            &mut t,
            &MergeOutcome::Conflicts {
                conflicts: vec![with_hunks],
            },
        );
        assert_eq!(t.as_str(), "CONFLICT (content): b.txt (2 hunks)\n");

        let mut t = Text::default();
        render_outcome(
            &mut t,
            &MergeOutcome::FastForward {
                to: "0123456789abcdef".into(),
            },
        );
        assert_eq!(t.as_str(), "Fast-forward to 01234567\n");
    }

    #[test]
    fn renders_status() {
        colored::control::set_override(false);
        let status = MergeStatus {
            tree: "proj".into(),
            branch: "main".into(),
            source_branch: "feature".into(),
            source_tip: "5e6f7a8b99".into(),
            target_tip: Some("1a2b3c4d99".into()),
            base: None,
            message: "Merge feature into main".into(),
            conflicts: vec![
                ConflictStatus {
                    path: "a.txt".into(),
                    kind: ConflictKind::Content,
                    hunks: vec![ConflictHunk {
                        ours_start: 2,
                        ours_len: 1,
                        theirs_start: 2,
                        theirs_len: 3,
                    }],
                    resolved: false,
                },
                ConflictStatus {
                    path: "b.bin".into(),
                    kind: ConflictKind::Binary,
                    hunks: Vec::new(),
                    resolved: true,
                },
            ],
        };
        let mut t = Text::default();
        render_status(&mut t, &status);
        let out = t.as_str();
        assert!(
            out.starts_with("Merging feature (5e6f7a8b) into main (1a2b3c4d)\nBase:    none\n"),
            "{out}"
        );
        assert!(
            out.contains("  unresolved   a.txt (content) [ours 2,1 / theirs 2,3]\n"),
            "{out}"
        );
        assert!(out.contains("  resolved     b.bin (binary)\n"), "{out}");
        assert!(out.contains("1 conflict unresolved"), "{out}");
    }

    #[test]
    fn labels() {
        assert_eq!(conflict_label(ConflictKind::Content), "content");
        assert_eq!(conflict_label(ConflictKind::AddAdd), "add/add");
    }

    #[derive(Debug, Parser)]
    struct Harness {
        #[command(subcommand)]
        cmd: Cmd,
    }

    #[derive(Debug, clap::Subcommand)]
    enum Cmd {
        Merge(MergeArgs),
        Revert(RevertArgs),
    }

    fn merge_args(args: &[&str]) -> std::result::Result<MergeArgs, clap::Error> {
        let mut full = vec!["wt", "merge"];
        full.extend_from_slice(args);
        Harness::try_parse_from(full).map(|h| match h.cmd {
            Cmd::Merge(a) => a,
            Cmd::Revert(_) => unreachable!(),
        })
    }

    #[test]
    fn parses_strategy_and_subcommands() {
        let a = merge_args(&["feature", "--strategy", "theirs"]).unwrap();
        assert_eq!(a.strategy, StrategyArg::Theirs);
        assert_eq!(a.source.as_deref(), Some("feature"));
        assert_eq!(
            merge_args(&["feature"]).unwrap().strategy,
            StrategyArg::Manual
        );
        assert!(merge_args(&["feature", "-s", "recursive"]).is_err());
        assert!(merge_args(&["--continue", "--strategy", "ours"]).is_err());

        let a = merge_args(&["resolve", "a.txt", "b.txt", "--ours"]).unwrap();
        let Some(MergeCommand::Resolve { paths, ours, .. }) = a.action else {
            panic!("resolve")
        };
        assert!(ours);
        assert_eq!(paths, [PathBuf::from("a.txt"), PathBuf::from("b.txt")]);
        let a = merge_args(&["resolve", "--all", "--theirs"]).unwrap();
        assert!(matches!(
            a.action,
            Some(MergeCommand::Resolve {
                all: true,
                theirs: true,
                ..
            })
        ));
        let a = merge_args(&["resolve", "x", "--file", "fixed.txt"]).unwrap();
        assert!(matches!(
            a.action,
            Some(MergeCommand::Resolve { file: Some(_), .. })
        ));
        // A side is required; --ours/--theirs/--file are exclusive; --all
        // takes no paths or file.
        assert!(merge_args(&["resolve", "x"]).is_err());
        assert!(merge_args(&["resolve", "x", "--ours", "--theirs"]).is_err());
        assert!(merge_args(&["resolve", "--ours"]).is_err());
        assert!(merge_args(&["resolve", "--all", "--file", "f"]).is_err());
        assert!(merge_args(&["resolve", "x", "--all", "--ours"]).is_err());

        assert!(matches!(
            merge_args(&["abort"]).unwrap().action,
            Some(MergeCommand::Abort)
        ));
        assert!(matches!(
            merge_args(&["status"]).unwrap().action,
            Some(MergeCommand::Status)
        ));
        assert!(merge_args(&[]).is_err());
    }

    #[test]
    fn parses_revert_options() {
        let h = Harness::try_parse_from(["wt", "revert", "HEAD", "-m", "undo", "--parent", "2"])
            .unwrap();
        let Cmd::Revert(a) = h.cmd else { panic!() };
        assert_eq!(a.rev, "HEAD");
        assert_eq!(a.message.as_deref(), Some("undo"));
        assert_eq!(a.parent, Some(2));
        let h = Harness::try_parse_from(["wt", "revert", "abc", "--mainline", "1"]).unwrap();
        let Cmd::Revert(a) = h.cmd else { panic!() };
        assert_eq!(a.parent, Some(1));
    }
}
