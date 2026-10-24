//! Identity conversions between Git and W0rkTree.
//!
//! * Snapshot ids for imported commits are UUID v5 values derived from the
//!   commit SHA in a fixed namespace, so re-importing the same history always
//!   yields the same snapshot ids.
//! * Snapshot authors are `"Name <email>"` strings; Git signatures need a
//!   separate name and email, so [`parse_author`] / [`format_author`] convert
//!   between the two with sensible fallbacks.

use chrono::{DateTime, Utc};
use git2::Oid;
use uuid::Uuid;

use crate::error::Result;

/// Namespace for [`snapshot_id_for_commit`]. Never change this value: doing
/// so would give every previously imported commit a new snapshot id.
pub const COMMIT_NAMESPACE: Uuid = Uuid::from_u128(0x5c3f_9a2e_7d41_4b8e_9f06_2a7c_d1e4_b358);

/// Name used in Git signatures when a snapshot author has no name.
pub const FALLBACK_NAME: &str = "unknown";

/// Email used in Git signatures when a snapshot author has no email.
pub const FALLBACK_EMAIL: &str = "unknown@localhost";

/// Deterministic snapshot id for a Git commit: UUID v5 of the lowercase hex
/// SHA in [`COMMIT_NAMESPACE`].
pub fn snapshot_id_for_commit(commit: Oid) -> String {
    Uuid::new_v5(&COMMIT_NAMESPACE, commit.to_string().as_bytes()).to_string()
}

/// An author split into its Git signature parts. Either part may be empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Author {
    pub name: String,
    pub email: String,
}

/// Parse a snapshot author string.
///
/// * `"Jane Doe <jane@example.com>"` -> name `Jane Doe`, email `jane@example.com`
/// * `"jane@example.com"` -> name `jane`, email `jane@example.com`
/// * `"jane"` -> name `jane`, empty email
///
/// Angle brackets and control characters are stripped from both parts since
/// Git signatures cannot contain them.
pub fn parse_author(raw: &str) -> Author {
    let raw = raw.trim();
    if let (Some(open), true) = (raw.rfind('<'), raw.ends_with('>')) {
        return Author {
            name: sanitize(&raw[..open]),
            email: sanitize(&raw[open + 1..raw.len() - 1]),
        };
    }
    if raw.contains('@') && !raw.contains(char::is_whitespace) {
        let email = sanitize(raw);
        let name = email.split('@').next().unwrap_or_default().to_owned();
        return Author { name, email };
    }
    Author {
        name: sanitize(raw),
        email: String::new(),
    }
}

/// Format a Git identity as a snapshot author string (`"Name <email>"`, or
/// just the name when there is no email).
pub fn format_author(name: &str, email: &str) -> String {
    let (name, email) = (name.trim(), email.trim());
    match (name.is_empty(), email.is_empty()) {
        (_, true) => name.to_owned(),
        (true, false) => format!("<{email}>"),
        (false, false) => format!("{name} <{email}>"),
    }
}

/// Git signature for a snapshot author at `at` (UTC, offset 0). Missing
/// parts fall back to [`FALLBACK_NAME`] / [`FALLBACK_EMAIL`].
pub fn signature(author: &str, at: DateTime<Utc>) -> Result<git2::Signature<'static>> {
    let parsed = parse_author(author);
    let name = if parsed.name.is_empty() {
        FALLBACK_NAME
    } else {
        parsed.name.as_str()
    };
    let email = if parsed.email.is_empty() {
        FALLBACK_EMAIL
    } else {
        parsed.email.as_str()
    };
    Ok(git2::Signature::new(
        name,
        email,
        &git2::Time::new(at.timestamp(), 0),
    )?)
}

/// Author string of a Git signature (lossy for non-UTF-8 names/emails).
pub fn author_of(sig: &git2::Signature<'_>) -> String {
    format_author(
        &String::from_utf8_lossy(sig.name_bytes()),
        &String::from_utf8_lossy(sig.email_bytes()),
    )
}

/// Convert a Git timestamp to UTC (out-of-range values clamp to the epoch).
pub fn git_time_to_utc(time: git2::Time) -> DateTime<Utc> {
    DateTime::from_timestamp(time.seconds(), 0).unwrap_or(DateTime::UNIX_EPOCH)
}

/// Normalise a message read from Git (trailing whitespace removed) for
/// storage in a snapshot or tag record.
pub fn message_from_git(raw: &[u8]) -> String {
    String::from_utf8_lossy(raw).trim_end().to_owned()
}

/// Message written to Git: trailing whitespace removed and, when non-empty,
/// terminated by a single newline (the convention `git commit` follows).
pub fn message_to_git(message: &str) -> String {
    let trimmed = message.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        format!("{trimmed}\n")
    }
}

