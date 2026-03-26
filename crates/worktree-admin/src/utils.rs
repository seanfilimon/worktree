//! Utility functions for Worktree Admin Panel

use chrono::{DateTime, Utc, Local};

/// Format bytes as human-readable string
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit_idx = 0;

    while size >= 1024.0 && unit_idx < UNITS.len() - 1 {
        size /= 1024.0;
        unit_idx += 1;
    }

    if size >= 100.0 {
        format!("{:.0} {}", size, UNITS[unit_idx])
    } else if size >= 10.0 {
        format!("{:.1} {}", size, UNITS[unit_idx])
    } else {
        format!("{:.2} {}", size, UNITS[unit_idx])
    }
}

/// Format duration in seconds as human-readable string
pub fn format_duration(seconds: u64) -> String {
    let days = seconds / 86400;
    let hours = (seconds % 86400) / 3600;
    let minutes = (seconds % 3600) / 60;
    let secs = seconds % 60;

    if days > 0 {
        format!("{}d {}h", days, hours)
    } else if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, secs)
    } else {
        format!("{}s", secs)
    }
}

/// Format datetime as relative time (e.g., "2 hours ago")
pub fn format_relative_time(dt: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let duration = now.signed_duration_since(*dt);

    let seconds = duration.num_seconds();

    if seconds < 0 {
        return "in the future".to_string();
    }

    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    let months = days / 30;
    let years = days / 365;

    if years > 0 {
        format!("{} year{} ago", years, if years == 1 { "" } else { "s" })
    } else if months > 0 {
        format!("{} month{} ago", months, if months == 1 { "" } else { "s" })
    } else if days > 0 {
        format!("{} day{} ago", days, if days == 1 { "" } else { "s" })
    } else if hours > 0 {
        format!("{} hour{} ago", hours, if hours == 1 { "" } else { "s" })
    } else if minutes > 0 {
        format!("{} minute{} ago", minutes, if minutes == 1 { "" } else { "s" })
    } else {
        "just now".to_string()
    }
}

/// Format datetime as local time string
pub fn format_datetime(dt: &DateTime<Utc>) -> String {
    let local: DateTime<Local> = dt.with_timezone(&Local);
    local.format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Format datetime as short date
pub fn format_date(dt: &DateTime<Utc>) -> String {
    let local: DateTime<Local> = dt.with_timezone(&Local);
    local.format("%Y-%m-%d").to_string()
}

/// Format datetime as time only
pub fn format_time(dt: &DateTime<Utc>) -> String {
    let local: DateTime<Local> = dt.with_timezone(&Local);
    local.format("%H:%M:%S").to_string()
}

/// Format number with thousands separator
pub fn format_number(num: usize) -> String {
    let s = num.to_string();
    let mut result = String::new();
    let mut count = 0;

    for c in s.chars().rev() {
        if count > 0 && count % 3 == 0 {
            result.push(',');
        }
        result.push(c);
        count += 1;
    }

    result.chars().rev().collect()
}

/// Truncate string to max length with ellipsis
pub fn truncate(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// Get color class for percentage value
pub fn percentage_color_class(percentage: f64) -> &'static str {
    if percentage >= 90.0 {
        "text-error"
    } else if percentage >= 75.0 {
        "text-warning"
    } else if percentage >= 50.0 {
        "text-info"
    } else {
        "text-success"
    }
}

/// Calculate percentage
pub fn calculate_percentage(value: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (value as f64 / total as f64) * 100.0
    }
}

// ===== Inline Style Helpers using Shadcn CSS Variables =====

/// Build inline style string from CSS properties
pub fn inline_style(props: &[(&str, &str)]) -> String {
    props
        .iter()
        .map(|(key, value)| format!("{}: {}", key, value))
        .collect::<Vec<_>>()
        .join("; ")
}

/// Shadcn color utilities for inline styles
pub mod colors {
    /// Get background color using CSS variable
    pub fn bg(color: &str) -> String {
        format!("background-color: hsl(var(--{}))", color)
    }

    /// Get text color using CSS variable
    pub fn text(color: &str) -> String {
        format!("color: hsl(var(--{}))", color)
    }

    /// Get border color using CSS variable
    pub fn border(color: &str) -> String {
        format!("border-color: hsl(var(--{}))", color)
    }

    // Preset colors
    pub fn bg_background() -> &'static str { "background-color: hsl(var(--background))" }
    pub fn bg_card() -> &'static str { "background-color: hsl(var(--card))" }
    pub fn bg_primary() -> &'static str { "background-color: hsl(var(--primary))" }
    pub fn bg_secondary() -> &'static str { "background-color: hsl(var(--secondary))" }
    pub fn bg_muted() -> &'static str { "background-color: hsl(var(--muted))" }
    pub fn bg_accent() -> &'static str { "background-color: hsl(var(--accent))" }
    pub fn bg_destructive() -> &'static str { "background-color: hsl(var(--destructive))" }
    pub fn bg_success() -> &'static str { "background-color: hsl(var(--success))" }
    pub fn bg_warning() -> &'static str { "background-color: hsl(var(--warning))" }
    pub fn bg_info() -> &'static str { "background-color: hsl(var(--info))" }

    pub fn text_foreground() -> &'static str { "color: hsl(var(--foreground))" }
    pub fn text_muted_foreground() -> &'static str { "color: hsl(var(--muted-foreground))" }
    pub fn text_primary() -> &'static str { "color: hsl(var(--primary))" }
    pub fn text_destructive() -> &'static str { "color: hsl(var(--destructive))" }
    pub fn text_success() -> &'static str { "color: hsl(var(--success))" }
    pub fn text_warning() -> &'static str { "color: hsl(var(--warning))" }
    pub fn text_info() -> &'static str { "color: hsl(var(--info))" }
}

