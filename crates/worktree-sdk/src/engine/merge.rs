use super::status::{load_state, save_state, FileEntry, SnapshotState};
use crate::error::{Result, SdkError};
use chrono::Utc;
use std::collections::HashMap;

pub struct MergeResult {
    pub snapshot: SnapshotState,
    pub files_merged: usize,
    pub conflicts: Vec<String>,
}

pub fn merge_branch(engine: &super::WorktreeEngine, source_branch: &str) -> Result<MergeResult> {
    let mut state = load_state(engine)?;
    let tree_name = state
        .current_tree
        .clone()
        .ok_or(SdkError::TreeNotFound("no current tree".into()))?;

    let tree = state
        .find_tree_mut(&tree_name)
        .ok_or(SdkError::TreeNotFound(tree_name.clone()))?;

    let target_branch = tree.current_branch.clone();
    if source_branch == target_branch {
        return Err(SdkError::MergeConflict(
            "cannot merge a branch into itself".into(),
        ));
    }

    // Get latest snapshots from both branches
    let source_snapshot = tree
        .snapshots
        .iter()
        .rfind(|s| s.branch_name == source_branch)
        .cloned();

    let target_snapshot = tree
        .snapshots
        .iter()
        .rfind(|s| s.branch_name == target_branch)
        .cloned();

    let (source_snap, target_snap) = match (source_snapshot, target_snapshot) {
        (Some(s), Some(t)) => (s, t),
        (None, _) => {
            return Err(SdkError::BranchNotFound(format!(
                "no snapshots on branch '{}'",
                source_branch
            )))
        }
        (_, None) => {
            return Err(SdkError::BranchNotFound(format!(
                "no snapshots on branch '{}'",
                target_branch
            )))
        }
    };

    // Find MRCA (Base Snapshot) for three-way merge
    let mut source_ancestors = std::collections::HashSet::new();
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(source_snap.id.clone());

    let snapshots_by_id: HashMap<String, &SnapshotState> =
        tree.snapshots.iter().map(|s| (s.id.clone(), s)).collect();

    while let Some(id) = queue.pop_front() {
        if source_ancestors.insert(id.clone()) {
            if let Some(snap) = snapshots_by_id.get(&id) {
                for p in &snap.parents {
                    queue.push_back(p.clone());
                }
            }
        }
    }

    struct QueueItem<'a> {
        snapshot: &'a SnapshotState,
    }

    impl<'a> PartialEq for QueueItem<'a> {
        fn eq(&self, other: &Self) -> bool {
            self.snapshot.id == other.snapshot.id
        }
    }

    impl<'a> Eq for QueueItem<'a> {}

    impl<'a> PartialOrd for QueueItem<'a> {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    impl<'a> Ord for QueueItem<'a> {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            self.snapshot.timestamp.cmp(&other.snapshot.timestamp)
        }
    }

    let mut base_files: Vec<FileEntry> = Vec::new();
    let mut pq = std::collections::BinaryHeap::new();
    if let Some(snap) = snapshots_by_id.get(&target_snap.id) {
        pq.push(QueueItem { snapshot: snap });
    }
    let mut visited_target = std::collections::HashSet::new();

    while let Some(item) = pq.pop() {
        let id = &item.snapshot.id;
        if visited_target.insert(id.clone()) {
            if source_ancestors.contains(id) {
                base_files = item.snapshot.files.clone();
                break;
            }
            for p in &item.snapshot.parents {
                if let Some(parent_snap) = snapshots_by_id.get(p) {
                    pq.push(QueueItem {
                        snapshot: parent_snap,
                    });
                }
            }
        }
    }

    let base_map: HashMap<String, FileEntry> = base_files
        .into_iter()
        .map(|f| (f.path.clone(), f))
        .collect();
    let source_map: HashMap<String, FileEntry> = source_snap
        .files
        .clone()
        .into_iter()
        .map(|f| (f.path.clone(), f))
        .collect();
    let target_map: HashMap<String, FileEntry> = target_snap
        .files
        .clone()
        .into_iter()
        .map(|f| (f.path.clone(), f))
        .collect();

    let mut all_paths = std::collections::HashSet::new();
    for p in base_map.keys() {
        all_paths.insert(p.clone());
    }
    for p in source_map.keys() {
        all_paths.insert(p.clone());
    }
    for p in target_map.keys() {
        all_paths.insert(p.clone());
    }

    let mut merged_files: HashMap<String, FileEntry> = HashMap::new();
    let mut conflicts = Vec::new();

    for path in all_paths {
        let base = base_map.get(&path);
        let source = source_map.get(&path);
        let target = target_map.get(&path);

        match (base, source, target) {
            // Unchanged in both
            (Some(b), Some(s), Some(t)) if b.hash == s.hash && b.hash == t.hash => {
                merged_files.insert(path.clone(), t.clone());
            }
            // Changed in source, unchanged in target
            (Some(b), Some(s), Some(t)) if b.hash != s.hash && b.hash == t.hash => {
                merged_files.insert(path.clone(), s.clone());
            }
            // Changed in target, unchanged in source
            (Some(b), Some(s), Some(t)) if b.hash == s.hash && b.hash != t.hash => {
                merged_files.insert(path.clone(), t.clone());
            }
            // Changed in both
            (Some(b), Some(s), Some(t)) if b.hash != s.hash && b.hash != t.hash => {
                if s.hash == t.hash {
                    merged_files.insert(path.clone(), t.clone());
                } else {
                    conflicts.push(path.clone());
                }
            }
            // Added in both
            (None, Some(s), Some(t)) => {
                if s.hash == t.hash {
                    merged_files.insert(path.clone(), t.clone());
                } else {
                    conflicts.push(path.clone());
                }
            }
            // Added in source only
            (None, Some(s), None) => {
                merged_files.insert(path.clone(), s.clone());
            }
            // Added in target only
            (None, None, Some(t)) => {
                merged_files.insert(path.clone(), t.clone());
            }
            // Deleted in source, unchanged in target
            (Some(b), None, Some(t)) if b.hash == t.hash => {}
            // Deleted in target, unchanged in source
            (Some(b), Some(s), None) if b.hash == s.hash => {}
            // Deleted in both
            (Some(_), None, None) => {}
            // Modify/Delete conflict
            (Some(b), None, Some(t)) if b.hash != t.hash => {
                conflicts.push(path.clone());
            }
            (Some(b), Some(s), None) if b.hash != s.hash => {
                conflicts.push(path.clone());
            }
            _ => {}
        }
    }

    conflicts.sort();

    let mut files: Vec<FileEntry> = merged_files.values().cloned().collect();
    files.sort_by(|a, b| a.path.cmp(&b.path));
    let files_merged = files.len();

    let source_tip = tree.find_branch(source_branch).and_then(|b| b.tip.clone());
    let target_tip = tree.find_branch(&target_branch).and_then(|b| b.tip.clone());

    let parents: Vec<String> = [target_tip, source_tip].into_iter().flatten().collect();
    let snapshot_id = uuid::Uuid::new_v4().to_string();

    let snapshot = SnapshotState {
        id: snapshot_id.clone(),
        message: format!("Merge branch '{}' into '{}'", source_branch, target_branch),
        author: std::env::var("WT_AUTHOR")
            .or_else(|_| std::env::var("USER"))
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string()),
        timestamp: Utc::now().to_rfc3339(),
        parents,
        tree_name: tree_name.clone(),
        branch_name: target_branch.clone(),
        files,
        auto_generated: false,
    };

    if conflicts.is_empty() {
        if let Some(branch) = tree.find_branch_mut(&target_branch) {
            branch.tip = Some(snapshot_id);
        }

        for f in &target_snap.files {
            if !merged_files.contains_key(&f.path) {
                let _ = std::fs::remove_file(engine.root().join(&f.path));
            }
        }

        for file in merged_files.values() {
            let file_path = engine.root().join(&file.path);
            if let Some(parent) = file_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if file.hash.len() >= 2 {
                let (prefix, rest) = file.hash.split_at(2);
                let obj_path = engine.objects_dir().join(prefix).join(rest);
                if let Ok(content) = std::fs::read(&obj_path) {
                    let _ = std::fs::write(&file_path, content);
                }
            }
        }

        tree.snapshots.push(snapshot.clone());
        save_state(engine, &state)?;
    }

    Ok(MergeResult {
        snapshot,
        files_merged,
        conflicts,
    })
}
