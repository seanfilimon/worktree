use std::path::Path;

use crate::watcher::debounce::DebouncedEvent;

/// A higher-level, semantically classified event derived from raw
/// file-system change notifications. The engine uses these to decide when
/// and how to create snapshots, branches, or other automated actions.
#[derive(Debug, Clone)]
pub enum SemanticEvent {
    /// A source or content file changed within a tree.
    CodeChange {
        /// The tree that owns the changed file.
        tree_name: String,
        /// The path that changed (worktree-relative).
        path: String,
    },

    /// A dependency manifest (e.g. `Cargo.toml`, `package.json`) changed.
    DependencyChange { tree_name: String, path: String },

    /// A configuration file (e.g. `.wt-tree/config.toml`, `.editorconfig`)
    /// changed.
    ConfigChange { tree_name: String, path: String },
}

impl SemanticEvent {
    pub fn path(&self) -> &str {
        match self {
            SemanticEvent::CodeChange { path, .. }
            | SemanticEvent::DependencyChange { path, .. }
            | SemanticEvent::ConfigChange { path, .. } => path,
        }
    }
}

/// Well-known dependency manifest file names.
const DEPENDENCY_FILES: &[&str] = &[
    "Cargo.toml",
    "Cargo.lock",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "go.mod",
    "go.sum",
    "requirements.txt",
    "pyproject.toml",
    "Pipfile",
    "Gemfile",
    "pom.xml",
    "build.gradle",
];

/// Well-known configuration file names.
const CONFIG_FILES: &[&str] = &[
    ".editorconfig",
    ".gitattributes",
    ".rustfmt.toml",
    "rustfmt.toml",
    "clippy.toml",
    "tsconfig.json",
    ".eslintrc",
    ".prettierrc",
];

/// Classify a raw debounced file-system event into a [`SemanticEvent`].
///
/// `tree_name` is resolved by the caller (the daemon knows the worktree's
/// tree layout); the classifier only inspects the path itself.
pub fn classify_event(raw: &DebouncedEvent, tree_name: &str) -> SemanticEvent {
    let path_str = raw.path.to_string_lossy().replace('\\', "/");
    let file_name = raw
        .path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();

    if DEPENDENCY_FILES.contains(&file_name.as_str()) {
        return SemanticEvent::DependencyChange {
            tree_name: tree_name.to_string(),
            path: path_str,
        };
    }

    let is_config = CONFIG_FILES.contains(&file_name.as_str())
        || path_str.contains(".wt-tree/")
        || (file_name.starts_with('.') && has_config_extension(&raw.path));
    if is_config {
        return SemanticEvent::ConfigChange {
            tree_name: tree_name.to_string(),
            path: path_str,
        };
    }

    SemanticEvent::CodeChange {
        tree_name: tree_name.to_string(),
        path: path_str,
    }
}

fn has_config_extension(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|e| e.to_str()),
        Some("toml" | "json" | "yaml" | "yml" | "ini" | "conf")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::watcher::debounce::EventKind;
    use std::path::PathBuf;

    fn raw(path: &str) -> DebouncedEvent {
        DebouncedEvent::now(PathBuf::from(path), EventKind::Modified)
    }

    #[test]
    fn classifies_dependency_manifests() {
        let event = classify_event(&raw("backend/Cargo.toml"), "backend");
        assert!(matches!(event, SemanticEvent::DependencyChange { .. }));
        let event = classify_event(&raw("web/package.json"), "web");
        assert!(matches!(event, SemanticEvent::DependencyChange { .. }));
    }

    #[test]
    fn classifies_config_files() {
        let event = classify_event(&raw("backend/.wt-tree/config.toml"), "backend");
        assert!(matches!(event, SemanticEvent::ConfigChange { .. }));
        let event = classify_event(&raw(".editorconfig"), "root");
        assert!(matches!(event, SemanticEvent::ConfigChange { .. }));
    }

    #[test]
    fn everything_else_is_code() {
        let event = classify_event(&raw("src/main.rs"), "root");
        assert!(matches!(event, SemanticEvent::CodeChange { .. }));
        let event = classify_event(&raw("docs/readme.md"), "root");
        assert!(matches!(event, SemanticEvent::CodeChange { .. }));
    }
}
