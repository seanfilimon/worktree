//! `wt gc` — Remove unreachable local objects and compact the local store
//! (docs/ENGINEERING_PLAN.md §14.6).

use clap::Args;
use colored::Colorize;
use worktree_sdk::repo::storage::{GcOptions, GcReport, GC_DEFAULT_PRUNE_DAYS};

use crate::commands::server_common::{bytes, fields};
use crate::context::Context;
use crate::error::Result;
use crate::output::Text;

/// Delete local objects and snapshot records that nothing references.
///
/// Kept: everything reachable from branches (archived ones too), tags,
/// remote-tracking refs, reflog entries newer than the prune window, a
/// pending merge, stash entries and in-progress rebase / cherry-pick state,
/// plus anything written in the last hour (or within the prune window when
/// shorter). Raw objects are rewritten compressed when that saves space,
/// and leftover temp files of interrupted writes are removed.
#[derive(Debug, Args)]
pub struct GcArgs {
    /// Report what would be deleted without changing anything
    #[arg(long, short = 'n')]
    pub dry_run: bool,
    /// Reflog entries newer than DAYS keep their snapshots (0 = only refs,
    /// merge and stash state protect history)
    #[arg(long, value_name = "DAYS", default_value_t = GC_DEFAULT_PRUNE_DAYS)]
    pub prune: u32,
}

pub fn run(ctx: &Context, args: GcArgs) -> Result<()> {
    let repo = ctx.open()?;
    let report = repo.gc(&GcOptions {
        dry_run: args.dry_run,
        prune_days: args.prune,
    })?;
    ctx.out.emit(&report, |t| render(t, &report))
}

fn render(t: &mut Text, r: &GcReport) {
    let verb = if r.dry_run { "would delete" } else { "deleted" };
    if r.dry_run {
        t.line("Dry run: nothing was deleted".yellow());
    }
    let mut rows = vec![
        ("objects scanned", r.objects_scanned.to_string()),
        ("reachable", r.objects_reachable.to_string()),
        ("kept (recent)", r.objects_recent.to_string()),
        (
            verb,
            format!("{} objects ({})", r.objects_deleted, bytes(r.bytes_deleted)),
        ),
        (
            "snapshots",
            format!(
                "{} reachable, {verb} {}",
                r.snapshots_reachable, r.snapshots_deleted
            ),
        ),
    ];
    if !r.dry_run {
        rows.push((
            "compacted",
            format!(
                "{} objects (saved {})",
                r.objects_compacted,
                bytes(r.bytes_compacted)
            ),
        ));
    }
    if r.temp_files_deleted > 0 {
        rows.push(("temp files", r.temp_files_deleted.to_string()));
    }
    rows.push((
        "store size",
        format!("{} -> {}", bytes(r.bytes_before), bytes(r.bytes_after)),
    ));
    rows.push(("prune window", format!("{} days", r.prune_days)));
    fields(t, &rows);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::Command;
    use crate::commands::server_common::testing::parse;

    fn gc(args: &[&str]) -> GcArgs {
        let mut full = vec!["gc"];
        full.extend_from_slice(args);
        match parse(&full) {
            Command::Gc(a) => a,
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn parses_flags() {
        let a = gc(&[]);
        assert!(!a.dry_run);
        assert_eq!(a.prune, 30);
        let a = gc(&["--dry-run", "--prune", "0"]);
        assert!(a.dry_run);
        assert_eq!(a.prune, 0);
        assert!(gc(&["-n"]).dry_run);
    }

    #[test]
    fn renders_reports() {
        colored::control::set_override(false);
        let mut t = Text::default();
        render(
            &mut t,
            &GcReport {
                dry_run: true,
                prune_days: 30,
                objects_scanned: 10,
                objects_reachable: 7,
                objects_recent: 1,
                objects_deleted: 2,
                bytes_deleted: 2048,
                snapshots_reachable: 4,
                snapshots_deleted: 1,
                bytes_before: 4096,
                bytes_after: 2048,
                ..GcReport::default()
            },
        );
        let text = t.as_str();
        assert!(text.starts_with("Dry run: nothing was deleted\n"), "{text}");
        assert!(
            text.contains("would delete     2 objects (2.0 KiB)"),
            "{text}"
        );
        assert!(text.contains("4 reachable, would delete 1"), "{text}");
        assert!(text.contains("4.0 KiB -> 2.0 KiB"), "{text}");
        assert!(!text.contains("compacted"), "{text}");
    }
}
