use std::path::Path;
use worktree_sdk::WorktreeEngine;
use crate::output::format;

pub async fn execute(
    tree: Option<String>,
    message: String,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;
    let tree_display = tree.as_deref().unwrap_or("(current)");

    format::print_info(&format!(
        "Creating snapshot on tree '{}': \"{}\"",
        tree_display, message
    ));

    let snapshot = worktree_sdk::engine::snapshot::create_snapshot(
        &engine,
        tree.as_deref(),
        &message,
    )?;

    format::print_success(&format!(
        "Snapshot created: {}",
        &snapshot.id[..snapshot.id.len().min(8)]
    ));
    format::print_kv("ID", &snapshot.id);
    format::print_kv("Message", &snapshot.message);
    format::print_kv("Author", &snapshot.author);
    format::print_kv("Timestamp", &snapshot.timestamp);
    format::print_kv("Tree", &snapshot.tree_name);
    format::print_kv("Branch", &snapshot.branch_name);
    format::print_kv("Files", &snapshot.files.len().to_string());

    if !snapshot.parents.is_empty() {
        let parents: Vec<String> = snapshot.parents.iter()
            .map(|p| p.chars().take(8).collect::<String>())
            .collect();
        format::print_kv("Parents", &parents.join(", "));
    }

    Ok(())
}
