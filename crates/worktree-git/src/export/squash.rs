use serde::{Deserialize, Serialize};
use worktree_protocol::object::snapshot::Snapshot;

/// Controls which snapshots are eligible for squashing during export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SquashMode {
    /// No squashing — every snapshot becomes a Git commit.
    None,
    /// Only squash auto-generated snapshots into their nearest manual snapshot.
    AutoOnly,
    /// Squash all consecutive snapshots into a single Git commit.
    All,
}

impl Default for SquashMode {
    fn default() -> Self {
        Self::None
    }
}

/// Options that govern how snapshot squashing is performed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SquashOptions {
    /// The squash strategy to apply.
    pub mode: SquashMode,
    /// The number of most-recent snapshots to keep unsquashed, regardless of mode.
    /// A value of `0` means the setting is ignored and all eligible snapshots may
    /// be squashed.
    pub keep_last: usize,
}

impl Default for SquashOptions {
    fn default() -> Self {
        Self {
            mode: SquashMode::None,
            keep_last: 0,
        }
    }
}

/// Squash a slice of snapshots according to the provided options, returning
/// the resulting (potentially reduced) list of snapshots.
///
/// # Panics
///
/// This function is not yet implemented and will panic with `todo!()`.
pub fn squash_snapshots(snapshots: &[Snapshot], _options: &SquashOptions) -> Vec<Snapshot> {
    let _ = snapshots;
    todo!("squash_snapshots: apply SquashOptions to collapse snapshot history")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squash_mode_default_is_none() {
        assert_eq!(SquashMode::default(), SquashMode::None);
    }

    #[test]
    fn squash_options_default() {
        let opts = SquashOptions::default();
        assert_eq!(opts.mode, SquashMode::None);
        assert_eq!(opts.keep_last, 0);
    }
}
