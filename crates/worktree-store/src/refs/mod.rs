//! Ref storage (lands in WT-PHASE-2).
//!
//! Layout per Storage.md:
//!
//! ```text
//! <store>/refs/branches/<name>   ← snapshot hash, atomic replace
//! <store>/refs/tags/<name>
//! <store>/state/head             ← current branch
//! ```
//!
//! Branch updates are compare-and-swap: callers pass the expected old value
//! and the update fails with `StoreError::RefConflict` on mismatch.
