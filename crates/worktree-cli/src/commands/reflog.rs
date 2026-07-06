use crate::output::format;
use worktree_sdk::Client;

pub async fn execute(count: usize) -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::open_current()?;
    let entries = client.reflog(count)?;

    format::print_header("Reflog");

    if entries.is_empty() {
        println!();
        format::print_info("No reflog entries yet.");
        return Ok(());
    }

    println!();
    for entry in &entries {
        format::print_list_item(entry);
    }

    println!();
    format::print_info(&format!("Showing {} reflog entry(ies)", entries.len()));
    Ok(())
}
