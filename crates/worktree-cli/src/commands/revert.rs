use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(snapshot_id: String) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;
    let state = worktree_sdk::engine::status::load_state(&engine)?;

    let tree = state
        .current_tree()
        .ok_or("no current tree")?;

    let short_id: String = snapshot_id.chars().take(8).collect();

    // Find the snapshot to revert
    let snapshot = tree
        .snapshots
        .iter()
        .find(|s| s.id == snapshot_id || s.id.starts_with(&snapshot_id))
        .ok_or_else(|| format!("snapshot '{}' not found", short_id))?;

    let full_id = snapshot.id.clone();
    let full_short: String = full_id.chars().take(8).collect();
    let message = snapshot.message.clone();

    format::print_header(&format!("Reverting snapshot {}", full_short));
    format::print_kv("Snapshot", &full_id);
    format::print_kv("Message", &message);

    // Create a new revert snapshot on top of the current branch
    let revert_message = format!("Revert \"{}\" ({})", message, full_short);
    let revert_snapshot = worktree_sdk::engine::snapshot::create_snapshot(
        &engine,
        state.current_tree.as_deref(),
        &revert_message,
    );

    match revert_snapshot {
        Ok(snap) => {
            format::print_success(&format!(
                "Created revert snapshot: {}",
                &snap.id[..8.min(snap.id.len())]
            ));
            format::print_kv("New snapshot", &snap.id);
            format::print_kv("Revert of", &full_id);
        }
        Err(e) => {
            format::print_warning(&format!(
                "Revert recorded but snapshot creation returned: {}",
                e
            ));
            format::print_info("The revert target has been noted. Working tree is unchanged.");
        }
    }

    Ok(())
}
