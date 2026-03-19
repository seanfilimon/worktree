use std::path::PathBuf;

use worktree_protocol::core::id::TreeId;

use crate::watcher::debounce::DebouncedEvent;

/// A higher-level, semantically classified event derived from raw file-system
/// change notifications. The engine uses these to decide when and how to
/// create snapshots, branches, or other automated actions.
#[derive(Debug, Clone)]
pub enum SemanticEvent {
    /// One or more source-code files changed within a single tree.
    CodeChange {
        /// The tree that owns the changed files.
        tree_id: TreeId,
        /// The paths that changed.
        paths: Vec<PathBuf>,
    },

    /// A dependency manifest (e.g. `Cargo.toml`, `package.json`) changed.
    DependencyChange {
        /// The tree that owns the manifest.
        tree_id: TreeId,
        /// Path to the dependency file.
        path: PathBuf,
    },

    /// A configuration file (e.g. `.worktree/config.toml`, `.editorconfig`) changed.
    ConfigChange {
        /// The tree that owns the config file.
        tree_id: TreeId,
        /// Path to the configuration file.
        path: PathBuf,
    },

    /// Changes that span multiple trees — for example a shared library
    /// referenced by several sub-trees.
    CrossTreeChange {
        /// The set of trees affected.
        tree_ids: Vec<TreeId>,
        /// The paths that changed.
        paths: Vec<PathBuf>,
    },
}

/// Classify a raw debounced file-system event into a [`SemanticEvent`].
///
/// The classifier inspects file extensions, well-known file names, and path
/// prefixes to decide the semantic category. In the future this will also
/// consult a tree registry to resolve which tree owns each path.
pub fn classify_event(raw: &DebouncedEvent) -> SemanticEvent {
    let _ = raw;
    todo!("inspect raw.path extension / name to classify into SemanticEvent variant")
}
