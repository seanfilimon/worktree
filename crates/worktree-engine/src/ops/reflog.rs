use crate::engine::WorktreeEngine;
use crate::error::{EngineError, Result};
use crate::persist::{load_state, read_reflog};

/// Show the real reflog (from `.wt/reflog/<branch>.log`) for the current
/// branch, newest first, formatted for display.
pub fn show_reflog(engine: &WorktreeEngine, count: usize) -> Result<Vec<String>> {
    let state = load_state(engine)?;
    let tree = state
        .current_tree()
        .ok_or(EngineError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;

    let lines = read_reflog(engine, branch, count)?;
    let mut entries = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        // <timestamp>\t<operation>\t<before>\t<after>\t<user>\t<message>
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() == 6 {
            let after_short: String = fields[3].chars().take(8).collect();
            entries.push(format!(
                "{}@{{{}}}: {} — {} ({}) [{}]",
                branch, i, fields[1], after_short, fields[5], fields[0]
            ));
        } else {
            entries.push(line.clone());
        }
    }
    Ok(entries)
}
