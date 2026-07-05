//! W0rkTree content-addressable storage.
//!
//! Implements the on-disk layout from `specs/storage/Storage.md`:
//! a per-worktree store in the platform-native data directory holding
//! content-addressed objects, refs, and sync state. The same CAS primitives
//! back the server's per-tenant storage.
//!
//! WT-PHASE-1 ships the crate skeleton (path resolution + module layout);
//! the object store itself lands in WT-PHASE-2.

pub mod cas;
pub mod codec;
pub mod error;
pub mod paths;
pub mod refs;

pub use error::{Result, StoreError};
