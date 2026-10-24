//! # Git Compatibility
//!
//! Provides mapping layers between Worktree's native object model and Git's
//! object model. This enables interoperability with existing Git tooling
//! and repositories.
//!
//! - **git_object_map** — Maps between Worktree objects and Git objects
//! - **git_ref_map** — Maps between Worktree branches/snapshots and Git refs
//! - **git_hash_map** — Bridges BLAKE3 content hashes and Git SHA-1 hashes

pub mod git_object_map;
pub mod git_ref_map;
pub mod git_hash_map;