fn sanitize(part: &str) -> String {
    part.chars()
        .filter(|c| !matches!(c, '<' | '>') && !c.is_control())
        .collect::<String>()
        .trim()
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oid(hex: &str) -> Oid {
        Oid::from_str(hex).expect("valid oid")
    }

    #[test]
    fn snapshot_ids_are_deterministic_v5_uuids() {
        let a = oid("0123456789abcdef0123456789abcdef01234567");
        let b = oid("89abcdef0123456789abcdef0123456789abcdef");
        let id = snapshot_id_for_commit(a);
        assert_eq!(id, snapshot_id_for_commit(a));
        assert_ne!(id, snapshot_id_for_commit(b));
        let parsed = Uuid::parse_str(&id).expect("uuid");
        assert_eq!(parsed.get_version_num(), 5);
        assert_eq!(
            parsed,
            Uuid::new_v5(
                &COMMIT_NAMESPACE,
                b"0123456789abcdef0123456789abcdef01234567"
            )
        );
        // Hyphenated lowercase form, as used by the SDK and the wire protocol.
        assert_eq!(id.len(), 36);
        assert_eq!(id, id.to_lowercase());
    }

    #[test]
    fn parse_full_author() {
        assert_eq!(
            parse_author("Jane Doe <jane@example.com>"),
            Author {
                name: "Jane Doe".into(),
                email: "jane@example.com".into()
            }
        );
        assert_eq!(
            parse_author("  Jane   <jane@example.com>  "),
            Author {
                name: "Jane".into(),
                email: "jane@example.com".into()
            }
        );
    }

    #[test]
    fn parse_email_only_and_name_only() {
        assert_eq!(
            parse_author("jane@example.com"),
            Author {
                name: "jane".into(),
                email: "jane@example.com".into()
            }
        );
        assert_eq!(
            parse_author("Jane Doe"),
            Author {
                name: "Jane Doe".into(),
                email: String::new()
            }
        );
        assert_eq!(
            parse_author("<jane@example.com>"),
            Author {
                name: String::new(),
                email: "jane@example.com".into()
            }
        );
        assert_eq!(
            parse_author(""),
            Author {
                name: String::new(),
                email: String::new()
            }
        );
    }

    #[test]
    fn parse_strips_invalid_characters() {
        let a = parse_author("Ja<ne> Doe <jane@ex\nample.com>");
        assert_eq!(a.name, "Jane Doe");
        assert_eq!(a.email, "jane@example.com");
        // Unterminated bracket: treated as a plain name.
        assert_eq!(parse_author("Jane <jane").name, "Jane jane");
    }

    #[test]
    fn format_round_trips_through_parse() {
        for (name, email) in [("Jane Doe", "jane@example.com"), ("bob", "")] {
            let s = format_author(name, email);
            let back = parse_author(&s);
            assert_eq!((back.name.as_str(), back.email.as_str()), (name, email));
        }
        assert_eq!(format_author("", "x@y"), "<x@y>");
        assert_eq!(format_author("", ""), "");
    }

    #[test]
    fn signature_uses_fallbacks_and_utc() {
        let at = DateTime::from_timestamp(1_700_000_000, 0).expect("ts");
        let sig = signature("Jane <jane@example.com>", at).expect("sig");
        assert_eq!(sig.name(), Some("Jane"));
        assert_eq!(sig.email(), Some("jane@example.com"));
        assert_eq!(sig.when().seconds(), 1_700_000_000);
        assert_eq!(sig.when().offset_minutes(), 0);

        let sig = signature("", at).expect("sig");
        assert_eq!(sig.name(), Some(FALLBACK_NAME));
        assert_eq!(sig.email(), Some(FALLBACK_EMAIL));

        let sig = signature("alice", at).expect("sig");
        assert_eq!(sig.name(), Some("alice"));
        assert_eq!(sig.email(), Some(FALLBACK_EMAIL));
        assert_eq!(author_of(&sig), format!("alice <{FALLBACK_EMAIL}>"));
    }

    #[test]
    fn time_conversion() {
        let t = git2::Time::new(1_600_000_000, 120);
        assert_eq!(git_time_to_utc(t).timestamp(), 1_600_000_000);
        assert_eq!(
            git_time_to_utc(git2::Time::new(i64::MAX, 0)),
            DateTime::UNIX_EPOCH
        );
    }

    #[test]
    fn message_normalisation() {
        assert_eq!(message_from_git(b"subject\n\nbody\n\n"), "subject\n\nbody");
        assert_eq!(message_to_git("subject\n\nbody"), "subject\n\nbody\n");
        assert_eq!(message_to_git("subject\n"), "subject\n");
        assert_eq!(message_to_git("   \n"), "");
        let original = "line one\n\nline two\n";
        assert_eq!(
            message_to_git(&message_from_git(original.as_bytes())),
            original
        );
    }
}
