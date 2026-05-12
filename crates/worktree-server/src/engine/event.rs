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
    let path = &raw.path;
    let tree_id = TreeId::nil();

    let filename = path.file_name().and_then(|n| n.to_str()).unwrap_or("");

    const DEPENDENCY_FILES: &[&str] = &[
        "Cargo.toml",
        "Cargo.lock",
        "package.json",
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "go.mod",
        "go.sum",
        "requirements.txt",
        "pyproject.toml",
        "Pipfile",
        "Pipfile.lock",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
    ];

    if DEPENDENCY_FILES.contains(&filename) {
        return SemanticEvent::DependencyChange {
            tree_id,
            path: path.clone(),
        };
    }

    let is_wt_path = path
        .components()
        .any(|c| matches!(c.as_os_str().to_str(), Some(".wt") | Some(".wt-tree")));

    const CONFIG_FILES: &[&str] = &[
        ".editorconfig",
        ".gitignore",
        ".wtignore",
        "config.toml",
        ".env",
        ".env.local",
    ];

    if is_wt_path || CONFIG_FILES.contains(&filename) {
        return SemanticEvent::ConfigChange {
            tree_id,
            path: path.clone(),
        };
    }

    SemanticEvent::CodeChange {
        tree_id,
        paths: vec![path.clone()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::debounce::{DebouncedEvent, EventKind};

    fn make_event(path: &str) -> DebouncedEvent {
        DebouncedEvent::now(PathBuf::from(path), EventKind::Modified)
    }

    #[test]
    fn cargo_toml_is_dependency_change() {
        let e = make_event("myproject/Cargo.toml");
        assert!(matches!(
            classify_event(&e),
            SemanticEvent::DependencyChange { .. }
        ));
    }

    #[test]
    fn package_json_is_dependency_change() {
        let e = make_event("frontend/package.json");
        assert!(matches!(
            classify_event(&e),
            SemanticEvent::DependencyChange { .. }
        ));
    }

    #[test]
    fn rust_source_is_code_change() {
        let e = make_event("src/main.rs");
        assert!(matches!(
            classify_event(&e),
            SemanticEvent::CodeChange { .. }
        ));
    }

    #[test]
    fn wt_dir_path_is_config_change() {
        let e = make_event(".wt/config.toml");
        assert!(matches!(
            classify_event(&e),
            SemanticEvent::ConfigChange { .. }
        ));
    }

    #[test]
    fn editorconfig_is_config_change() {
        let e = make_event(".editorconfig");
        assert!(matches!(
            classify_event(&e),
            SemanticEvent::ConfigChange { .. }
        ));
    }

    #[test]
    fn code_change_paths_contains_the_file() {
        let e = make_event("src/lib.rs");
        match classify_event(&e) {
            SemanticEvent::CodeChange { paths, .. } => {
                assert_eq!(paths, vec![PathBuf::from("src/lib.rs")]);
            }
            other => panic!("expected CodeChange, got {:?}", other),
        }
    }
}
