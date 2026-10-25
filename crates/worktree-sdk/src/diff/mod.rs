//! Pure text diffing (no I/O). Used by `Repository::diff_*`.
//!
//! Text is compared line by line, where a line is everything up to and
//! including a `\n`. Line contents are preserved exactly: CRLF files keep
//! their `\r`, and a final line without a terminator is a different line
//! from the same text with one (so adding or removing the trailing newline
//! shows up as a change, exactly like GNU diff).
//!
//! Lines are matched with Myers' O(ND) algorithm (linear-space variant); see
//! the `myers` submodule.

mod myers;
#[cfg(test)]
pub(crate) mod tests;

use std::ops::Range;

use serde::{Deserialize, Serialize};

pub(crate) use myers::match_lines;

/// One line of a hunk. The text excludes the trailing `\n` (a `\r` from a
/// CRLF line ending is kept). Whether a file's last line lacks a newline is
/// not represented here; [`unified_diff`] emits the
/// `\ No newline at end of file` marker for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
}

/// A unified-diff hunk. Line numbers are 1-based; a zero-length side has
/// start = line before the hunk (0 at file start), as in GNU diff.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hunk {
    pub old_start: usize,
    pub old_len: usize,
    pub new_start: usize,
    pub new_len: usize,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffStat {
    pub added: usize,
    pub removed: usize,
}

/// Text written after a line that is the last line of a file lacking a
/// trailing newline.
const NO_NEWLINE_MARKER: &str = "\\ No newline at end of file\n";

/// Bytes inspected for NUL by [`is_binary`].
const BINARY_SNIFF_LEN: usize = 8 * 1024;

/// Line diff (Myers or histogram) grouped into hunks with `context` lines.
/// Handles a missing trailing newline.
///
/// Within a hunk, removed lines of a change precede its added lines. Changes
/// separated by at most `2 * context` unchanged lines share a hunk. Returns
/// an empty vector when the texts are equal.
pub fn diff_lines(old: &str, new: &str, context: usize) -> Vec<Hunk> {
    if old == new {
        return Vec::new();
    }
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let matches = match_lines(&old_lines, &new_lines);
    build_hunks(
        &old_lines,
        &new_lines,
        &changes(&matches, old_lines.len(), new_lines.len()),
        context,
    )
}

/// Full unified diff text with `--- a/<old_path>` / `+++ b/<new_path>`
/// headers (use /dev/null for added/deleted files). Empty string if equal.
///
/// A side is shown as `/dev/null` when its text is empty or its path is
/// empty or `/dev/null`; callers diffing an added or deleted file simply
/// pass `""` as the missing side's content. Every emitted line ends with
/// `\n`; a side whose last line lacks a newline gets a
/// `\ No newline at end of file` marker after that line.
pub fn unified_diff(
    old_path: &str,
    new_path: &str,
    old: &str,
    new: &str,
    context: usize,
) -> String {
    if old == new {
        return String::new();
    }
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let matches = match_lines(&old_lines, &new_lines);
    let hunks = build_hunks(
        &old_lines,
        &new_lines,
        &changes(&matches, old_lines.len(), new_lines.len()),
        context,
    );

    let old_missing_eol = !old.is_empty() && !old.ends_with('\n');
    let new_missing_eol = !new.is_empty() && !new.ends_with('\n');

    let mut out = String::with_capacity(old.len() + new.len() + 64);
    out.push_str("--- ");
    out.push_str(&header_path("a/", old_path, old.is_empty()));
    out.push_str("\n+++ ");
    out.push_str(&header_path("b/", new_path, new.is_empty()));
    out.push('\n');

    for hunk in &hunks {
        out.push_str("@@ -");
        push_range(&mut out, hunk.old_start, hunk.old_len);
        out.push_str(" +");
        push_range(&mut out, hunk.new_start, hunk.new_len);
        out.push_str(" @@\n");

        // 0-based index of the next old/new line the hunk consumes.
        let mut old_idx = zero_based_start(hunk.old_start, hunk.old_len);
        let mut new_idx = zero_based_start(hunk.new_start, hunk.new_len);
        for line in &hunk.lines {
            let (prefix, text, ends_file) = match line {
                DiffLine::Context(text) => {
                    old_idx += 1;
                    new_idx += 1;
                    (' ', text, old_missing_eol && old_idx == old_lines.len())
                }
                DiffLine::Removed(text) => {
                    old_idx += 1;
                    ('-', text, old_missing_eol && old_idx == old_lines.len())
                }
                DiffLine::Added(text) => {
                    new_idx += 1;
                    ('+', text, new_missing_eol && new_idx == new_lines.len())
                }
            };
            out.push(prefix);
            out.push_str(text);
            out.push('\n');
            if ends_file {
                out.push_str(NO_NEWLINE_MARKER);
            }
        }
    }
    out
}

