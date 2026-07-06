use crate::engine::WorktreeEngine;
use crate::error::Result;
use crate::persist::{self, DoctorReport};

/// Inspect the worktree's store: object counts and integrity verification.
pub fn run(engine: &WorktreeEngine) -> Result<DoctorReport> {
    persist::doctor(engine)
}
