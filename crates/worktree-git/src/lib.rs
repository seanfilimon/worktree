//! # worktree-git
//!
//! Git compatibility layer for W0rkTree, built on `worktree-sdk` and libgit2.
//!
//! - [`bridge`]: `wt git import` / `wt git export` (commit DAG <-> snapshots).
//! - [`map`]: the persistent snapshot-id <-> commit-sha mapping (`.wt/git-map.json`).
//! - [`ident`]: deterministic snapshot ids and author/signature conversion.
//! - [`remote`]: push an exported Git repository to a URL / mirror-fetch one.
//! - [`source`]: open an import source (local path, or a URL mirrored to a
//!   temporary directory).
//! - [`license`]: license rules, the redistribution filter and the
//!   generated `LICENSES/NOTICE` for license-aware exports.
//! - [`config`]: `.gitignore` and `.gitattributes` handling.
//! - [`hash_index`]: BLAKE3 <-> Git SHA-1 blob index used during export.
//! - [`lfs`]: Git LFS pointer detection and content substitution on import.
//! - [`submodule`]: submodule pins (`.wtsubmodules`) and nested-tree import.

pub mod bridge;
pub mod config;
pub mod error;
pub mod hash_index;
pub mod ident;
pub mod lfs;
pub mod license;
pub mod map;
pub mod remote;
pub mod source;
pub mod submodule;

mod export;
mod import;

#[cfg(test)]
mod test_support;

pub use bridge::{
    export_repository, import_into_tree, import_repository, push_repository, ExportOptions,
    ExportReport, ImportOptions, ImportReport, LfsMode, LfsReport, PushRepoOptions, PushRepoReport,
    SubmoduleMode, SubmoduleReport,
};
pub use error::{GitCompatError, Result};
pub use map::GitMap;