/// Count added/removed lines.
pub fn diff_stat(old: &str, new: &str) -> DiffStat {
    if old == new {
        return DiffStat::default();
    }
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let common = match_lines(&old_lines, &new_lines).len();
    DiffStat {
        added: new_lines.len() - common,
        removed: old_lines.len() - common,
    }
}

/// Line similarity of two texts in percent: twice the number of common
/// lines over the total number of lines (100 for equal texts, including two
/// empty ones; 0 when nothing is shared).
pub fn similarity(old: &str, new: &str) -> u8 {
    if old == new {
        return 100;
    }
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let total = old_lines.len() + new_lines.len();
    if total == 0 {
        return 100;
    }
    let common = match_lines(&old_lines, &new_lines).len();
    // Rounded down, so only identical texts score 100.
    u8::try_from((common * 2 * 100 / total).min(99)).unwrap_or(99)
}

/// Unified diff with changed words marked inline, like
/// `git diff --word-diff=plain`: headers and `@@` lines as in
/// [`unified_diff`], unchanged lines printed without a prefix, and within
/// changed lines removed text as `[-text-]` and added text as `{+text+}`.
/// Words are runs of alphanumerics/underscores, runs of other whitespace,
/// or single punctuation characters. Empty string if the texts are equal.
pub fn word_diff(old_path: &str, new_path: &str, old: &str, new: &str, context: usize) -> String {
    if old == new {
        return String::new();
    }
    let old_lines = split_lines(old);
    let new_lines = split_lines(new);
    let matches = match_lines(&old_lines, &new_lines);
    let hunks = build_hunks(
        &old_lines,
        &new_lines,
        &changes(&matches, old_lines.len(), new_lines.len()),
        context,
    );

    let mut out = String::with_capacity(old.len() + new.len() + 64);
    out.push_str("--- ");
    out.push_str(&header_path("a/", old_path, old.is_empty()));
    out.push_str("\n+++ ");
    out.push_str(&header_path("b/", new_path, new.is_empty()));
    out.push('\n');
    for hunk in &hunks {
        out.push_str("@@ -");
        push_range(&mut out, hunk.old_start, hunk.old_len);
        out.push_str(" +");
        push_range(&mut out, hunk.new_start, hunk.new_len);
        out.push_str(" @@\n");
        let mut removed = String::new();
        let mut added = String::new();
        for line in &hunk.lines {
            match line {
                DiffLine::Context(text) => {
                    flush_word_change(&mut out, &removed, &added);
                    removed.clear();
                    added.clear();
                    out.push_str(text);
                    out.push('\n');
                }
                DiffLine::Removed(text) => {
                    removed.push_str(text);
                    removed.push('\n');
                }
                DiffLine::Added(text) => {
                    added.push_str(text);
                    added.push('\n');
                }
            }
        }
        flush_word_change(&mut out, &removed, &added);
    }
    out
}

/// Writes one replaced block (whole lines, each ending in `\n`) as a word
/// diff.
fn flush_word_change(out: &mut String, removed: &str, added: &str) {
    if removed.is_empty() && added.is_empty() {
        return;
    }
    let old_words = split_words(removed);
    let new_words = split_words(added);
    let matches = match_lines(&old_words, &new_words);
    let (mut i, mut j) = (0, 0);
    for &(mi, mj) in matches
        .iter()
        .chain(std::iter::once(&(old_words.len(), new_words.len())))
    {
        push_marked(out, &old_words[i..mi].concat(), "[-", "-]");
        push_marked(out, &new_words[j..mj].concat(), "{+", "+}");
        if mi < old_words.len() {
            out.push_str(old_words[mi]);
        }
        (i, j) = (mi + 1, mj + 1);
    }
}

/// Wraps `text` in markers line by line, so markers never span a newline.
fn push_marked(out: &mut String, text: &str, open: &str, close: &str) {
    for piece in text.split_inclusive('\n') {
        let body = strip_newline(piece);
        if !body.is_empty() {
            out.push_str(open);
            out.push_str(body);
            out.push_str(close);
        }
        if piece.ends_with('\n') {
            out.push('\n');
        }
    }
}

