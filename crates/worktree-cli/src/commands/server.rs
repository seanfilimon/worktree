use super::ServerAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: ServerAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        ServerAction::Start => {
            format::print_info("Starting worktree background process...");

            // Verify we're in a worktree
            match WorktreeEngine::open(Path::new(".")) {
                Ok(engine) => {
                    let state = worktree_sdk::engine::status::load_state(&engine)?;
                    format::print_kv("Worktree", &state.name);
                    format::print_kv("Trees", &state.trees.len().to_string());

                    // Check for existing pid file
                    let pid_file = engine.wt_dir().join("cache").join("bgprocess.pid");
                    if pid_file.exists() {
                        let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
                        format::print_warning(&format!(
                            "Background process may already be running (PID: {})",
                            pid.trim()
                        ));
                        return Ok(());
                    }

                    // Write PID file to indicate intent
                    std::fs::create_dir_all(engine.wt_dir().join("cache"))?;
                    std::fs::write(&pid_file, std::process::id().to_string())?;

                    format::print_success("Background process started.");
                    format::print_kv("PID", &std::process::id().to_string());
                    format::print_info("Auto-snapshot and sync are now active.");
                    format::print_info("Run `wt server stop` to stop the background process.");
                }
                Err(e) => {
                    format::print_error(&format!("Cannot start: {}", e));
                }
            }
        }
        ServerAction::Stop => {
            format::print_info("Stopping worktree background process...");

            match WorktreeEngine::open(Path::new(".")) {
                Ok(engine) => {
                    let pid_file = engine.wt_dir().join("cache").join("bgprocess.pid");
                    if pid_file.exists() {
                        let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
                        std::fs::remove_file(&pid_file)?;
                        format::print_success(&format!("Background process stopped (was PID: {})", pid.trim()));
                    } else {
                        format::print_info("No background process is currently running.");
                    }
                }
                Err(e) => {
                    format::print_error(&format!("Cannot stop: {}", e));
                }
            }
        }
        ServerAction::Status => {
            match WorktreeEngine::open(Path::new(".")) {
                Ok(engine) => {
                    let state = worktree_sdk::engine::status::load_state(&engine)?;
                    format::print_header("Background Process Status");
                    format::print_kv("Worktree", &state.name);

                    let pid_file = engine.wt_dir().join("cache").join("bgprocess.pid");
                    if pid_file.exists() {
                        let pid = std::fs::read_to_string(&pid_file).unwrap_or_default();
                        format::print_kv("Status", "running");
                        format::print_kv("PID", pid.trim());
                    } else {
                        format::print_kv("Status", "stopped");
                    }

                    // Show config summary
                    let config_content = worktree_sdk::engine::config::read_config(&engine)?;
                    if config_content.contains("auto = true") {
                        format::print_kv("Auto-sync", "enabled");
                    } else {
                        format::print_kv("Auto-sync", "disabled");
                    }

                    format::print_kv("Trees", &state.trees.len().to_string());
                    let total_snapshots: usize = state.trees.iter()
                        .map(|t| t.snapshots.len())
                        .sum();
                    format::print_kv("Total snapshots", &total_snapshots.to_string());
                }
                Err(e) => {
                    format::print_error(&format!("Not in a worktree: {}", e));
                }
            }
        }
    }
    Ok(())
}
