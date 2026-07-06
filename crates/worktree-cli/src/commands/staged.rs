use super::StagedAction;
use crate::output::format;
use worktree_sdk::Client;

pub async fn execute(action: Option<StagedAction>) -> Result<(), Box<dyn std::error::Error>> {
    let action = action.unwrap_or(StagedAction::List);
    let client = Client::open_current()?;

    match action {
        StagedAction::List => {
            let state = client.state()?;

            format::print_header("Staged Snapshots (Team Activity)");
            println!();

            let current_author = std::env::var("WT_AUTHOR")
                .or_else(|_| std::env::var("USER"))
                .or_else(|_| std::env::var("USERNAME"))
                .unwrap_or_else(|_| "unknown".to_string());

            let mut found_others = false;
            for tree in &state.trees {
                for snap in &tree.snapshots {
                    if snap.author != current_author {
                        found_others = true;
                        let short_id: String = snap.id.chars().take(8).collect();
                        format::print_list_item(&format!(
                            "{} on {}/{} — \"{}\" ({} file(s))",
                            snap.author,
                            tree.name,
                            snap.branch_name,
                            snap.message,
                            snap.files.len(),
                        ));
                        format::print_kv("      Snapshot", &short_id);
                        format::print_kv("      Time", &snap.timestamp);
                    }
                }
            }

            if !found_others {
                format::print_info("No staged snapshots from other team members.");
                format::print_info(
                    "As your team creates snapshots, their activity will appear here.",
                );
            }
        }
        StagedAction::Clear => {
            let staged_dir = client.wt_dir().join("cache").join("staged");
            let staged_state = client.wt_dir().join("cache").join("staged_index.json");

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