/// Splits text into word-diff tokens: runs of word characters, runs of
/// non-newline whitespace, newlines, and single other characters.
pub(crate) fn split_words(text: &str) -> Vec<&str> {
    #[derive(PartialEq, Clone, Copy)]
    enum Class {
        Word,
        Space,
        Other,
    }
    let class = |c: char| {
        if c.is_alphanumeric() || c == '_' {
            Class::Word
        } else if c.is_whitespace() && c != '\n' {
            Class::Space
        } else {
            Class::Other
        }
    };
    let mut tokens = Vec::new();
    let mut start = 0;
    let mut current: Option<Class> = None;
    for (i, c) in text.char_indices() {
        let cls = class(c);
        let joins = current == Some(cls) && cls != Class::Other;
        if !joins && i > start {
            tokens.push(&text[start..i]);
            start = i;
        }
        current = Some(cls);
    }
    if start < text.len() {
        tokens.push(&text[start..]);
    }
    tokens
}

/// Heuristic: NUL byte in the first 8 KiB, or invalid UTF-8.
pub fn is_binary(data: &[u8]) -> bool {
    data[..data.len().min(BINARY_SNIFF_LEN)].contains(&0) || std::str::from_utf8(data).is_err()
}

/// Splits `text` into lines, each including its `\n` terminator (the last
/// line may lack one). Empty text has no lines.
pub(crate) fn split_lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// `line` without its trailing `\n`, if any.
pub(crate) fn strip_newline(line: &str) -> &str {
    line.strip_suffix('\n').unwrap_or(line)
}

/// A maximal run of non-matching lines: `old` lines replaced by `new` lines.
/// At most one of the two ranges is empty.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    old: Range<usize>,
    new: Range<usize>,
}

/// Converts an increasing matching into the changes between matched lines.
fn changes(matches: &[(usize, usize)], old_len: usize, new_len: usize) -> Vec<Change> {
    let mut out = Vec::new();
    let (mut i, mut j) = (0, 0);
    for &(mi, mj) in matches.iter().chain(std::iter::once(&(old_len, new_len))) {
        if i < mi || j < mj {
            out.push(Change {
                old: i..mi,
                new: j..mj,
            });
        }
        i = mi + 1;
        j = mj + 1;
    }
    out
}

/// Groups changes into hunks with up to `context` unchanged lines around
/// them. Unchanged lines between changes are identical on both sides, so
/// context is always taken from `old`.
fn build_hunks(old: &[&str], new: &[&str], changes: &[Change], context: usize) -> Vec<Hunk> {
    let mut hunks = Vec::new();
    let mut rest = changes;
    while let Some(first) = rest.first() {
        // Extend the group while the gap to the next change is small enough
        // that the two context windows would touch or overlap.
        let mut count = 1;
        while count < rest.len()
            && rest[count].old.start - rest[count - 1].old.end <= context.saturating_mul(2)
        {
            count += 1;
        }
        let (group, tail) = rest.split_at(count);
        rest = tail;
        let last = &group[count - 1];

        let lead = context.min(first.old.start);
        let trail = context.min(old.len() - last.old.end);
        let old_range = first.old.start - lead..last.old.end + trail;
        let new_range = first.new.start - lead..last.new.end + trail;

        let mut lines = Vec::with_capacity(old_range.len() + new_range.len());
        let mut pos = old_range.start;
        for change in group {
            lines.extend(
                old[pos..change.old.start]
                    .iter()
                    .map(|l| DiffLine::Context(own(l))),
            );
            lines.extend(
                old[change.old.clone()]
                    .iter()
                    .map(|l| DiffLine::Removed(own(l))),
            );
            lines.extend(
                new[change.new.clone()]
                    .iter()
                    .map(|l| DiffLine::Added(own(l))),
            );
            pos = change.old.end;
        }
        lines.extend(
            old[pos..old_range.end]
                .iter()
                .map(|l| DiffLine::Context(own(l))),
        );

        hunks.push(Hunk {
            old_start: one_based_start(old_range.start, old_range.len()),
            old_len: old_range.len(),
            new_start: one_based_start(new_range.start, new_range.len()),
            new_len: new_range.len(),
            lines,
        });
    }
    hunks
}

fn own(line: &str) -> String {
    strip_newline(line).to_owned()
}

/// GNU numbering: 1-based first line, or the line before an empty range.
fn one_based_start(start: usize, len: usize) -> usize {
    if len == 0 {
        start
    } else {
        start + 1
    }
}

/// Inverse of [`one_based_start`].
fn zero_based_start(start: usize, len: usize) -> usize {
    if len == 0 {
        start
    } else {
        start - 1
    }
}

/// Hunk range in GNU form: `start` for one line, `start,len` otherwise.
fn push_range(out: &mut String, start: usize, len: usize) {
    out.push_str(&start.to_string());
    if len != 1 {
        out.push(',');
        out.push_str(&len.to_string());
    }
}

fn header_path(prefix: &str, path: &str, absent: bool) -> String {
    if absent || path.is_empty() || path == "/dev/null" {
        "/dev/null".to_owned()
    } else {
        format!("{prefix}{path}")
    }
}
