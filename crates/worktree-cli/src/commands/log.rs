use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(count: usize) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;
    let snapshots = worktree_sdk::engine::log::show_log(&engine, count)?;

    format::print_header("Snapshot Log");

    if snapshots.is_empty() {
        println!();
        format::print_info("No snapshots yet. Create one with `wt snapshot -m \"message\"`");
        return Ok(());
    }

    println!();
    for snap in &snapshots {
        let short_id: String = snap.id.chars().take(8).collect();
        let auto_marker = if snap.auto_generated { " (auto)" } else { "" };
        println!(
            "  {} {}{}",
            format::styled_hash(&short_id),
            snap.message,
            auto_marker
        );
        format::print_kv("    Author", &snap.author);
        format::print_kv("    Date", &snap.timestamp);
        format::print_kv("    Branch", &snap.branch_name);
        format::print_kv("    Tree", &snap.tree_name);
        if !snap.parents.is_empty() {
            let parent_ids: Vec<String> = snap
                .parents
                .iter()
                .map(|p| p.chars().take(8).collect())
                .collect();
            format::print_kv("    Parents", &parent_ids.join(", "));
        }
        format::print_kv("    Files", &format!("{}", snap.files.len()));
        println!();
    }

    format::print_info(&format!("Showing {} snapshot(s)", snapshots.len()));
    Ok(())
}
