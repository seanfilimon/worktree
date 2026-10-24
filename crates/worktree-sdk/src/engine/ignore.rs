use crate::error::Result;

pub fn list_ignored(engine: &super::WorktreeEngine) -> Result<Vec<String>> {
    let content = super::config::read_ignore(engine)?;
    let patterns: Vec<String> = content.lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .map(|l| l.to_string())
        .collect();
    Ok(patterns)
}
