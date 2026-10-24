//! Path normalization for worktree roots.
//!
//! Roots are compared, persisted and handed to the file watcher in one
//! canonical form: absolute, symlinks resolved, and on Windows without the
//! `\\?\` verbatim prefix that `canonicalize` adds (watchers report paths
//! with the same prefix as the watched path, and users never type it).

use std::path::{Path, PathBuf};

/// Canonicalize an existing path into the daemon's root form.
pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    Ok(strip_verbatim(&path.canonicalize()?))
}

/// Best-effort normalization for looking up a root that may no longer
/// exist: canonical if possible, else lexically absolute.
pub fn lookup_key(path: &Path) -> PathBuf {
    canonical(path)
        .or_else(|_| std::path::absolute(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// Remove a Windows verbatim prefix (`\\?\C:\x` → `C:\x`,
/// `\\?\UNC\srv\share` → `\\srv\share`). No-op elsewhere.
pub fn strip_verbatim(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        if let Some(s) = path.to_str() {
            if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
                return PathBuf::from(format!(r"\\{rest}"));
            }
            if let Some(rest) = s.strip_prefix(r"\\?\") {
                let bytes = rest.as_bytes();
                if bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':' {
                    return PathBuf::from(rest);
                }
            }
        }
    }
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_is_absolute_and_resolves_dots() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("sub")).unwrap();
        let via_dots = dir.path().join("sub").join("..").join("sub");
        let c = canonical(&via_dots).unwrap();
        assert!(c.is_absolute());
        assert_eq!(c, canonical(&dir.path().join("sub")).unwrap());
        assert!(!c.to_string_lossy().starts_with(r"\\?\"));
    }

    #[test]
    fn lookup_key_of_missing_path_is_absolute() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("gone");
        let key = lookup_key(&missing);
        assert!(key.is_absolute());
        assert!(key.ends_with("gone"));
    }

    #[test]
    fn lookup_key_matches_canonical_for_existing_paths() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(lookup_key(dir.path()), canonical(dir.path()).unwrap());
    }

    #[cfg(windows)]
    #[test]
    fn strips_windows_verbatim_prefixes() {
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\C:\work\x")),
            PathBuf::from(r"C:\work\x")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\UNC\srv\share\x")),
            PathBuf::from(r"\\srv\share\x")
        );
        // Verbatim paths without a drive letter must keep their prefix.
        assert_eq!(
            strip_verbatim(Path::new(r"\\?\Volume{abc}\x")),
            PathBuf::from(r"\\?\Volume{abc}\x")
        );
        assert_eq!(
            strip_verbatim(Path::new(r"C:\plain")),
            PathBuf::from(r"C:\plain")
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn strip_is_noop_off_windows() {
        assert_eq!(strip_verbatim(Path::new("/a/b")), PathBuf::from("/a/b"));
    }
}
