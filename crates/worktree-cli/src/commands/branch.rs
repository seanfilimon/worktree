use super::BranchAction;
use crate::output::format;
use colored::Colorize;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: BranchAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        BranchAction::Create { name } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let branch = worktree_sdk::engine::branch::create_branch(&engine, &name, None)?;
            format::print_success(&format!("Created branch '{}'", branch.name));
            if let Some(tip) = &branch.tip {
                format::print_kv("Base", &tip[..8.min(tip.len())]);
            }
        }
        BranchAction::List => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let (branches, current) = worktree_sdk::engine::branch::list_branches(&engine, None)?;
            format::print_header("Branches");
            for b in &branches {
                let marker = if b.name == current { "* " } else { "  " };
                let tip_display = b.tip.as_ref()
                    .map(|t| t[..8.min(t.len())].to_string())
                    .unwrap_or_else(|| "(no snapshots)".to_string());
                if b.name == current {
                    println!("{}{} ({})", marker, b.name.green().bold(), tip_display);
                } else {
                    println!("{}{} ({})", marker, b.name, tip_display);
                }
            }
            format::print_info(&format!("{} branch(es) total", branches.len()));
        }
        BranchAction::Switch { name } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            worktree_sdk::engine::branch::switch_branch(&engine, &name, None)?;
            format::print_success(&format!("Switched to branch '{}'", name));
        }
        BranchAction::Delete { name } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            worktree_sdk::engine::branch::delete_branch(&engine, &name, None)?;
            format::print_success(&format!("Deleted branch '{}'", name));
        }
    }
    Ok(())
}
