use crate::output::format;
use worktree_sdk::Client;

pub async fn execute() -> Result<(), Box<dyn std::error::Error>> {
    let client = Client::open_current()?;
    let report = client.doctor()?;

    format::print_header("Store Health");
    format::print_kv("Store", &report.store_dir.display().to_string());
    println!();

    for (kind, count) in &report.counts {
        format::print_kv(kind, &count.to_string());
    }
    println!();

    if report.corrupt.is_empty() {
        format::print_success("All objects verified — no corruption found.");
    } else {
        format::print_error(&format!(
            "{} corrupt object(s) detected:",
            report.corrupt.len()
        ));
        for (kind, hex, error) in &report.corrupt {
            format::print_list_item(&format!("{kind}/{hex}: {error}"));
        }
        return Err(format!("{} corrupt object(s)", report.corrupt.len()).into());
    }

    Ok(())
}
