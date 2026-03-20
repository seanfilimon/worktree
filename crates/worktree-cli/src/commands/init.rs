use std::path::Path;
use worktree_sdk::WorktreeEngine;
use crate::output::format;

pub async fn execute(path: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
    let target = path.unwrap_or_else(|| ".".to_string());
    format::print_info(&format!("Initializing worktree at '{}'...", target));

    let target_path = Path::new(&target);
    let engine = WorktreeEngine::init(target_path)?;

    let root_display = engine.root().display();
    format::print_success(&format!("Worktree repository initialized at '{}'", root_display));
    format::print_kv("Location", &root_display.to_string());
    format::print_kv("Default branch", "main");
    format::print_kv("Default tree", "root");

    Ok(())
}
