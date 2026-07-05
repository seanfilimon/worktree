use crate::engine::WorktreeEngine;
use crate::error::Result;

pub fn list_ignored(engine: &WorktreeEngine) -> Result<Vec<String>> {
    let content = crate::ops::config::read_ignore(engine)?;
    let patterns: Vec<String> = content
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();
    Ok(patterns)
}
