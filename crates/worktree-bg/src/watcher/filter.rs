//! Classification of raw file-system event paths.

use std::path::{Component, Path};

use notify::event::{AccessKind, AccessMode, MetadataKind, ModifyKind};
use notify::EventKind;

/// Name of the repository metadata directory at a worktree root.
pub const WT_DIR: &str = ".wt";
/// Marker directory of a nested tree.
pub const TREE_MARKER_DIR: &str = ".wt-tree";
/// Per-directory ignore file.
pub const IGNORE_FILE: &str = ".wtignore";

/// What an event path means to the daemon.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathClass {
    /// Repository metadata (`.wt/…`, `.wt-tree/…`); never a working change.
    /// `reload` is set when the path affects config or ignore rules.
    Internal { reload: bool },
    /// A working-directory change at `rel` ('/'-separated, relative to the
    /// worktree root). `reload` is set for `.wtignore` files.
    Change { rel: String, reload: bool },
}

/// Classify `path` (as reported by the watcher) relative to `root`.
/// `None` for paths outside the worktree and for the root itself.
pub fn classify(root: &Path, path: &Path) -> Option<PathClass> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_string_lossy()),
            // Watchers report normalized paths; anything else is not ours.
            _ => return None,
        }
    }
    let first = parts.first()?;
    if first == WT_DIR {
        let reload = matches!(parts.as_slice(), [_, f] if f == "config.toml" || f == "ignore");
        return Some(PathClass::Internal { reload });
    }
    if let Some(i) = parts.iter().position(|p| p == TREE_MARKER_DIR) {
        // hardening (§15.10): a tree's shared `.wt-tree/` files (config,
        // ignore, hooks, access) are tracked content as well as config.
        let tail = parts[i..].join("/");
        if worktree_sdk::model::is_shared_tree_config_path(&tail) {
            return Some(PathClass::Change {
                rel: parts.join("/"),
                reload: true,
            });
        }
        return Some(PathClass::Internal { reload: true });
    }
    let reload = parts.last().is_some_and(|p| p == IGNORE_FILE);
    Some(PathClass::Change {
        rel: parts.join("/"),
        reload,
    })
}

/// Whether an event kind can represent a content or structure change.
/// Pure reads and access-time updates are not changes.
pub fn is_change(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)) => false,
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use notify::event::{CreateKind, DataChange, RemoveKind, RenameMode};

    fn root() -> PathBuf {
        std::env::temp_dir().join("wt-filter-root")
    }

    fn change(rel: &str) -> Option<PathClass> {
        Some(PathClass::Change {
            rel: rel.into(),
            reload: false,
        })
    }

    #[test]
    fn working_files_are_changes_with_forward_slashes() {
        let r = root();
        assert_eq!(classify(&r, &r.join("a.txt")), change("a.txt"));
        assert_eq!(
            classify(&r, &r.join("src").join("lib.rs")),
            change("src/lib.rs")
        );
    }

    #[test]
    fn wt_directory_is_internal() {
        let r = root();
        assert_eq!(
            classify(&r, &r.join(".wt")),
            Some(PathClass::Internal { reload: false })
        );
        let obj = r.join(".wt").join("objects").join("ab").join("cdef");
        assert_eq!(
            classify(&r, &obj),
            Some(PathClass::Internal { reload: false })
        );
        let index = r.join(".wt").join("trees").join("main").join("index.json");
        assert_eq!(
            classify(&r, &index),
            Some(PathClass::Internal { reload: false })
        );
    }

    #[test]
    fn config_and_ignore_files_request_reload() {
        let r = root();
        let cfg = r.join(".wt").join("config.toml");
        assert_eq!(
            classify(&r, &cfg),
            Some(PathClass::Internal { reload: true })
        );
        let ign = r.join(".wt").join("ignore");
        assert_eq!(
            classify(&r, &ign),
            Some(PathClass::Internal { reload: true })
        );
        // Only the top-level files count.
        let nested = r.join(".wt").join("trees").join("config.toml");
        assert_eq!(
            classify(&r, &nested),
            Some(PathClass::Internal { reload: false })
        );
    }

    #[test]
    fn wtignore_is_a_change_that_requests_reload() {
        let r = root();
        assert_eq!(
            classify(&r, &r.join("sub").join(".wtignore")),
            Some(PathClass::Change {
                rel: "sub/.wtignore".into(),
                reload: true
            })
        );
    }

    #[test]
    fn nested_tree_markers_are_internal() {
        let r = root();
        let marker = r
            .join("libs")
            .join("core")
            .join(".wt-tree")
            .join("local.toml");
        assert_eq!(
            classify(&r, &marker),
            Some(PathClass::Internal { reload: true })
        );
    }

    #[test]
    fn shared_tree_files_are_changes_that_reload() {
        // hardening (§15.10): shared `.wt-tree/` files are tracked.
        let r = root();
        let config = r
            .join("libs")
            .join("core")
            .join(".wt-tree")
            .join("config.toml");
        assert_eq!(
            classify(&r, &config),
            Some(PathClass::Change {
                rel: "libs/core/.wt-tree/config.toml".into(),
                reload: true
            })
        );
        let hook = r.join(".wt-tree").join("hooks").join("pre-snapshot");
        assert_eq!(
            classify(&r, &hook),
            Some(PathClass::Change {
                rel: ".wt-tree/hooks/pre-snapshot".into(),
                reload: true
            })
        );
    }

    #[test]
    fn similar_names_are_not_internal() {
        let r = root();
        assert_eq!(classify(&r, &r.join(".wtx").join("f")), change(".wtx/f"));
        assert_eq!(
            classify(&r, &r.join("docs").join(".wt")),
            change("docs/.wt")
        );
    }

    #[test]
    fn outside_paths_and_root_are_ignored() {
        let r = root();
        assert_eq!(classify(&r, &r), None);
        assert_eq!(
            classify(&r, &std::env::temp_dir().join("elsewhere.txt")),
            None
        );
    }

    #[test]
    fn reads_are_not_changes() {
        assert!(!is_change(&EventKind::Access(AccessKind::Read)));
        assert!(!is_change(&EventKind::Access(AccessKind::Open(
            AccessMode::Any
        ))));
        assert!(!is_change(&EventKind::Access(AccessKind::Close(
            AccessMode::Read
        ))));
        assert!(!is_change(&EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::AccessTime
        ))));
    }

    #[test]
    fn writes_are_changes() {
        assert!(is_change(&EventKind::Create(CreateKind::File)));
        assert!(is_change(&EventKind::Modify(ModifyKind::Data(
            DataChange::Content
        ))));
        assert!(is_change(&EventKind::Modify(ModifyKind::Name(
            RenameMode::Both
        ))));
        assert!(is_change(&EventKind::Modify(ModifyKind::Metadata(
            MetadataKind::Permissions
        ))));
        assert!(is_change(&EventKind::Remove(RemoveKind::File)));
        assert!(is_change(&EventKind::Access(AccessKind::Close(
            AccessMode::Write
        ))));
        assert!(is_change(&EventKind::Any));
    }
}
