use super::TagAction;
use crate::output::format;
use std::path::Path;
use worktree_sdk::WorktreeEngine;

pub async fn execute(action: TagAction) -> Result<(), Box<dyn std::error::Error>> {
    match action {
        TagAction::Create { name, message } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let tag = worktree_sdk::engine::tag::create_tag(
                &engine,
                &name,
                message.as_deref(),
                None,
            )?;
            format::print_success(&format!("Tag '{}' created", tag.name));
            format::print_kv("Target", &tag.target_snapshot[..8.min(tag.target_snapshot.len())]);
            if let Some(msg) = &tag.message {
                format::print_kv("Message", msg);
            }
            if let Some(tagger) = &tag.tagger {
                format::print_kv("Tagger", tagger);
            }
            format::print_kv("Created", &tag.created_at);
        }
        TagAction::List => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            let tags = worktree_sdk::engine::tag::list_tags(&engine, None)?;
            format::print_header("Tags");
            if tags.is_empty() {
                format::print_info("No tags yet. Create one with `wt tag create <name>`");
            } else {
                for tag in &tags {
                    let short_target: String = tag.target_snapshot.chars().take(8).collect();
                    let msg = tag.message.as_deref().unwrap_or("");
                    if msg.is_empty() {
                        format::print_list_item(&format!("{} -> {}", tag.name, short_target));
                    } else {
                        format::print_list_item(&format!(
                            "{} -> {} ({})",
                            tag.name, short_target, msg
                        ));
                    }
                }
                println!();
                format::print_info(&format!("{} tag(s) total", tags.len()));
            }
        }
        TagAction::Delete { name } => {
            let engine = WorktreeEngine::open(Path::new("."))?;
            worktree_sdk::engine::tag::delete_tag(&engine, &name, None)?;
            format::print_success(&format!("Tag '{}' deleted", name));
        }
    }
    Ok(())
}
