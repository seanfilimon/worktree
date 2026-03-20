use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(branch: String, strategy: String) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;

    format::print_header(&format!("Merging branch '{}' (strategy: {})", branch, strategy));

    match worktree_sdk::engine::merge::merge_branch(&engine, &branch) {
        Ok(result) => {
            format::print_success(&format!(
                "Merged branch '{}' into current branch",
                branch
            ));
            format::print_kv("Snapshot", &result.snapshot.id[..result.snapshot.id.len().min(8)]);
            format::print_kv("Files merged", &result.files_merged.to_string());
            if !result.conflicts.is_empty() {
                format::print_warning(&format!("{} conflict(s) resolved", result.conflicts.len()));
                for conflict in &result.conflicts {
                    format::print_list_item(conflict);
                }
            }
        }
        Err(e) => {
            format::print_error(&format!("Merge failed: {}", e));
            return Err(e.into());
        }
    }

    Ok(())
}
