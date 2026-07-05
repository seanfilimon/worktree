//! Lifecycle hooks (`.wt/hooks/`).
//!
//! Per DotWt.md, the engine runs `pre-snapshot` / `post-snapshot` /
//! `pre-push` hooks around the corresponding operations. Wired up in
//! WT-PHASE-3 alongside the daemon; until then this module only names the
//! hook points so callers have a stable seam.

/// Hook points recognized by the engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    PreSnapshot,
    PostSnapshot,
    PrePush,
}

impl Hook {
    /// File name of the hook script inside `.wt/hooks/`.
    pub fn file_name(&self) -> &'static str {
        match self {
            Hook::PreSnapshot => "pre-snapshot",
            Hook::PostSnapshot => "post-snapshot",
            Hook::PrePush => "pre-push",
        }
    }
}
