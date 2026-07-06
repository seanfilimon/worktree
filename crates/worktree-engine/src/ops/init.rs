use crate::engine::WorktreeEngine;
use crate::error::Result;
use std::fs;
use std::path::Path;

/// Initialize a new worktree at the given path.
///
/// Creates the `.wt/` configuration directory per DotWt.md and the
/// content-addressable store (platform data directory) with a `root` tree
/// on `main`. Objects and refs never live inside `.wt/` — only
/// configuration, identity, hooks, reflog, and conflict metadata do.
pub fn initialize(root: &Path) -> Result<()> {
    let wt_dir = root.join(".wt");

    // Directory structure (DotWt.md §Initialisation).
    fs::create_dir_all(&wt_dir)?;
    fs::create_dir_all(wt_dir.join("identity").join("keys"))?;
    fs::create_dir_all(wt_dir.join("access"))?;
    fs::create_dir_all(wt_dir.join("hooks"))?;
    fs::create_dir_all(wt_dir.join("reflog"))?;
    fs::create_dir_all(wt_dir.join("conflicts"))?;
    fs::create_dir_all(wt_dir.join("cache"))?;

    // Derive project name from directory
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("worktree");

    // Write default config.toml
    let config = format!(
        r#"[worktree]
name = "{}"
visibility = "private"

[sync]
auto = true
interval_secs = 30

[auto_snapshot]
enabled = true
inactivity_timeout_secs = 30
max_changed_files = 50

[large_files]
threshold_bytes = 10485760
chunk_size_bytes = 4194304
lazy_loading = true

[reflog]
retention_days = 90
max_entries = 10000
sync_to_server = true
"#,
        name
    );
    fs::write(wt_dir.join("config.toml"), config)?;

    // Write default ignore
    let ignore = r#"# W0rkTree ignore patterns
# Hard ignores (cannot be overridden)
.wt/
.git/

# Default soft ignores
node_modules/
target/
__pycache__/
.DS_Store
*.pyc
*.pyo
.env
.venv/
dist/
build/
"#;
    fs::write(wt_dir.join("ignore"), ignore)?;

    // Default access control files (DeclarativeAccess.md). Roles mirror the
    // protocol's built-ins; policies start empty — the root ceiling is
    // "owner only" until policies grant more.
    let roles = r#"# W0rkTree role definitions (.wt/access/roles.toml)
# Built-in roles (reader, contributor, maintainer, admin) are always
# available; define custom roles here.

# [roles.release-manager]
# description = "Can create tags and releases"
# permissions = ["snapshot:read", "tag:create", "release:create"]
"#;
    fs::write(wt_dir.join("access").join("roles.toml"), roles)?;

    let policies = r#"# W0rkTree access policies (.wt/access/policies.toml)
# Declarative, Terraform-style. The server enforces these on every sync;
# local tooling reads them for display only.

# [[policy]]
# name = "example"
# effect = "allow"
# subjects = ["team:frontend"]
# scope = "tree:frontend"
# permissions = ["snapshot:create", "branch:push"]
"#;
    fs::write(wt_dir.join("access").join("policies.toml"), policies)?;

    // Initialize the content-addressable store (root tree, main branch).
    let engine = WorktreeEngine::open_unchecked(root);
    crate::persist::init_store(&engine, name)?;

    Ok(())
}
