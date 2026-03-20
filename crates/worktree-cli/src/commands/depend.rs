use super::DependAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: DependAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        DependAction::Add { tree, target, blocking } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let state = worktree_sdk::engine::status::load_state(&engine)?;

            // Validate source tree exists
            if state.find_tree(&tree).is_none() {
                return Err(format!("Source tree '{}' not found", tree).into());
            }

            // Write dependency to tree's config
            let tree_state = state.find_tree(&tree).unwrap();
            let wt_tree_dir = engine.root().join(&tree_state.path).join(".wt-tree");
            if !wt_tree_dir.exists() {
                std::fs::create_dir_all(&wt_tree_dir)?;
            }

            let config_path = wt_tree_dir.join("config.toml");
            let mut config_content = if config_path.exists() {
                std::fs::read_to_string(&config_path)?
            } else {
                format!("[tree]\nname = \"{}\"\n", tree)
            };

            let blocking_str = if blocking { "true" } else { "false" };
            let dep_entry = format!(
                "\n[[dependencies]]\nname = \"{}\"\npath = \"{}\"\nbranch = \"main\"\nrequired = {}\n",
                target, target, blocking_str
            );
            config_content.push_str(&dep_entry);
            std::fs::write(&config_path, config_content)?;

            format::print_success(&format!(
                "Added dependency: {} -> {}{}",
                tree,
                target,
                if blocking { " (blocking)" } else { "" }
            ));
            format::print_kv("Source", &tree);
            format::print_kv("Target", &target);
            format::print_kv("Blocking", blocking_str);
        }
        DependAction::List => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            format::print_header("Dependencies");
            println!();

            let deps = worktree_sdk::engine::dependency::list_dependencies(&engine)?;
            if deps.is_empty() {
                format::print_info("No dependencies configured.");
                format::print_info("Use `wt depend add <tree> <target>` to add a dependency.");
            } else {
                for dep in &deps {
                    format::print_list_item(dep);
                }
                println!();
                format::print_info(&format!("{} dependency(ies) total", deps.len()));
            }

            // Also show any .wt-tree config dependencies
            let state = worktree_sdk::engine::status::load_state(&engine)?;
            for tree in &state.trees {
                if tree.name == "root" {
                    continue;
                }
                let config_path = engine.root().join(&tree.path).join(".wt-tree").join("config.toml");
                if config_path.exists() {
                    let content = std::fs::read_to_string(&config_path)?;
                    if content.contains("[[dependencies]]") {
                        println!();
                        format::print_info(&format!("Dependencies in tree '{}':", tree.name));
                        // Parse and display dependency blocks
                        for line in content.lines() {
                            if line.starts_with("name = ") || line.starts_with("path = ") || line.starts_with("required = ") {
                                format::print_kv("  ", line.trim());
                            }
                        }
                    }
                }
            }
        }
        DependAction::Todo => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            format::print_header("TODO Items");
            println!();

            let state = worktree_sdk::engine::status::load_state(&engine)?;
            let mut todo_count = 0;

            for tree in &state.trees {
                let config_path = engine.root().join(&tree.path).join(".wt-tree").join("config.toml");
                if config_path.exists() {
                    let content = std::fs::read_to_string(&config_path)?;
                    if content.contains("required = true") {
                        todo_count += 1;
                        format::print_list_item(&format!(
                            "[OPEN] Required dependency in tree '{}' — blocking",
                            tree.name
                        ));
                    }
                }
            }

            if todo_count == 0 {
                format::print_info("No TODO items.");
                format::print_info("TODOs are automatically generated from blocking dependencies.");
            } else {
                println!();
                format::print_info(&format!("{} TODO item(s)", todo_count));
            }
        }
    }
    Ok(())
}
