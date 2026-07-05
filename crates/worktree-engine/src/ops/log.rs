use crate::engine::WorktreeEngine;
use crate::error::Result;
use crate::ops::snapshot;
use crate::persist::SnapshotState;

pub fn show_log(engine: &WorktreeEngine, count: usize) -> Result<Vec<SnapshotState>> {
    snapshot::list_snapshots(engine, None, count)
}
