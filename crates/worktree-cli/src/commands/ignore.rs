use super::IgnoreAction;
use crate::output::format;
use worktree_sdk::Client;

pub async fn execute(action: IgnoreAction) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::open_current()?;
    match action {
        IgnoreAction::List => {
            let patterns = client.ignore_list()?;
            format::print_header("Ignore Patterns");
            if patterns.is_empty() {
                format::print_info("No ignore patterns configured.");
            } else {
                for pattern in &patterns {
                    format::print_list_item(pattern);
                }
                println!();
                format::print_info(&format!("{} pattern(s) active", patterns.len()));
            }
        }
        IgnoreAction::Add { pattern } => {
            let ignore_path = client.wt_dir().join("ignore");
            let mut content = std::fs::read_to_string(&ignore_path).unwrap_or_default();
            if !content.ends_with('\n') && !content.is_empty() {
                content.push('\n');
            }
            content.push_str(&pattern);
            content.push('\n');
            std::fs::write(&ignore_path, content)?;
            format::print_success(&format!("Added ignore pattern: {}", pattern));
        }
    }
    Ok(())
}
