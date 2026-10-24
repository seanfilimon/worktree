use crate::error::Result;
use super::snapshot;
use super::status::SnapshotState;

pub fn show_log(
    engine: &super::WorktreeEngine,
    count: usize,
) -> Result<Vec<SnapshotState>> {
    snapshot::list_snapshots(engine, None, count)
}
