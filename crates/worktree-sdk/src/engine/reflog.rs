use crate::error::Result;

pub fn show_reflog(engine: &super::WorktreeEngine, count: usize) -> Result<Vec<String>> {
    let state = super::status::load_state(engine)?;
    let tree = state.current_tree()
        .ok_or(crate::error::SdkError::TreeNotFound("no current tree".into()))?;
    let branch = &tree.current_branch;

    let mut entries = Vec::new();
    let snapshots = tree.snapshots_on_branch(branch);
    for (i, snap) in snapshots.iter().rev().enumerate().take(count) {
        entries.push(format!(
            "{}@{{{}}}: {} — {} ({})",
            branch, i, snap.id.chars().take(8).collect::<String>(),
            snap.message, snap.timestamp
        ));
    }

    Ok(entries)
}
