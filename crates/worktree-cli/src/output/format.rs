//! Small formatting helpers shared by the human renderers.

use chrono::{DateTime, Local, Utc};

/// Length of abbreviated snapshot ids (matches `Snapshot::short_id`).
pub const SHORT_ID_LEN: usize = 8;

/// Abbreviate a snapshot id.
pub fn short(id: &str) -> &str {
    id.get(..SHORT_ID_LEN).unwrap_or(id)
}

/// Abbreviate an optional snapshot id; `none` stands in for "no snapshot".
pub fn short_or<'a>(id: Option<&'a str>, none: &'a str) -> &'a str {
    id.map_or(none, short)
}

/// "1 file", "3 files".
pub fn plural(count: usize, singular: &str, plural: &str) -> String {
    format!("{count} {}", if count == 1 { singular } else { plural })
}

/// Git-style relative time ("5 minutes ago").
pub fn ago(then: DateTime<Utc>) -> String {
    relative_time(then, Utc::now())
}

pub fn relative_time(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let secs = (now - then).num_seconds();
    if secs < 0 {
        return "in the future".into();
    }
    const MINUTE: i64 = 60;
    const HOUR: i64 = 60 * MINUTE;
    const DAY: i64 = 24 * HOUR;
    const WEEK: i64 = 7 * DAY;
    const MONTH: i64 = 30 * DAY;
    const YEAR: i64 = 365 * DAY;
    let (n, unit) = match secs {
        s if s < 10 => return "just now".into(),
        s if s < MINUTE => (s, "second"),
        s if s < HOUR => (s / MINUTE, "minute"),
        s if s < DAY => (s / HOUR, "hour"),
        s if s < 2 * WEEK => (s / DAY, "day"),
        s if s < 2 * MONTH => (s / WEEK, "week"),
        s if s < YEAR => (s / MONTH, "month"),
        s => (s / YEAR, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

/// Absolute timestamp in the local time zone, git style.
pub fn date(at: DateTime<Utc>) -> String {
    at.with_timezone(&Local)
        .format("%a %b %e %H:%M:%S %Y %z")
        .to_string()
}

/// Split a `--stat` bar of at most `width` characters between additions
/// and deletions, scaling by `max_total` (the largest change in the set).
pub fn stat_bar(added: usize, removed: usize, max_total: usize, width: usize) -> (usize, usize) {
    let total = added + removed;
    if total == 0 {
        return (0, 0);
    }
    if max_total <= width {
        return (added, removed);
    }
    let scaled = (total * width).div_ceil(max_total);
    // Never hide a non-zero side entirely.
    let share = |n: usize| {
        if n == 0 {
            0
        } else {
            (n * scaled / total).max(1)
        }
    };
    (share(added), share(removed))
}

/// First line of a message.
pub fn subject(message: &str) -> &str {
    message.lines().next().unwrap_or("")
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    #[test]
    fn short_ids() {
        assert_eq!(short("0123456789abcdef"), "01234567");
        assert_eq!(short("abc"), "abc");
        assert_eq!(short_or(Some("0123456789"), "-"), "01234567");
        assert_eq!(short_or(None, "(none)"), "(none)");
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "file", "files"), "1 file");
        assert_eq!(plural(0, "file", "files"), "0 files");
        assert_eq!(plural(7, "entry", "entries"), "7 entries");
    }

    #[test]
    fn relative_times() {
        let now = Utc::now();
        let cases = [
            (Duration::seconds(-5), "in the future"),
            (Duration::seconds(3), "just now"),
            (Duration::seconds(42), "42 seconds ago"),
            (Duration::minutes(1), "1 minute ago"),
            (Duration::minutes(59), "59 minutes ago"),
            (Duration::hours(5), "5 hours ago"),
            (Duration::days(1), "1 day ago"),
            (Duration::days(13), "13 days ago"),
            (Duration::days(21), "3 weeks ago"),
            (Duration::days(90), "3 months ago"),
            (Duration::days(365), "1 year ago"),
            (Duration::days(800), "2 years ago"),
        ];
        for (delta, expected) in cases {
            assert_eq!(relative_time(now - delta, now), expected, "{delta:?}");
        }
    }

    #[test]
    fn dates_render() {
        let rendered = date(Utc::now());
        assert!(rendered.len() > 20, "{rendered}");
    }

    #[test]
    fn stat_bars() {
        assert_eq!(stat_bar(0, 0, 10, 40), (0, 0));
        assert_eq!(stat_bar(3, 2, 10, 40), (3, 2));
        let (p, m) = stat_bar(100, 100, 200, 40);
        assert_eq!(p + m, 40);
        assert_eq!(p, 20);
        let (p, m) = stat_bar(1, 0, 1000, 40);
        assert_eq!((p, m), (1, 0));
        let (p, m) = stat_bar(999, 1, 1000, 40);
        assert!(p >= 1 && m >= 1 && p + m <= 41, "{p} {m}");
    }

    #[test]
    fn subjects() {
        assert_eq!(subject("first\nsecond"), "first");
        assert_eq!(subject(""), "");
    }
}
