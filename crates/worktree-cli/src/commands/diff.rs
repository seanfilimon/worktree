use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;
use worktree_sdk::engine::diff::{DiffStatus, DiffEntry};

pub async fn execute(
    from: Option<String>,
    to: Option<String>,
    name_only: bool,
    stat: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;

    let entries: Vec<DiffEntry> = match (from.as_deref(), to.as_deref()) {
        (None, None) | (Some("working"), None) => {
            // Diff working tree against last snapshot
            worktree_sdk::engine::diff::diff_working_tree(&engine)?
        }
        (Some(from_id), Some(to_id)) => {
            // Diff two snapshots
            worktree_sdk::engine::diff::diff_snapshots(&engine, from_id, to_id)?
        }
        (Some(from_id), None) => {
            // Diff snapshot against working tree (treat as working tree diff for now)
            if from_id == "working" {
                worktree_sdk::engine::diff::diff_working_tree(&engine)?
            } else {
                format::print_info(&format!("Showing changes since snapshot {}...", &from_id[..from_id.len().min(8)]));
                worktree_sdk::engine::diff::diff_working_tree(&engine)?
            }
        }
        (None, Some(to_id)) => {
            format::print_info(&format!("Showing changes up to snapshot {}...", &to_id[..to_id.len().min(8)]));
            worktree_sdk::engine::diff::diff_working_tree(&engine)?
        }
    };

    if entries.is_empty() {
        format::print_success("No differences found.");
        return Ok(());
    }

    format::print_header("Differences");
    println!();

    let mut added_count = 0usize;
    let mut modified_count = 0usize;
    let mut deleted_count = 0usize;

    for entry in &entries {
        match entry.status {
            DiffStatus::Added => added_count += 1,
            DiffStatus::Modified => modified_count += 1,
            DiffStatus::Deleted => deleted_count += 1,
            DiffStatus::Renamed(_) => modified_count += 1,
        }

        if name_only {
            let prefix = status_prefix(&entry.status);
            println!("  {} {}", prefix, entry.path);
        } else if stat {
            let prefix = status_prefix(&entry.status);
            let size_info = match (&entry.old_size, &entry.new_size) {
                (Some(old), Some(new)) => {
                    let diff = *new as i64 - *old as i64;
                    if diff >= 0 {
                        format!("+{} bytes", diff)
                    } else {
                        format!("{} bytes", diff)
                    }
                }
                (None, Some(new)) => format!("+{} bytes", new),
                (Some(old), None) => format!("-{} bytes", old),
                (None, None) => String::new(),
            };
            println!("  {} {} {}", prefix, entry.path, size_info);
        } else {
            print_diff_entry(entry);
        }
    }

    println!();
    format::print_info(&format!(
        "{} file(s) changed: {} added, {} modified, {} deleted",
        entries.len(),
        added_count,
        modified_count,
        deleted_count
    ));

    Ok(())
}

fn status_prefix(status: &DiffStatus) -> &'static str {
    match status {
        DiffStatus::Added => "+",
        DiffStatus::Modified => "~",
        DiffStatus::Deleted => "-",
        DiffStatus::Renamed(_) => "→",
    }
}

fn print_diff_entry(entry: &DiffEntry) {
    let prefix = status_prefix(&entry.status);
    let label = match &entry.status {
        DiffStatus::Added => "added",
        DiffStatus::Modified => "modified",
        DiffStatus::Deleted => "deleted",
        DiffStatus::Renamed(new_name) => {
            println!("  {} {} → {} (renamed)", prefix, entry.path, new_name);
            return;
        }
    };
    println!("  {} {} ({})", prefix, entry.path, label);

    if let (Some(old_hash), Some(new_hash)) = (&entry.old_hash, &entry.new_hash) {
        println!(
            "    {} → {}",
            &old_hash[..old_hash.len().min(8)],
            &new_hash[..new_hash.len().min(8)]
        );
    }
}
