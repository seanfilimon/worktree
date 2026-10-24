use std::path::Path;
use std::fs;
use crate::error::Result;

/// Initialize a new worktree at the given path.
/// Creates .wt/ directory structure and default configuration.
pub fn initialize(root: &Path) -> Result<()> {
    let wt_dir = root.join(".wt");

    // Create directory structure
    fs::create_dir_all(&wt_dir)?;
    fs::create_dir_all(wt_dir.join("objects"))?;
    fs::create_dir_all(wt_dir.join("refs").join("branches"))?;
    fs::create_dir_all(wt_dir.join("refs").join("tags"))?;
    fs::create_dir_all(wt_dir.join("reflog"))?;
    fs::create_dir_all(wt_dir.join("identity"))?;
    fs::create_dir_all(wt_dir.join("access"))?;
    fs::create_dir_all(wt_dir.join("hooks"))?;
    fs::create_dir_all(wt_dir.join("cache"))?;
    fs::create_dir_all(wt_dir.join("conflicts"))?;

    // Derive project name from directory
    let name = root.file_name()
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
"#, name);

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

    // Initialize state
    let state = crate::engine::status::WorktreeState::new(name);
    let state_json = serde_json::to_string_pretty(&state)
        .map_err(|e| crate::error::SdkError::Serialization(e.to_string()))?;
    fs::write(wt_dir.join("state.json"), state_json)?;

    Ok(())
}
