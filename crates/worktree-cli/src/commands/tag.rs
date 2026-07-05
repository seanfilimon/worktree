use super::TagAction;
use crate::output::format;
use worktree_sdk::Client;

pub async fn execute(action: TagAction) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::open_current()?;
    match action {
        TagAction::Create { name, message } => {
            let tag = client.tag_create(&name, message.as_deref())?;
            format::print_success(&format!("Tag '{}' created", tag.name));
            format::print_kv(
                "Target",
                &tag.target_snapshot[..8.min(tag.target_snapshot.len())],
            );
            if let Some(msg) = &tag.message {
                format::print_kv("Message", msg);
            }
            if let Some(tagger) = &tag.tagger {
                format::print_kv("Tagger", tagger);
            }
            format::print_kv("Created", &tag.created_at);
        }
        TagAction::List => {
            let tags = client.tag_list()?;
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
            client.tag_delete(&name)?;
            format::print_success(&format!("Tag '{}' deleted", name));
        }
    }
    Ok(())
}
