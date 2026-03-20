use colored::Colorize;

/// Print a styled header line.
pub fn print_header(text: &str) {
    println!("{}", text.bold().underline());
}

/// Print a success message in green with a checkmark prefix.
pub fn print_success(text: &str) {
    println!("{} {}", "✔".green().bold(), text.green());
}

/// Print an error message in red with a cross prefix.
pub fn print_error(text: &str) {
    println!("{} {}", "✖".red().bold(), text.red());
}

/// Print an informational message in cyan with an info prefix.
pub fn print_info(text: &str) {
    println!("{} {}", "ℹ".cyan().bold(), text.cyan());
}

/// Print a warning message in yellow with a warning prefix.
pub fn print_warning(text: &str) {
    println!("{} {}", "⚠".yellow().bold(), text.yellow());
}

/// Print a key-value pair with the key dimmed and the value normal.
pub fn print_kv(key: &str, value: &str) {
    println!("  {}: {}", key.dimmed(), value);
}

/// Print a list item with a bullet prefix.
pub fn print_list_item(text: &str) {
    println!("  {} {}", "•".dimmed(), text);
}

/// Return a styled hash string (yellow, for snapshot IDs).
pub fn styled_hash(hash: &str) -> String {
    format!("{}", hash.yellow().bold())
}
