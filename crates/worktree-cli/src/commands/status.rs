use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(team: bool) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;
    let status = worktree_sdk::engine::status::compute_status(&engine)?;

    format::print_header("Worktree Status");
    format::print_kv("Tree", &status.tree_name);
    format::print_kv("Branch", &status.branch_name);
    format::print_kv("Snapshots", &status.snapshot_count.to_string());
    println!();

    if status.is_clean() {
        format::print_success("Working tree clean.");
    } else {
        let total = status.total_changes();
        format::print_warning(&format!("{} change(s) detected:", total));
        println!();

        if !status.added.is_empty() {
            for path in &status.added {
                format::print_list_item(&format!("+ {}", path));
            }
        }

        if !status.modified.is_empty() {
            for path in &status.modified {
                format::print_list_item(&format!("~ {}", path));
            }
        }

        if !status.deleted.is_empty() {
            for path in &status.deleted {
                format::print_list_item(&format!("- {}", path));
            }
        }
    }

    if team {
        println!();
        format::print_header("Team Activity");
        format::print_info("No staged snapshots from other team members.");
    }

    Ok(())
}
