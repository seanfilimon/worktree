//! JSON-document state backend (`.wt/state.json`).
//!
//! Whole-document load/save with atomic replace. Slated for replacement by
//! the content-addressable store in WT-PHASE-2.

use super::WorktreeState;
use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};

/// Load state from disk
pub fn load_state(engine: &WorktreeEngine) -> Result<WorktreeState> {
    let state_file = engine.state_file();
    if !state_file.exists() {
        return Err(EngineError::NotAWorktree);
    }
    let content = std::fs::read_to_string(&state_file)?;
    serde_json::from_str(&content).map_err(|e| EngineError::Serialization(e.to_string()))
}

/// Save state to disk (atomic write via temp file + rename)
pub fn save_state(engine: &WorktreeEngine, state: &WorktreeState) -> Result<()> {
    let state_file = engine.state_file();
    let tmp_file = state_file.with_extension("json.tmp");
    let content = serde_json::to_string_pretty(state)
        .map_err(|e| EngineError::Serialization(e.to_string()))?;
    std::fs::write(&tmp_file, &content)?;
    std::fs::rename(&tmp_file, &state_file)?;
    Ok(())
}
