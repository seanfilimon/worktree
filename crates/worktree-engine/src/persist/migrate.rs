//! One-shot migration from the legacy `.wt/state.json` backend to the CAS.
//!
//! The JSON backend stored snapshot metadata and file *hashes* but never
//! file *contents*, so migrated snapshots carry manifests whose blobs are
//! absent from the store — history metadata is preserved, historical bytes
//! were never recorded. Content storage begins with the first post-migration
//! snapshot.
//!
//! Snapshot ids change during migration (UUIDs → content hashes); parents,
//! branch tips, and tag targets are remapped consistently.

use super::cas::{self, Meta, TreeMeta};
use super::WorktreeState;
use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use std::collections::HashMap;

/// Migrate `.wt/state.json` into the store if the store is uninitialized.
///
/// Returns `true` if a migration ran.
pub fn migrate_if_needed(engine: &WorktreeEngine) -> Result<bool> {
    let store = cas::open_store(engine)?;
    if cas::read_meta(&store)?.is_some() {
        return Ok(false);
    }

    // Serialize concurrent migrators, then re-check under the lock.
    let state_dir = store
        .state()
        .map_err(|e| EngineError::Serialization(e.to_string()))?;
    let _lock = state_dir
        .lock_exclusive(std::time::Duration::from_secs(5))
        .map_err(|e| EngineError::Serialization(e.to_string()))?;
    if cas::read_meta(&store)?.is_some() {
        return Ok(false);
    }

    let state_file = engine.state_file();
    if !state_file.exists() {
        // Neither store meta nor legacy state: not an initialized worktree.
        return Err(EngineError::NotAWorktree);
    }

    let contents = std::fs::read_to_string(&state_file)?;
    let old: WorktreeState =
        serde_json::from_str(&contents).map_err(|e| EngineError::Serialization(e.to_string()))?;

    tracing::info!("migrating legacy state.json to content-addressable store");
    migrate_state(engine, &old)?;

    std::fs::rename(&state_file, state_file.with_extension("json.migrated"))?;
    Ok(true)
}

fn migrate_state(engine: &WorktreeEngine, old: &WorktreeState) -> Result<()> {
    let mut tree_metas = Vec::with_capacity(old.trees.len());

    for tree in &old.trees {
        // Old snapshot lists are chronological, so parents always precede
        // children and a single forward pass can remap ids.
        let mut id_map: HashMap<String, String> = HashMap::new();

        for snapshot in &tree.snapshots {
            let parents = snapshot
                .parents
                .iter()
                .map(|p| id_map.get(p).cloned().unwrap_or_else(|| p.clone()))
                .collect();
            let migrated = cas::store_migrated_snapshot(engine, snapshot, parents)?;
            id_map.insert(snapshot.id.clone(), migrated);
        }

        for branch in &tree.branches {
            let tip = branch
                .tip
                .as_ref()
                .map(|t| id_map.get(t).cloned().unwrap_or_else(|| t.clone()));
            cas::write_migrated_branch(engine, &tree.name, &branch.name, tip, &branch.created_at)?;
        }

        for tag in &tree.tags {
            let target = id_map
                .get(&tag.target_snapshot)
                .cloned()
                .unwrap_or_else(|| tag.target_snapshot.clone());
            cas::write_migrated_tag(engine, &tree.name, tag, &target)?;
        }

        tree_metas.push(TreeMeta {
            name: tree.name.clone(),
            path: tree.path.clone(),
            current_branch: tree.current_branch.clone(),
        });
    }

    let store = cas::open_store(engine)?;
    cas::write_meta(
        &store,
        &Meta {
            name: old.name.clone(),
            created_at: old.created_at.clone(),
            current_tree: old.current_tree.clone(),
            trees: tree_metas,
        },
    )
}
