//! End-to-end test support.
//!
//! The actual tests live in `tests/`; they spawn the real workspace
//! binaries (built on demand via `escargot`) against `tempfile` dirs so no
//! test ever touches a real worktree or the platform data directory
//! (`WT_STORAGE_DIR` / `WT_WORKER_SOCKET` are pointed into the temp dir).
