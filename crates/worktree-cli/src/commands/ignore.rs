use super::IgnoreAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: IgnoreAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        IgnoreAction::List => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let patterns = worktree_sdk::engine::ignore::list_ignored(&engine)?;
            format::print_header("Ignore Patterns");
            if patterns.is_empty() {
                format::print_info("No ignore patterns configured.");
            } else {
                for pattern in &patterns {
                    format::print_list_item(pattern);
                }
                println!();
                format::print_info(&format!("{} pattern(s) active", patterns.len()));
            }
        }
        IgnoreAction::Add { pattern } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let ignore_path = engine.wt_dir().join("ignore");
            let mut content = std::fs::read_to_string(&ignore_path).unwrap_or_default();
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&pattern);
            content.push('\n');
            std::fs::write(&ignore_path, content)?;
            format::print_success(&format!("Added ignore pattern: {}", pattern));
        }
    }
    Ok(())
}