/// Layout utilities for inline styles
pub mod layout {
    pub fn flex() -> &'static str { "display: flex" }
    pub fn flex_col() -> &'static str { "display: flex; flex-direction: column" }
    pub fn items_center() -> &'static str { "align-items: center" }
    pub fn justify_center() -> &'static str { "justify-content: center" }
    pub fn justify_between() -> &'static str { "justify-content: space-between" }

    pub fn gap(size: &str) -> String {
        format!("gap: {}", size)
    }

    pub fn padding(size: &str) -> String {
        format!("padding: {}", size)
    }

    pub fn margin(size: &str) -> String {
        format!("margin: {}", size)
    }

    pub fn width(size: &str) -> String {
        format!("width: {}", size)
    }

    pub fn height(size: &str) -> String {
        format!("height: {}", size)
    }
}

/// Border utilities for inline styles
pub mod border {
    pub fn rounded() -> &'static str { "border-radius: var(--radius)" }
    pub fn rounded_lg() -> &'static str { "border-radius: calc(var(--radius) + 0.25rem)" }
    pub fn rounded_xl() -> &'static str { "border-radius: calc(var(--radius) + 0.5rem)" }
    pub fn rounded_full() -> &'static str { "border-radius: 9999px" }

    pub fn border() -> &'static str { "border: 1px solid hsl(var(--border))" }
    pub fn border_primary() -> &'static str { "border: 1px solid hsl(var(--primary))" }
}

/// Shadow utilities for inline styles
pub mod shadow {
    pub fn sm() -> &'static str { "box-shadow: 0 1px 2px 0 rgb(0 0 0 / 0.05)" }
    pub fn md() -> &'static str { "box-shadow: 0 4px 6px -1px rgb(0 0 0 / 0.1), 0 2px 4px -2px rgb(0 0 0 / 0.1)" }
    pub fn lg() -> &'static str { "box-shadow: 0 10px 15px -3px rgb(0 0 0 / 0.1), 0 4px 6px -4px rgb(0 0 0 / 0.1)" }
}

/// Transition utilities for inline styles
pub mod transition {
    pub fn all() -> &'static str { "transition: all 150ms cubic-bezier(0.4, 0, 0.2, 1)" }
    pub fn colors() -> &'static str { "transition: color, background-color, border-color 150ms cubic-bezier(0.4, 0, 0.2, 1)" }
}

/// Typography utilities for inline styles
pub mod typography {
    pub fn text_xs() -> &'static str { "font-size: 0.75rem" }
    pub fn text_sm() -> &'static str { "font-size: 0.875rem" }
    pub fn text_base() -> &'static str { "font-size: 1rem" }
    pub fn text_lg() -> &'static str { "font-size: 1.125rem" }
    pub fn text_xl() -> &'static str { "font-size: 1.25rem" }
    pub fn text_2xl() -> &'static str { "font-size: 1.5rem" }
    pub fn text_3xl() -> &'static str { "font-size: 1.875rem" }

    pub fn font_normal() -> &'static str { "font-weight: 400" }
    pub fn font_medium() -> &'static str { "font-weight: 500" }
    pub fn font_semibold() -> &'static str { "font-weight: 600" }
    pub fn font_bold() -> &'static str { "font-weight: 700" }
}

/// Combine multiple style strings
pub fn combine_styles(styles: &[&str]) -> String {
    styles
        .iter()
        .filter(|s| !s.is_empty())
        .map(|s| s.trim_end_matches(';'))
        .collect::<Vec<_>>()
        .join("; ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_bytes(1024 * 1024 * 1024 * 1024), "1.00 TB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(30), "30s");
        assert_eq!(format_duration(90), "1m 30s");
        assert_eq!(format_duration(3600), "1h 0m");
        assert_eq!(format_duration(86400), "1d 0h");
        assert_eq!(format_duration(90000), "1d 1h");
    }

    #[test]
    fn test_format_relative_time() {
        let now = Utc::now();
        let one_hour_ago = now - Duration::hours(1);
        let one_day_ago = now - Duration::days(1);

        let result = format_relative_time(&one_hour_ago);
        assert!(result.contains("hour"));

        let result = format_relative_time(&one_day_ago);
        assert!(result.contains("day"));
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1000000), "1,000,000");
        assert_eq!(format_number(123), "123");
    }

    #[test]
    fn test_truncate() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("hello world", 8), "hello...");
        assert_eq!(truncate("hi", 5), "hi");
    }

    #[test]
    fn test_calculate_percentage() {
        assert_eq!(calculate_percentage(50, 100), 50.0);
        assert_eq!(calculate_percentage(75, 100), 75.0);
        assert_eq!(calculate_percentage(0, 0), 0.0);
    }

    #[test]
    fn test_percentage_color_class() {
        assert_eq!(percentage_color_class(95.0), "text-error");
        assert_eq!(percentage_color_class(80.0), "text-warning");
        assert_eq!(percentage_color_class(60.0), "text-info");
        assert_eq!(percentage_color_class(30.0), "text-success");
    }

    #[test]
    fn test_inline_style() {
        let style = inline_style(&[("color", "red"), ("font-size", "16px")]);
        assert!(style.contains("color: red"));
        assert!(style.contains("font-size: 16px"));
    }

    #[test]
    fn test_combine_styles() {
        let combined = combine_styles(&[
            colors::bg_primary(),
            layout::flex(),
            border::rounded()
        ]);
        assert!(combined.contains("background-color"));
        assert!(combined.contains("display"));
        assert!(combined.contains("border-radius"));
    }
}
