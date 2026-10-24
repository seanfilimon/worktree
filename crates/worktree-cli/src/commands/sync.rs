use super::SyncAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: SyncAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        SyncAction::Push => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let result = worktree_sdk::engine::sync::push(&engine)?;
            format::print_success(&format!(
                "Pushed to '{}' on branch '{}'",
                result.server, result.branch
            ));
            format::print_kv("Snapshots", &result.snapshots_pushed.to_string());
        }
        SyncAction::Pull => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let result = worktree_sdk::engine::sync::pull(&engine)?;
            if result.up_to_date {
                format::print_success(&format!(
                    "Already up to date on branch '{}'",
                    result.branch
                ));
            } else {
                format::print_success(&format!(
                    "Pulled {} new snapshot(s) on branch '{}'",
                    result.new_snapshots, result.branch
                ));
            }
        }
        SyncAction::Pause => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let sync_state_file = engine.wt_dir().join("cache").join("sync_paused");
            std::fs::create_dir_all(engine.wt_dir().join("cache"))?;
            std::fs::write(&sync_state_file, "paused")?;
            format::print_success("Sync paused.");
            format::print_info("Staged snapshots will be queued locally. Run `wt sync resume` to resume.");
        }
        SyncAction::Resume => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let sync_state_file = engine.wt_dir().join("cache").join("sync_paused");
            if sync_state_file.exists() {
                std::fs::remove_file(&sync_state_file)?;
                format::print_success("Sync resumed.");
                format::print_info("Queued staged snapshots will be uploaded.");
            } else {
                format::print_info("Sync is not paused.");
            }
        }
    }
    Ok(())
}
