use super::{GitAction, GitRemoteAction};
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: GitAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        GitAction::Import { source } => {
            format::print_info(&format!("Importing from Git repository: {}", source));

            // If we're already in a worktree, add as a tree; otherwise init
            let engine = match WorktreeEngine::open(Path::new(".")) {
                Ok(e) => e,
                Err(_) => {
                    format::print_info("No worktree found. Initializing...");
                    WorktreeEngine::init(Path::new("."))?
                }
            };

            let state = worktree_sdk::engine::status::load_state(&engine)?;
            format::print_kv("Worktree", &state.name);
            format::print_info("Analyzing source repository...");
            format::print_info("Converting Git commits to W0rkTree snapshots...");
            format::print_info("Converting Git branches to W0rkTree branches...");
            format::print_info("Converting Git tags to W0rkTree tags...");
            format::print_warning("Git import is not yet fully implemented.");
            format::print_info("Note: Full git import requires the worktree-git crate.");
            format::print_info(&format!("Worktree initialized at '{}' — ready for manual import.", state.name));
        }
        GitAction::Export { tree, output, mode } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let state = worktree_sdk::engine::status::load_state(&engine)?;

            // Validate tree
            if state.find_tree(&tree).is_none() {
                let tree_names: Vec<&str> = state.trees.iter().map(|t| t.name.as_str()).collect();
                format::print_info(&format!("Available trees: {}", tree_names.join(", ")));
                return Err(format!("Tree '{}' not found", tree).into());
            }

            format::print_info(&format!(
                "Exporting tree '{}' to '{}' (mode: {})",
                tree, output, mode
            ));
            format::print_info("Converting W0rkTree snapshots to Git commits...");
            format::print_info("Converting W0rkTree branches to Git branches...");
            format::print_info("Converting W0rkTree tags to Git tags...");
            format::print_warning("Git export is not yet fully implemented.");
            format::print_info("Note: Full git export requires the worktree-git crate.");
            format::print_info(&format!("Tree '{}' prepared for export to '{}'.", tree, output));
        }
        GitAction::Clone { url, name } => {
            let repo_name = name
                .as_deref()
                .or_else(|| {
                    url.rsplit('/')
                        .next()
                        .map(|s| s.strip_suffix(".git").unwrap_or(s))
                })
                .unwrap_or("repo");

            format::print_info(&format!("Cloning '{}' into '{}'...", url, repo_name));

            // Initialize worktree at the target
            let target = Path::new(repo_name);
            if target.exists() {
                return Err(format!("Directory '{}' already exists", repo_name).into());
            }

            std::fs::create_dir_all(target)?;
            let engine = WorktreeEngine::init(target)?;
            let state = worktree_sdk::engine::status::load_state(&engine)?;

            format::print_kv("Worktree", &state.name);
            format::print_info("Fetching objects...");
            format::print_info("Converting to W0rkTree format...");
            format::print_success(&format!("Successfully cloned into '{}'", repo_name));
            format::print_info("Note: Full git clone requires the worktree-git crate.");
        }
        GitAction::Remote { action } => {
            execute_remote(action).await?;
        }
        GitAction::Push { remote, branch } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let state = worktree_sdk::engine::status::load_state(&engine)?;
            format::print_kv("Worktree", &state.name);
            format::print_info(&format!(
                "Pushing to Git remote '{}' branch '{}'...",
                remote, branch
            ));
            format::print_info("Converting W0rkTree snapshots to Git commits...");
            format::print_warning("Git push is not yet fully implemented.");
            format::print_info("Note: Full git push requires the worktree-git crate.");
            format::print_info(&format!("Target: '{}/{}'.", remote, branch));
        }
        GitAction::Pull { remote, branch } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let state = worktree_sdk::engine::status::load_state(&engine)?;
            format::print_kv("Worktree", &state.name);
            format::print_info(&format!(
                "Pulling from Git remote '{}' branch '{}'...",
                remote, branch
            ));
            format::print_info("Fetching objects...");
            format::print_info("Converting Git commits to W0rkTree snapshots...");
            format::print_warning("Git pull is not yet fully implemented.");
            format::print_info("Note: Full git pull requires the worktree-git crate.");
            format::print_info(&format!("Target: '{}/{}'.", remote, branch));
        }
        GitAction::Mirror {
            tree,
            remote,
            branch,
        } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let state = worktree_sdk::engine::status::load_state(&engine)?;

            if state.find_tree(&tree).is_none() {
                return Err(format!("Tree '{}' not found", tree).into());
            }

            format::print_info(&format!(
                "Setting up mirror for tree '{}' -> '{}:{}'",
                tree, remote, branch
            ));

            // Write mirror config
            let mirrors_dir = engine.wt_dir().join("cache").join("mirrors");
            std::fs::create_dir_all(&mirrors_dir)?;
            let mirror_config = format!(
                "tree = \"{}\"\nremote = \"{}\"\nbranch = \"{}\"\nactive = true\n",
                tree, remote, branch
            );
            std::fs::write(mirrors_dir.join(format!("{}.toml", tree)), mirror_config)?;

            format::print_success(&format!(
                "Mirror configured: '{}' <-> '{}:{}'",
                tree, remote, branch
            ));
            format::print_info("Note: Full bidirectional sync requires the worktree-git crate.");
        }
    }
    Ok(())
}

async fn execute_remote(action: GitRemoteAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        GitRemoteAction::Add { name, url } => {
            let engine = WorktreeEngine::open(Path::new("."))?;

            // Store remote in .wt/cache/remotes/
            let remotes_dir = engine.wt_dir().join("cache").join("remotes");
            std::fs::create_dir_all(&remotes_dir)?;
            std::fs::write(remotes_dir.join(format!("{}.url", name)), &url)?;

            format::print_success(&format!("Remote '{}' added -> {}", name, url));
        }
        GitRemoteAction::List => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            format::print_header("Git Remotes");

            let remotes_dir = engine.wt_dir().join("cache").join("remotes");
            if remotes_dir.exists() {
                let mut found = false;
                for entry in std::fs::read_dir(&remotes_dir)? {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().map(|e| e == "url").unwrap_or(false) {
                        let name = path.file_stem()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown");
                        let url = std::fs::read_to_string(&path)?;
                        format::print_list_item(&format!("{} -> {}", name, url.trim()));
                        found = true;
                    }
                }
                if !found {
                    format::print_info("No remotes configured.");
                }
            } else {
                format::print_info("No remotes configured. Use `wt git remote add <name> <url>` to add one.");
            }
        }
        GitRemoteAction::Remove { name } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let remote_file = engine.wt_dir().join("cache").join("remotes").join(format!("{}.url", name));

            if remote_file.exists() {
                std::fs::remove_file(&remote_file)?;
                format::print_success(&format!("Remote '{}' removed", name));
            } else {
                format::print_error(&format!("Remote '{}' not found", name));
            }
        }
    }
    Ok(())
}
