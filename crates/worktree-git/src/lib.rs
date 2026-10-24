//! # worktree-git
//!
//! Git compatibility layer for W0rkTree, built on `worktree-sdk` and libgit2.
//!
//! - [`bridge`]: `wt git import` / `wt git export` (commit DAG <-> snapshots).
//! - [`map`]: the persistent snapshot-id <-> commit-sha mapping (`.wt/git-map.json`).
//! - [`ident`]: deterministic snapshot ids and author/signature conversion.
//! - [`remote`]: push an exported Git repository to a URL / mirror-fetch one.
//! - [`config`]: `.gitignore` and `.gitattributes` handling.
//! - [`hash_index`]: BLAKE3 <-> Git SHA-1 blob index used during export.

pub mod bridge;
pub mod config;
pub mod error;
pub mod hash_index;
pub mod ident;
pub mod map;
pub mod remote;

mod export;
mod import;

#[cfg(test)]
mod test_support;

pub use bridge::{
    export_repository, import_repository, ExportOptions, ExportReport, ImportOptions, ImportReport,
};
pub use error::{GitCompatError, Result};
pub use map::GitMap;
