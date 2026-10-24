use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(count: usize) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;
    let entries = worktree_sdk::engine::reflog::show_reflog(&engine, count)?;

    format::print_header("Reflog");

    if entries.is_empty() {
        println!();
        format::print_info("No reflog entries yet.");
        return Ok(());
    }

    println!();
    for entry in &entries {
        format::print_list_item(entry);
    }

    println!();
    format::print_info(&format!("Showing {} reflog entry(ies)", entries.len()));
    Ok(())
}
