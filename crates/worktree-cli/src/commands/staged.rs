use super::StagedAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

fn print_staged(clear: bool) -> Result<(), Box<dyn std::error::Error>> {
    let engine = WorktreeEngine::open(Path::new("."))?;

    if clear {
        // Clear terminal screen for interactive watch
        print!("\x1B[2J\x1B[1;1H");
    }

    format::print_header("Staged Snapshots (Team Activity)");
    println!();

    let current_author = std::env::var("WT_AUTHOR")
        .or_else(|_| std::env::var("USER"))
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string());

    let mut found_others = false;

    let staged_file = engine.wt_dir().join("cache").join("staged_index.json");
    if let Ok(content) = std::fs::read_to_string(&staged_file) {
        if let Ok(snapshots) = serde_json::from_str::<Vec<serde_json::Value>>(&content) {
            for snap in snapshots {
                let author = snap
                    .get("author")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        snap.get("snapshot")
                            .and_then(|s| s.get("author"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or("unknown");

                if author != current_author {
                    found_others = true;

                    let id = snap
                        .get("snapshot_id")
                        .and_then(|v| v.as_str())
                        .or_else(|| snap.get("id").and_then(|v| v.as_str()))
                        .or_else(|| {
                            snap.get("snapshot")
                                .and_then(|s| s.get("id"))
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("unknown");

                    let tree_name = snap
                        .get("tree_id")
                        .and_then(|v| v.as_str())
                        .or_else(|| snap.get("worktree").and_then(|v| v.as_str()))
                        .or_else(|| snap.get("tree_name").and_then(|v| v.as_str()))
                        .unwrap_or("unknown");

                    let branch_name = snap
                        .get("branch")
                        .and_then(|v| v.as_str())
                        .or_else(|| snap.get("branch_name").and_then(|v| v.as_str()))
                        .unwrap_or("unknown");

                    let message = snap
                        .get("message")
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            snap.get("snapshot")
                                .and_then(|s| s.get("message"))
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("No message");

                    let timestamp = snap
                        .get("created_at")
                        .or_else(|| snap.get("timestamp"))
                        .and_then(|v| v.as_str())
                        .or_else(|| {
                            snap.get("snapshot")
                                .and_then(|s| s.get("created_at").or_else(|| s.get("timestamp")))
                                .and_then(|v| v.as_str())
                        })
                        .unwrap_or("unknown time");

                    let files_len = snap
                        .get("objects")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .or_else(|| {
                            snap.get("files")
                                .and_then(|v| v.as_array())
                                .map(|a| a.len())
                        })
                        .or_else(|| {
                            snap.get("snapshot")
                                .and_then(|s| s.get("files"))
                                .and_then(|v| v.as_array())
                                .map(|a| a.len())
                        })
                        .unwrap_or(0);

                    let short_id: String = id.chars().take(8).collect();
                    format::print_list_item(&format!(
                        "{} on {}/{} — \"{}\" ({} file(s))",
                        author, tree_name, branch_name, message, files_len,
                    ));
                    format::print_kv("      Snapshot", &short_id);
                    format::print_kv("      Time", timestamp);
                }
            }
        }
    }

    if !found_others {
        format::print_info("No staged snapshots from other team members.");
        format::print_info("As your team creates snapshots, their activity will appear here.");
    }
    Ok(())
}

pub async fn execute(
    action: Option<StagedAction>,
    watch: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let action = action.unwrap_or(StagedAction::List);

    match action {
        StagedAction::List => {
            if watch {
                let engine = WorktreeEngine::open(Path::new("."))?;
                let state_file = engine.wt_dir().join("state.json");
                let staged_file = engine.wt_dir().join("cache").join("staged_index.json");

                let _ = print_staged(true);

                let mut last_mod = std::time::SystemTime::UNIX_EPOCH;
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                    let mut current_mod = last_mod;

                    if let Ok(meta) = std::fs::metadata(&state_file) {
                        if let Ok(m) = meta.modified() {
                            if m > current_mod {
                                current_mod = m;
                            }
                        }
                    }
                    if let Ok(meta) = std::fs::metadata(&staged_file) {
                        if let Ok(m) = meta.modified() {
                            if m > current_mod {
                                current_mod = m;
                            }
                        }
                    }

                    if current_mod > last_mod {
                        last_mod = current_mod;
                        let _ = print_staged(true);
                    }
                }
            } else {
                print_staged(false)?;
            }
        }
        StagedAction::Clear => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let staged_dir = engine.wt_dir().join("cache").join("staged");
            let staged_state = engine.wt_dir().join("cache").join("staged_index.json");

            let mut cleared = false;
            if staged_dir.exists() {
                std::fs::remove_dir_all(&staged_dir)?;
                cleared = true;
            }
            if staged_state.exists() {
                std::fs::remove_file(&staged_state)?;
                cleared = true;
            }

            if cleared {
                format::print_success("Staged snapshot cache cleared.");
            } else {
                format::print_info("No staged snapshots to clear.");
            }
        }
    }
    Ok(())
}
