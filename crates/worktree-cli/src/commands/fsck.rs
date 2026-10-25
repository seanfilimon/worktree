//! `wt fsck` — Verify the integrity of the local store and history
//! (docs/ENGINEERING_PLAN.md §14.6).

use clap::Args;
use colored::Colorize;
use worktree_sdk::repo::storage::{FsckIssueKind, FsckOptions, FsckReport};

use crate::commands::server_common::fields;
use crate::context::Context;
use crate::error::{CliError, Result};
use crate::output::format::plural;
use crate::output::Text;

/// Check that snapshot records parse, their parents and manifests are
/// stored, manifests and chunk lists decode, every referenced blob and
/// chunk is present (restricted paths excepted) and every branch, tag and
/// remote-tracking ref names a stored snapshot. Exits 1 when a problem is
/// found.
#[derive(Debug, Args)]
pub struct FsckArgs {
    /// Also read back and re-hash every stored object
    #[arg(long)]
    pub full: bool,
}

pub fn run(ctx: &Context, args: FsckArgs) -> Result<()> {
    let repo = ctx.open()?;
    let report = repo.fsck(&FsckOptions { full: args.full })?;
    ctx.out.emit(&report, |t| render(t, &report))?;
    if report.is_ok() {
        return Ok(());
    }
    Err(CliError::failure(format!(
        "fsck found {}",
        plural(report.errors.len(), "problem", "problems")
    ))
    .with_hint(
        "`wt fetch` can restore objects the server has; `wt gc` never deletes reachable objects",
    ))
}

fn label(kind: FsckIssueKind) -> &'static str {
    match kind {
        FsckIssueKind::CorruptObject => "corrupt object",
        FsckIssueKind::MissingObject => "missing object",
        FsckIssueKind::MissingManifest => "missing manifest",
        FsckIssueKind::BadManifest => "bad manifest",
        FsckIssueKind::BadChunkList => "bad chunk list",
        FsckIssueKind::BadSnapshot => "bad snapshot",
        FsckIssueKind::MissingParent => "missing parent",
        FsckIssueKind::DanglingRef => "dangling ref",
    }
}

fn render(t: &mut Text, r: &FsckReport) {
    for e in &r.errors {
        t.line(format_args!(
            "{} {}: {}",
            format!("{}:", label(e.kind)).red().bold(),
            e.id,
            e.detail
        ));
    }
    if !r.errors.is_empty() {
        t.blank();
    }
    let mut rows = vec![
        ("snapshots", r.snapshots_checked.to_string()),
        ("manifests", r.manifests_checked.to_string()),
        ("refs", r.refs_checked.to_string()),
    ];
    if r.full {
        rows.push(("objects hashed", r.objects_checked.to_string()));
    }
    if r.restricted_missing > 0 {
        rows.push((
            "restricted",
            format!("{} withheld blobs (expected)", r.restricted_missing),
        ));
    }
    fields(t, &rows);
    if r.errors.is_empty() {
        t.line("No problems found".green());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::parse;
    use worktree_sdk::repo::storage::FsckIssue;

    #[test]
    fn parses_full_flag() {
        match parse(&["fsck"]) {
            Command::Fsck(a) => assert!(!a.full),
            other => panic!("{other:?}"),
        }
        match parse(&["fsck", "--full"]) {
            Command::Fsck(a) => assert!(a.full),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn renders_problems_and_clean_reports() {
        colored::control::set_override(false);
        let mut t = Text::default();
        render(
            &mut t,
            &FsckReport {
                full: true,
                objects_checked: 5,
                snapshots_checked: 2,
                manifests_checked: 2,
                refs_checked: 1,
                errors: vec![FsckIssue {
                    kind: FsckIssueKind::DanglingRef,
                    id: "demo: tag v1".into(),
                    detail: "points at snapshot x, which is not stored".into(),
                }],
                ..FsckReport::default()
            },
        );
        let text = t.as_str();
        assert!(
            text.starts_with(
                "dangling ref: demo: tag v1: points at snapshot x, which is not stored\n\n"
            ),
            "{text}"
        );
        assert!(text.contains("objects hashed  5"), "{text}");
        assert!(!text.contains("No problems"), "{text}");

        let mut t = Text::default();
        render(&mut t, &FsckReport::default());
        assert!(
            t.as_str().ends_with("No problems found\n"),
            "{}",
            t.as_str()
        );
        assert!(!t.as_str().contains("objects hashed"));
    }
}
