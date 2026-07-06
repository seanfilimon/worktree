//! W0rkTree content-addressable storage.
//!
//! Implements the on-disk layout from `specs/storage/Storage.md`:
//! a per-worktree store in the platform-native data directory holding
//! content-addressed objects, refs, and sync state. The same CAS primitives
//! back the server's per-tenant storage.
//!
//! This crate provides **generic storage primitives** — object frames,
//! fan-out CAS, atomic refs, state files, the writer lock. Schema (what a
//! snapshot record contains, what a branch ref holds) is defined by the
//! callers: `worktree-engine` locally, `worktree-server` remotely.

pub mod cas;
pub mod codec;
pub mod error;
pub mod local;
pub mod paths;
pub mod refs;
pub mod state;

pub use cas::{ObjectKind, ObjectStore};
pub use error::{Result, StoreError};
pub use local::LocalStore;
pub use refs::RefStore;
pub use state::{StateDir, StoreLock};
