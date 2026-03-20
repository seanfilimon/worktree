use std::fs;
use crate::error::{SdkError, Result};

/// Read the root worktree config
pub fn read_config(engine: &super::WorktreeEngine) -> Result<String> {
    let config_path = engine.wt_dir().join("config.toml");
    Ok(fs::read_to_string(&config_path)?)
}

/// Read the ignore patterns
pub fn read_ignore(engine: &super::WorktreeEngine) -> Result<String> {
    let ignore_path = engine.wt_dir().join("ignore");
    if ignore_path.exists() {
        Ok(fs::read_to_string(&ignore_path)?)
    } else {
        Ok(String::new())
    }
}
