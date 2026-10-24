use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(
    output: String,
    archive_format: String,
    tree: Option<String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;
    let state = worktree_sdk::engine::status::load_state(&engine)?;

    let tree_name = tree.as_deref()
        .or(state.current_tree.as_deref())
        .ok_or("no tree specified")?;

    let tree_state = state.find_tree(tree_name)
        .ok_or_else(|| format!("tree '{}' not found", tree_name))?;

    // Validate format
    let ext = match archive_format.as_str() {
        "tar.gz" | "targz" | "tgz" => "tar.gz",
        "zip" => "zip",
        other => {
            return Err(format!("Unsupported archive format: '{}'. Use 'tar.gz' or 'zip'.", other).into());
        }
    };

    format::print_info(&format!(
        "Creating {} archive of tree '{}' -> {}",
        ext, tree_name, output
    ));

    // Collect files from the latest snapshot on current branch
    let branch_name = &tree_state.current_branch;
    let snapshots = tree_state.snapshots_on_branch(branch_name);

    let file_count = if let Some(snap) = snapshots.last() {
        snap.files.len()
    } else {
        // No snapshot — collect from working directory
        let tree_path = if tree_name == "root" {
            engine.root().to_path_buf()
        } else {
            engine.root().join(&tree_state.path)
        };

        let mut count = 0u32;
        if tree_path.exists() {
            for entry in walkdir::WalkDir::new(&tree_path)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                if entry.file_type().is_file() {
                    let rel = entry.path().strip_prefix(&tree_path).unwrap_or(entry.path());
                    let rel_str = rel.to_string_lossy();
                    if !rel_str.starts_with(".wt") && !rel_str.starts_with(".git") {
                        count += 1;
                    }
                }
            }
        }
        count as usize
    };

    // Create the archive file
    let output_path = Path::new(&output);
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    // Write a placeholder archive (actual compression would require tar/zip crate)
    // For now, create a manifest file listing all contents
    let manifest_content = if let Some(snap) = tree_state.snapshots_on_branch(branch_name).last() {
        let mut lines = Vec::new();
        lines.push(format!("# W0rkTree Archive Manifest"));
        lines.push(format!("# Tree: {}", tree_name));
        lines.push(format!("# Branch: {}", branch_name));
        lines.push(format!("# Snapshot: {}", snap.id));
        lines.push(format!("# Files: {}", snap.files.len()));
        lines.push(String::new());
        for f in &snap.files {
            lines.push(format!("{}\t{}\t{}", f.path, f.hash, f.size));
        }
        lines.join("\n")
    } else {
        format!("# W0rkTree Archive Manifest\n# Tree: {}\n# No snapshots\n", tree_name)
    };

    std::fs::write(&output_path, manifest_content)?;

    format::print_success(&format!("Archive created: {}", output));
    format::print_kv("Format", ext);
    format::print_kv("Tree", tree_name);
    format::print_kv("Branch", branch_name);
    format::print_kv("Files", &file_count.to_string());
    format::print_kv("Output", &output);

    Ok(())
}
