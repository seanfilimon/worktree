//! Validation of user-supplied names (branches, tags, trees, remotes) and
//! normalization of user-supplied paths.

use crate::error::{Result, SdkError};

/// Validate a branch or tag name. Names may contain '/' separators (e.g.
/// `feature/login`) but every component must be non-empty, must not start
/// with '.' or '-', must not end with `.lock`, and the name must not contain
/// whitespace, control characters or any of `~ ^ : ? * [ \ "` `< > |` `@{`.
/// `HEAD` is reserved.
pub(crate) fn validate_ref_name(kind: &str, name: &str) -> Result<()> {
    let bad = |why: &str| {
        Err(SdkError::InvalidArgument(format!(
            "invalid {kind} name {name:?}: {why}"
        )))
    };
    if name.is_empty() {
        return bad("empty");
    }
    if name == "HEAD" {
        return bad("reserved");
    }
    if name.len() > 200 {
        return bad("too long");
    }
    if name.contains("@{") || name.contains("..") {
        return bad("contains '@{' or '..'");
    }
    if let Some(c) = name.chars().find(|c| {
        c.is_whitespace()
            || c.is_control()
            || matches!(
                c,
                '~' | '^' | ':' | '?' | '*' | '[' | '\\' | '"' | '<' | '>' | '|'
            )
    }) {
        return bad(&format!("contains {c:?}"));
    }
    for component in name.split('/') {
        if component.is_empty() {
            return bad("empty component");
        }
        if component.starts_with('.') || component.starts_with('-') {
            return bad("component starts with '.' or '-'");
        }
        if component.ends_with(".lock") || component.ends_with('.') {
            return bad("component ends with '.lock' or '.'");
        }
    }
    Ok(())
}

/// Validate a tree name: a single filesystem-safe path component.
pub(crate) fn validate_tree_name(name: &str) -> Result<()> {
    let bad = |why: &str| {
        Err(SdkError::InvalidArgument(format!(
            "invalid tree name {name:?}: {why}"
        )))
    };
    if name.is_empty() {
        return bad("empty");
    }
    if name.len() > 100 {
        return bad("too long");
    }
    if name.starts_with('.') || name.ends_with('.') || name.ends_with(' ') {
        return bad("must not start with '.' or end with '.' or ' '");
    }
    if let Some(c) = name.chars().find(|c| {
        c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
    }) {
        return bad(&format!("contains {c:?}"));
    }
    Ok(())
}

/// Validate a remote name: ASCII letters, digits, `-` and `_`.
pub(crate) fn validate_remote_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(SdkError::InvalidArgument(format!(
            "invalid remote name {name:?}: use letters, digits, '-' or '_'"
        )));
    }
    Ok(())
}

/// Normalize a user-supplied relative path to '/'-separated form without
/// leading `./`, trailing '/' or empty components. `""` means "everything".
pub(crate) fn normalize_rel_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect::<Vec<_>>()
        .join("/")
}

/// Whether `path` equals `prefix` or lies below it ('/'-separated).
/// An empty prefix matches everything.
pub(crate) fn path_has_prefix(path: &str, prefix: &str) -> bool {
    prefix.is_empty()
        || path == prefix
        || (path.len() > prefix.len()
            && path.starts_with(prefix)
            && path.as_bytes()[prefix.len()] == b'/')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ref_names() {
        for ok in ["main", "feature/login", "v1.0", "release-2", "a_b", "x/y/z"] {
            assert!(validate_ref_name("branch", ok).is_ok(), "{ok}");
        }
        for bad in [
            "", "HEAD", "a b", "a~1", "a^", "a:b", "a..b", "/a", "a/", "a//b", ".hidden", "-x",
            "x.lock", "a@{1}", "a\\b", "x.",
        ] {
            assert!(validate_ref_name("branch", bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn tree_names() {
        for ok in ["root", "my project", "web-app", "api_v2"] {
            assert!(validate_tree_name(ok).is_ok(), "{ok}");
        }
        for bad in ["", ".", "..", ".x", "a/b", "a\\b", "a:b", "x.", "x "] {
            assert!(validate_tree_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn remote_names() {
        assert!(validate_remote_name("origin").is_ok());
        assert!(validate_remote_name("up-stream_2").is_ok());
        assert!(validate_remote_name("").is_err());
        assert!(validate_remote_name("a.b").is_err());
        assert!(validate_remote_name("a/b").is_err());
    }

    #[test]
    fn normalize_and_prefix() {
        assert_eq!(normalize_rel_path("./a\\b//c/"), "a/b/c");
        assert_eq!(normalize_rel_path("."), "");
        assert!(path_has_prefix("a/b", ""));
        assert!(path_has_prefix("a/b", "a"));
        assert!(path_has_prefix("a/b", "a/b"));
        assert!(!path_has_prefix("ab/c", "a"));
        assert!(!path_has_prefix("a", "a/b"));
    }
}
