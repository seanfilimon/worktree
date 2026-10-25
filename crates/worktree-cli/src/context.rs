//! Per-invocation state: global flags, the working directory, and helpers
//! for opening the repository and selecting the tree to operate on.

use std::future::Future;
use std::path::{Path, PathBuf};

use worktree_sdk::remote::RestClient;
use worktree_sdk::{Repository, TreeInfo};

use crate::cli::GlobalArgs;
use crate::error::{CliError, Result};
use crate::output::Output;
use crate::paths;

pub struct Context {
    pub out: Output,
    cwd: PathBuf,
    tree: Option<String>,
    server: Option<String>,
}

impl Context {
    /// Must be called after `-C` has been applied.
    pub fn new(global: &GlobalArgs) -> Result<Self> {
        let cwd = std::env::current_dir()
            .map_err(|e| CliError::failure(format!("cannot read the current directory: {e}")))?;
        Ok(Self {
            out: Output::new(global.json),
            cwd,
            tree: global.tree.clone(),
            server: global.server.clone(),
        })
    }

    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// Resolve a user-supplied path against the current directory.
    pub fn absolute(&self, path: &Path) -> PathBuf {
        self.cwd.join(path)
    }

    /// Open the worktree containing the current directory.
    pub fn open(&self) -> Result<Repository> {
        Ok(Repository::open(&self.cwd)?)
    }

    /// local-config (§13.9): the explicit global `--tree` flag, if given
    /// (`wt config set --tree NAME` writes that tree's config).
    pub fn tree_flag(&self) -> Option<&str> {
        self.tree.as_deref()
    }

    /// The tree selected by `--tree`, else the tree containing the cwd.
    pub fn select_tree(&self, repo: &Repository) -> Result<TreeInfo> {
        Ok(match &self.tree {
            Some(name) => repo.tree(name)?,
            None => repo.tree_for_path(&self.cwd)?,
        })
    }

    /// Open the repository and select the tree.
    pub fn open_tree(&self) -> Result<(Repository, TreeInfo)> {
        let repo = self.open()?;
        let tree = self.select_tree(&repo)?;
        Ok((repo, tree))
    }

    /// Convert cwd-relative paths into paths relative to `tree`'s directory.
    /// A path naming the tree directory itself selects everything, which the
    /// SDK spells as an empty list.
    pub fn tree_paths(
        &self,
        repo: &Repository,
        tree: &TreeInfo,
        inputs: &[PathBuf],
    ) -> Result<Vec<String>> {
        let base = repo.tree_dir(&tree.name)?;
        let mut out = Vec::with_capacity(inputs.len());
        for input in inputs {
            let rel = paths::relative_to(&base, &self.cwd, input)?;
            if rel.is_empty() {
                return Ok(Vec::new());
            }
            out.push(rel);
        }
        Ok(out)
    }

    /// REST client for server-side commands: `--server` / `WT_SERVER`, else
    /// the origin remote of the worktree containing the cwd (if any), else
    /// the only server with a saved login.
    pub fn rest(&self) -> Result<RestClient> {
        let repo = Repository::open(&self.cwd).ok();
        Ok(RestClient::resolve(
            self.server.as_deref(),
            repo.as_ref(),
            "origin",
        )?)
    }

    /// Convert a cwd-relative path into a path relative to the worktree root.
    pub fn root_path(&self, repo: &Repository, input: &Path) -> Result<String> {
        paths::relative_to(repo.root(), &self.cwd, input)
    }
}

/// Run an async SDK call to completion on a small multi-threaded runtime
/// (multi-threaded so SDK code may use `block_in_place`).
pub fn block_on<F: Future>(future: F) -> Result<F::Output> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| CliError::failure(format!("cannot start the async runtime: {e}")))?;
    Ok(runtime.block_on(future))
}
