use super::snapshot;
use super::status::SnapshotState;
use crate::error::Result;

pub fn show_log(engine: &super::WorktreeEngine, count: usize) -> Result<Vec<SnapshotState>> {
    snapshot::list_snapshots(engine, None, count)
}
