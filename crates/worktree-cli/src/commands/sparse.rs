//! `wt sparse` — sparse checkout: check out and fetch only the paths you
//! need (ENGINEERING_PLAN §15.4, `specs/tree/SparseCheckout.md`).

use clap::{Args, Subcommand};
use colored::Colorize;
use serde::Serialize;
use worktree_sdk::repo::{SparseReport, SparseStatus};

use crate::context::Context;
use crate::error::Result;
use crate::output::format::plural;
use crate::output::Text;

/// `wt sparse set|add|remove|list|disable|reapply`
#[derive(Debug, Args)]
#[command(after_long_help = "\
Patterns are relative to the tree root. A pattern without glob characters
names a file or directory and everything below it (`src`, `docs/guide`);
other patterns use gitignore syntax (`*.md`, `/*` for every top-level
entry) and `!pattern` excludes again; the deepest matching level wins.
Files outside the sparse set are not written, not reported by `wt status`
and keep their content in new snapshots. Patterns are stored in
.wt/trees/<tree>/sparse.

In a sparse clone (`wt clone --sparse PATTERN...`) and after a `wt fetch`
or `wt pull` with sparse checkout on, only content of sparse paths is
downloaded; anything else (diff or merge of other paths, `wt sparse add`)
is downloaded on demand, which needs the server to be reachable.")]
pub struct SparseArgs {
    #[command(subcommand)]
    pub action: SparseCommand,
}

#[derive(Debug, Subcommand)]
pub enum SparseCommand {
    /// Enable sparse checkout with exactly these patterns
    Set {
        /// Paths or gitignore-style patterns
        #[arg(required = true, value_name = "PATTERN")]
        patterns: Vec<String>,
    },
    /// Add patterns and check out the files they bring in
    Add {
        #[arg(required = true, value_name = "PATTERN")]
        patterns: Vec<String>,
    },
    /// Remove patterns and the unmodified files that leave the set
    Remove {
        #[arg(required = true, value_name = "PATTERN")]
        patterns: Vec<String>,
    },
    /// Show the patterns and how much of the tree is checked out
    List,
    /// Turn sparse checkout off and check out every file
    Disable,
    /// Make the working directory match the patterns again
    Reapply,
}

#[derive(Serialize)]
struct ListResult<'a> {
    tree: &'a str,
    enabled: bool,
    #[serde(flatten)]
    status: Option<&'a SparseStatus>,
}

pub fn run(ctx: &Context, args: SparseArgs) -> Result<()> {
    let (repo, tree) = ctx.open_tree()?;
    let name = tree.name.as_str();
    let report = match args.action {
        SparseCommand::Set { patterns } => repo.sparse_set(name, &patterns)?,
        SparseCommand::Add { patterns } => repo.sparse_add(name, &patterns)?,
        SparseCommand::Remove { patterns } => repo.sparse_remove(name, &patterns)?,
        SparseCommand::Disable => repo.sparse_disable(name)?,
        SparseCommand::Reapply => repo.sparse_reapply(name)?,
        SparseCommand::List => {
            let status = repo.sparse_status(name)?;
            let result = ListResult {
                tree: name,
                enabled: status.is_some(),
                status: status.as_ref(),
            };
            return ctx
                .out
                .emit(&result, |t| render_list(t, name, status.as_ref()));
        }
    };
    ctx.out.emit(&report, |t| render_report(t, &report))
}

fn render_list(t: &mut Text, tree: &str, status: Option<&SparseStatus>) {
    let Some(status) = status else {
        t.line(format_args!(
            "Sparse checkout is off for {}: every file is checked out.",
            tree.bold()
        ));
        return;
    };
    for p in &status.patterns {
        t.line(p);
    }
    t.line(format_args!("({})", summary(status).dimmed()));
}

/// "3 of 10 files checked out" plus the partial-clone note.
pub(crate) fn summary(status: &SparseStatus) -> String {
    let mut s = format!(
        "{} of {} checked out",
        status.files_checked_out,
        plural(status.files_total, "file", "files")
    );
    if let Some(remote) = &status.partial_remote {
        s.push_str(&format!(
            "; partial clone, other content is downloaded from {remote} on demand"
        ));
    }
    s
}

fn render_report(t: &mut Text, report: &SparseReport) {
    if report.enabled {
        t.line(format_args!(
            "Sparse checkout of {}: {}",
            report.tree.bold(),
            report.patterns.join(" ")
        ));
    } else {
        t.line(format_args!(
            "Sparse checkout of {} is off: every file is checked out.",
            report.tree.bold()
        ));
    }
    let mut parts = vec![
        format!("{} written", plural(report.written, "file", "files")),
        format!("{} removed", report.removed),
    ];
    if report.fetched > 0 {
        parts.push(format!(
            "{} downloaded",
            plural(report.fetched, "object", "objects")
        ));
    }
    t.line(parts.join(", "));
    if !report.kept_modified.is_empty() {
        t.line(format_args!(
            "Kept {} outside the sparse set because they have local changes:",
            plural(
                report.kept_modified.len(),
                "modified file",
                "modified files"
            )
        ));
        for path in &report.kept_modified {
            t.line(format_args!("  {}", path.yellow()));
        }
        t.line("Snapshot or restore them, then run `wt sparse reapply`.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> std::result::Result<crate::cli::Command, clap::Error> {
        use clap::Parser;
        crate::cli::Cli::try_parse_from(std::iter::once("wt").chain(args.iter().copied()))
            .map(|c| c.command)
    }

    #[test]
    fn subcommands_parse() {
        use crate::cli::Command;
        match parse(&["sparse", "set", "src", "docs"]).expect("parse") {
            Command::Sparse(SparseArgs {
                action: SparseCommand::Set { patterns },
            }) => assert_eq!(patterns, vec!["src", "docs"]),
            other => panic!("{other:?}"),
        }
        assert!(parse(&["sparse", "set"]).is_err(), "patterns are required");
        assert!(parse(&["sparse", "add"]).is_err());
        for sub in ["list", "disable", "reapply"] {
            parse(&["sparse", sub]).expect(sub);
        }
        assert!(parse(&["sparse"]).is_err(), "a subcommand is required");
    }

    #[test]
    fn rendering() {
        colored::control::set_override(false);
        let status = SparseStatus {
            patterns: vec!["/src".into()],
            files_total: 10,
            files_checked_out: 3,
            partial_remote: Some("origin".into()),
        };
        let mut t = Text::default();
        render_list(&mut t, "proj", Some(&status));
        assert!(t.as_str().starts_with("/src\n"), "{}", t.as_str());
        assert!(t.as_str().contains("3 of 10 files checked out"));
        assert!(t.as_str().contains("downloaded from origin on demand"));

        let mut t = Text::default();
        render_list(&mut t, "proj", None);
        assert!(t.as_str().contains("is off"));

        let report = SparseReport {
            tree: "proj".into(),
            patterns: vec!["/src".into()],
            enabled: true,
            written: 2,
            removed: 1,
            kept_modified: vec!["docs/x.md".into()],
            fetched: 4,
        };
        let mut t = Text::default();
        render_report(&mut t, &report);
        let out = t.as_str();
        assert!(
            out.contains("2 files written, 1 removed, 4 objects downloaded"),
            "{out}"
        );
        assert!(out.contains("docs/x.md"), "{out}");
    }
}
