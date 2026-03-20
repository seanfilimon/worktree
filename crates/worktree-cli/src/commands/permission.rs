use super::PermissionAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: PermissionAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        PermissionAction::Set {
            tree,
            tenant,
            user,
            allow,
        } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let target = if let Some(t) = &tenant {
                format!("tenant '{}'", t)
            } else if let Some(u) = &user {
                format!("user '{}'", u)
            } else {
                "all".to_string()
            };

            format::print_info(&format!(
                "Setting permission '{}' on tree '{}' for {}...",
                allow, tree, target
            ));

            // Validate the tree exists
            let state = worktree_sdk::engine::status::load_state(&engine)?;
            if state.find_tree(&tree).is_none() {
                return Err(format!("Tree '{}' not found", tree).into());
            }

            // Write permission to access/policies.toml
            let access_dir = engine.wt_dir().join("access");
            std::fs::create_dir_all(&access_dir)?;
            let policies_path = access_dir.join("policies.toml");

            let subject = if let Some(t) = &tenant {
                format!("tenant:{}", t)
            } else if let Some(u) = &user {
                format!("account:{}", u)
            } else {
                "all_authenticated".to_string()
            };

            let policy_entry = format!(
                "\n[[policy]]\nname = \"grant-{}-{}\"\neffect = \"allow\"\nsubjects = [\"{}\"]\nscope = \"tree:{}\"\npermissions = [\"{}\"]\n",
                allow.replace(':', "-"),
                tree,
                subject,
                tree,
                allow,
            );

            let mut content = if policies_path.exists() {
                std::fs::read_to_string(&policies_path)?
            } else {
                "# W0rkTree Access Policies\n".to_string()
            };
            content.push_str(&policy_entry);
            std::fs::write(&policies_path, content)?;

            format::print_kv("Tree", &tree);
            format::print_kv("Subject", &subject);
            format::print_kv("Permission", &allow);
            format::print_success("Permission set successfully.");
        }
        PermissionAction::Get { tree } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            format::print_header(&format!("Permissions for tree '{}'", tree));

            let policies_path = engine.wt_dir().join("access").join("policies.toml");
            if policies_path.exists() {
                let content = std::fs::read_to_string(&policies_path)?;
                let lines: Vec<&str> = content.lines().collect();
                let mut found = false;
                let mut i = 0;
                while i < lines.len() {
                    if lines[i].starts_with("[[policy]]") {
                        // Look ahead for scope matching this tree
                        let mut block_end = i + 1;
                        let mut matches_tree = false;
                        let mut name = String::new();
                        let mut effect = String::new();
                        let mut subjects = String::new();
                        let mut permissions = String::new();

                        while block_end < lines.len() && !lines[block_end].starts_with("[[") {
                            let line = lines[block_end].trim();
                            if line.contains(&format!("tree:{}", tree)) {
                                matches_tree = true;
                            }
                            if let Some(val) = line.strip_prefix("name = ") {
                                name = val.trim_matches('"').to_string();
                            }
                            if let Some(val) = line.strip_prefix("effect = ") {
                                effect = val.trim_matches('"').to_string();
                            }
                            if let Some(val) = line.strip_prefix("subjects = ") {
                                subjects = val.to_string();
                            }
                            if let Some(val) = line.strip_prefix("permissions = ") {
                                permissions = val.to_string();
                            }
                            block_end += 1;
                        }

                        if matches_tree {
                            found = true;
                            println!();
                            format::print_list_item(&format!("{} ({}) — {} → {}",
                                name, effect, subjects, permissions));
                        }
                        i = block_end;
                    } else {
                        i += 1;
                    }
                }

                if !found {
                    format::print_info("No permissions configured for this tree.");
                }
            } else {
                format::print_info("No permissions configured yet.");
            }
        }
        PermissionAction::List => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            format::print_header("All Permissions");

            let policies_path = engine.wt_dir().join("access").join("policies.toml");
            if policies_path.exists() {
                let content = std::fs::read_to_string(&policies_path)?;
                if content.trim().is_empty() || !content.contains("[[policy]]") {
                    format::print_info("No permissions configured yet.");
                } else {
                    println!();
                    println!("{}", content);
                }
            } else {
                format::print_info("No permissions configured yet. Use `wt permission set` to add permissions.");
            }

            // Also show roles if they exist
            let roles_path = engine.wt_dir().join("access").join("roles.toml");
            if roles_path.exists() {
                let content = std::fs::read_to_string(&roles_path)?;
                if !content.trim().is_empty() {
                    println!();
                    format::print_header("Custom Roles");
                    println!("{}", content);
                }
            }
        }
    }
    Ok(())
}
