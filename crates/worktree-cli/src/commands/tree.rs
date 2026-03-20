use super::TreeAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: TreeAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        TreeAction::Add { path } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let tree = worktree_sdk::engine::tree::add_tree(&engine, &path)?;
            format::print_success(&format!("Tree '{}' added at '{}'", tree.name, tree.path));
            Ok(())
        }
        TreeAction::List => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let trees = worktree_sdk::engine::tree::list_trees(&engine)?;
            format::print_header("Trees");
            if trees.is_empty() {
                format::print_info("No trees configured.");
            } else {
                for tree in &trees {
                    let branch_count = tree.branches.len();
                    let snapshot_count = tree.snapshots.len();
                    format::print_list_item(&format!(
                        "{} (path: {}, branch: {}, {} branches, {} snapshots)",
                        tree.name, tree.path, tree.current_branch, branch_count, snapshot_count
                    ));
                }
            }
            Ok(())
        }
        TreeAction::Remove { name } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            worktree_sdk::engine::tree::remove_tree(&engine, &name)?;
            format::print_success(&format!("Tree '{}' removed", name));
            Ok(())
        }
        TreeAction::Status { name } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let trees = worktree_sdk::engine::tree::list_trees(&engine)?;
            let target_name = name.as_deref();

            let filtered: Vec<_> = if let Some(n) = target_name {
                trees.iter().filter(|t| t.name == n).collect()
            } else {
                trees.iter().collect()
            };

            if filtered.is_empty() {
                if let Some(n) = target_name {
                    format::print_error(&format!("Tree '{}' not found", n));
                } else {
                    format::print_info("No trees configured.");
                }
            } else {
                for tree in &filtered {
                    format::print_header(&format!("Tree: {}", tree.name));
                    format::print_kv("Path", &tree.path);
                    format::print_kv("Current branch", &tree.current_branch);
                    format::print_kv("Branches", &tree.branches.len().to_string());
                    format::print_kv("Snapshots", &tree.snapshots.len().to_string());
                    format::print_kv("Tags", &tree.tags.len().to_string());

                    if !tree.branches.is_empty() {
                        println!();
                        format::print_info("Branches:");
                        for branch in &tree.branches {
                            let tip_display = branch.tip.as_deref()
                                .map(|t| &t[..t.len().min(8)])
                                .unwrap_or("(no snapshots)");
                            let marker = if branch.name == tree.current_branch { "* " } else { "  " };
                            println!("  {}{} -> {}", marker, branch.name, tip_display);
                        }
                    }
                }
            }
            Ok(())
        }
    }
}
