//! Author identity resolution.
//!
//! Placeholder until tenant identity (`.wt/identity/tenant.toml`) is wired
//! in; resolves the snapshot author from the environment.

/// Resolve the author string for snapshots and tags.
///
/// Checks `WT_AUTHOR`, then the platform user environment variables.
pub fn author() -> String {
    std::env::var("WT_AUTHOR")
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Like [`author`], but returns `None` when no identity is configured.
pub fn author_opt() -> Option<String> {
    std::env::var("WT_AUTHOR")
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
}
