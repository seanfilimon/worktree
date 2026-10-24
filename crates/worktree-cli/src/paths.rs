//! Converting user-supplied paths (relative to the current directory) into
//! the '/'-separated, root-relative paths the SDK works with.

use std::path::{Component, Path, PathBuf};

use crate::error::{CliError, Result};

/// Express `input` (absolute, or relative to `cwd`) relative to `base`, with
/// '/' separators. Returns "" for `base` itself. Works for paths that do not
/// exist (e.g. deleted files) and tolerates differing spellings of the same
/// directory (symlinks, `\\?\` prefixes, case on Windows).
pub fn relative_to(base: &Path, cwd: &Path, input: &Path) -> Result<String> {
    let target = normalize(&cwd.join(input));
    let base_norm = normalize(base);
    if let Ok(rel) = target.strip_prefix(&base_norm) {
        return Ok(to_slash(rel));
    }
    if let Ok(canon_base) = base_norm.canonicalize() {
        if let Ok(rel) = canonicalize_lenient(&target).strip_prefix(&canon_base) {
            return Ok(to_slash(rel));
        }
    }
    Err(CliError::failure(format!(
        "path '{}' is outside '{}'",
        input.display(),
        base.display()
    )))
}

/// Lexically resolve `.` and `..` components.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    out.push(component);
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Canonicalize the longest existing ancestor of `path` and re-append the
/// remaining (non-existent) components.
fn canonicalize_lenient(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(canon) = existing.canonicalize() {
            return rest.iter().rev().fold(canon, |acc, part| acc.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn to_slash(rel: &Path) -> String {
    rel.components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_dots() {
        let base = Path::new("root");
        assert_eq!(
            normalize(&base.join("a").join("..").join("b").join(".")),
            base.join("b")
        );
    }

    #[test]
    fn relative_inside_base() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        let sub = base.join("src");
        std::fs::create_dir_all(&sub).expect("mkdir");
        assert_eq!(
            relative_to(base, &sub, Path::new("main.rs")).expect("rel"),
            "src/main.rs"
        );
        assert_eq!(
            relative_to(base, &sub, Path::new("../README")).expect("rel"),
            "README"
        );
        assert_eq!(relative_to(base, base, Path::new(".")).expect("rel"), "");
        assert_eq!(
            relative_to(base, base, Path::new("missing/deep/file.txt")).expect("rel"),
            "missing/deep/file.txt"
        );
    }

    #[test]
    fn absolute_inputs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path();
        let abs = base.join("a").join("b.txt");
        assert_eq!(
            relative_to(base, Path::new("/elsewhere"), &abs).expect("rel"),
            "a/b.txt"
        );
    }

    #[test]
    fn canonical_spelling_of_base() {
        let dir = tempfile::tempdir().expect("tempdir");
        let canon = dir.path().canonicalize().expect("canon");
        std::fs::create_dir_all(canon.join("x")).expect("mkdir");
        // Base given in its plain spelling, input in canonical form (or vice versa).
        assert_eq!(
            relative_to(dir.path(), &canon, Path::new("x/y")).expect("rel"),
            "x/y"
        );
        assert_eq!(
            relative_to(&canon, dir.path(), Path::new("x")).expect("rel"),
            "x"
        );
    }

    #[test]
    fn outside_base_is_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("tree");
        std::fs::create_dir_all(&base).expect("mkdir");
        let err = relative_to(&base, dir.path(), Path::new("other.txt")).expect_err("outside");
        assert!(err.to_string().contains("outside"), "{err}");
    }
}
