use super::BranchAction;
use crate::output::format;
use colored::Colorize;
use worktree_sdk::Client;

pub async fn execute(action: BranchAction) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::open_current()?;
    match action {
        BranchAction::Create { name } => {
            let branch = client.branch_create(&name)?;
            format::print_success(&format!("Created branch '{}'", branch.name));
            if let Some(tip) = &branch.tip {
                format::print_kv("Base", &tip[..8.min(tip.len())]);
            }
        }
        BranchAction::List => {
            let (branches, current) = client.branch_list()?;
            format::print_header("Branches");
            for b in &branches {
                let marker = if b.name == current { "* " } else { "  " };
                let tip_display = b
                    .tip
                    .as_ref()
                    .map(|t| t[..8.min(t.len())].to_string())
                    .unwrap_or_else(|| "(no snapshots)".to_string());
                if b.name == current {
                    println!("{}{} ({})", marker, b.name.green().bold(), tip_display);
                } else {
                    println!("{}{} ({})", marker, b.name, tip_display);
                }
            }
            format::print_info(&format!("{} branch(es) total", branches.len()));
        }
        BranchAction::Switch { name } => {
            client.branch_switch(&name)?;
            format::print_success(&format!("Switched to branch '{}'", name));
        }
        BranchAction::Delete { name } => {
            client.branch_delete(&name)?;
            format::print_success(&format!("Deleted branch '{}'", name));
        }
    }
    Ok(())
}
