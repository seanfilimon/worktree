//! Tests for line diffing, plus small test helpers shared with `merge`.

use super::*;

/// Deterministic xorshift64* generator for property-style tests.
pub(crate) struct Rng(u64);

impl Rng {
    pub(crate) fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    pub(crate) fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform-ish value in `0..n` (`n > 0`).
    pub(crate) fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub(crate) fn chance(&mut self, percent: usize) -> bool {
        self.below(100) < percent
    }

    /// Random text over a tiny line alphabet (so lines repeat a lot), with
    /// occasional CRLF endings and a random trailing newline.
    pub(crate) fn text(&mut self, max_lines: usize) -> String {
        const ALPHABET: [&str; 8] = ["a", "b", "c", "d", "", "}", "fn f() {", "x y"];
        let count = self.below(max_lines + 1);
        let mut out = String::new();
        for i in 0..count {
            out.push_str(ALPHABET[self.below(ALPHABET.len())]);
            if i + 1 < count || self.chance(70) {
                out.push_str(if self.chance(10) { "\r\n" } else { "\n" });
            }
        }
        out
    }

    /// `base` with a few random line edits (replace, insert, delete).
    pub(crate) fn mutate(&mut self, base: &str) -> String {
        let mut lines: Vec<String> = split_lines(base).into_iter().map(str::to_owned).collect();
        for _ in 0..self.below(4) {
            let pos = self.below(lines.len() + 1);
            let fresh = format!("edit{}\n", self.below(5));
            match self.below(3) {
                0 if pos < lines.len() => lines[pos] = fresh,
                1 if pos < lines.len() => {
                    lines.remove(pos);
                }
                _ => lines.insert(pos, fresh),
            }
        }
        lines.concat()
    }
}

/// A hunk body line: tag (`' '`, `-`, `+`) and text including its newline.
type BodyLine = (char, String);

/// Applies a unified diff produced by [`unified_diff`] to `old`, verifying
/// every context and removed line. Test-only.
fn apply_patch(old: &str, patch: &str) -> Result<String, String> {
    if patch.is_empty() {
        return Ok(old.to_owned());
    }
    let old_lines = split_lines(old);
    let mut out = String::new();
    let mut pos = 0; // next unconsumed old line
    let mut body: Vec<BodyLine> = Vec::new();
    let mut hunks: Vec<(usize, usize, Vec<BodyLine>)> = Vec::new();

    let mut lines = split_lines(patch).into_iter();
    let header = (lines.next(), lines.next());
    match header {
        (Some(a), Some(b)) if a.starts_with("--- ") && b.starts_with("+++ ") => {}
        _ => return Err("missing file headers".into()),
    }
    let mut current: Option<(usize, usize)> = None;
    for line in lines {
        if let Some(rest) = line.strip_prefix("@@ -") {
            if let Some((start, len)) = current.take() {
                hunks.push((start, len, std::mem::take(&mut body)));
            }
            let old_range = rest.split(' ').next().ok_or("bad hunk header")?;
            let (start, len) = match old_range.split_once(',') {
                Some((s, l)) => (s.parse::<usize>(), l.parse::<usize>()),
                None => (old_range.parse::<usize>(), Ok(1)),
            };
            current = Some((
                start.map_err(|e| e.to_string())?,
                len.map_err(|e| e.to_string())?,
            ));
        } else if line == NO_NEWLINE_MARKER {
            let last = body.last_mut().ok_or("marker without line")?;
            last.1
                .pop()
                .filter(|c| *c == '\n')
                .ok_or("marker after unterminated line")?;
        } else {
            let mut chars = line.chars();
            let tag = chars.next().ok_or("empty body line")?;
            if !matches!(tag, ' ' | '-' | '+') || !line.ends_with('\n') {
                return Err(format!("bad body line {line:?}"));
            }
            body.push((tag, chars.as_str().to_owned()));
        }
    }
    let (start, len) = current.ok_or("no hunks")?;
    hunks.push((start, len, body));

    for (start, len, body) in hunks {
        let begin = if len == 0 { start } else { start - 1 };
        if begin < pos || begin > old_lines.len() {
            return Err(format!("hunk at {start} out of order"));
        }
        old_lines[pos..begin].iter().for_each(|l| out.push_str(l));
        pos = begin;
        for (tag, text) in body {
            if tag != '+' {
                if old_lines.get(pos) != Some(&text.as_str()) {
                    return Err(format!("mismatch at old line {}: {text:?}", pos + 1));
                }
                pos += 1;
            }
            if tag != '-' {
                out.push_str(&text);
            }
        }
        if pos - begin != len {
            return Err(format!(
                "hunk at {start} consumed {} lines, header says {len}",
                pos - begin
            ));
        }
    }
    old_lines[pos..].iter().for_each(|l| out.push_str(l));
    Ok(out)
}

/// Reference LCS length by dynamic programming.
fn lcs_len(a: &[&str], b: &[&str]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diag = 0;
        for (j, y) in b.iter().enumerate() {
            let up = row[j + 1];
            row[j + 1] = if x == y { diag + 1 } else { up.max(row[j]) };
            diag = up;
        }
    }
    row[b.len()]
}

fn ctx(s: &str) -> DiffLine {
    DiffLine::Context(s.into())
}
fn add(s: &str) -> DiffLine {
    DiffLine::Added(s.into())
}
fn del(s: &str) -> DiffLine {
    DiffLine::Removed(s.into())
}

fn numbered(range: std::ops::RangeInclusive<usize>) -> String {
    range.map(|i| format!("{i}\n")).collect()
}

/// Lines `1..=n` with the listed line numbers replaced by the given text
/// (which may span several lines, or be empty to delete the line).
fn edited(n: usize, edits: &[(usize, &str)]) -> String {
    (1..=n)
        .map(|i| {
            edits
                .iter()
                .find(|(at, _)| *at == i)
                .map_or(format!("{i}\n"), |(_, text)| (*text).to_owned())
        })
        .collect()
}

// ---------------------------------------------------------------- diff_lines

#[test]
fn equal_texts_have_no_hunks() {
    assert!(diff_lines("a\nb\n", "a\nb\n", 3).is_empty());
    assert!(diff_lines("", "", 3).is_empty());
    assert_eq!(unified_diff("x", "x", "same", "same", 3), "");
    assert_eq!(diff_stat("a\n", "a\n"), DiffStat::default());
}

#[test]
fn single_change_with_context() {
    let old = numbered(1..=9);
    let new = old.replace("5\n", "five\n");
    let hunks = diff_lines(&old, &new, 2);
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 3,
            old_len: 5,
            new_start: 3,
            new_len: 5,
            lines: vec![
                ctx("3"),
                ctx("4"),
                del("5"),
                add("five"),
                ctx("6"),
                ctx("7")
            ],
        }]
    );
}

#[test]
fn add_to_empty_file_uses_zero_start() {
    let hunks = diff_lines("", "a\nb\n", 3);
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 0,
            old_len: 0,
            new_start: 1,
            new_len: 2,
            lines: vec![add("a"), add("b")]
        }]
    );
}

#[test]
fn delete_everything_uses_zero_start() {
    let hunks = diff_lines("a\nb\n", "", 3);
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 1,
            old_len: 2,
            new_start: 0,
            new_len: 0,
            lines: vec![del("a"), del("b")]
        }]
    );
}

#[test]
fn pure_insertion_numbers_line_before() {
    // Insert after line 2 with no context: old side is empty at line 2.
    let hunks = diff_lines("1\n2\n3\n", "1\n2\nnew\n3\n", 0);
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 2,
            old_len: 0,
            new_start: 3,
            new_len: 1,
            lines: vec![add("new")]
        }]
    );
}

#[test]
fn pure_deletion_numbers_line_before() {
    let hunks = diff_lines("1\n2\n3\n", "1\n2\n", 0);
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 3,
            old_len: 1,
            new_start: 2,
            new_len: 0,
            lines: vec![del("3")]
        }]
    );
}

#[test]
fn insertion_at_start_without_context() {
    let hunks = diff_lines("1\n", "0\n1\n", 0);
    assert_eq!(
        hunks,
        vec![Hunk {
            old_start: 0,
            old_len: 0,
            new_start: 1,
            new_len: 1,
            lines: vec![add("0")]
        }]
    );
}

#[test]
fn removals_precede_additions_within_a_change() {
    let hunks = diff_lines("keep\nold1\nold2\nkeep2\n", "keep\nnew1\nnew2\nkeep2\n", 0);
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        hunks[0].lines,
        vec![del("old1"), del("old2"), add("new1"), add("new2")]
    );
}

#[test]
fn nearby_changes_share_a_hunk_at_twice_context() {
    // Changes at lines 2 and 9: six unchanged lines between them == 2 * 3.
    let old = numbered(1..=12);
    let new = edited(12, &[(2, "two\n"), (9, "nine\n")]);
    let hunks = diff_lines(&old, &new, 3);
    assert_eq!(hunks.len(), 1);
    assert_eq!((hunks[0].old_start, hunks[0].old_len), (1, 12));
}

#[test]
fn distant_changes_split_into_hunks() {
    // Changes at lines 2 and 10: seven unchanged lines between them.
    let old = numbered(1..=20);
    let new = edited(20, &[(2, "two\n"), (10, "ten\n")]);
    let hunks = diff_lines(&old, &new, 3);
    assert_eq!(hunks.len(), 2);
    assert_eq!(
        (
            hunks[0].old_start,
            hunks[0].old_len,
            hunks[0].new_start,
            hunks[0].new_len
        ),
        (1, 5, 1, 5)
    );
    assert_eq!(
        (
            hunks[1].old_start,
            hunks[1].old_len,
            hunks[1].new_start,
            hunks[1].new_len
        ),
        (7, 7, 7, 7)
    );
}

#[test]
fn hunk_offsets_track_earlier_size_changes() {
    let old = numbered(1..=20);
    let new = edited(20, &[(2, "2a\n2b\n2c\n"), (15, "")]);
    let hunks = diff_lines(&old, &new, 1);
    assert_eq!(hunks.len(), 2);
    assert_eq!(
        (
            hunks[1].old_start,
            hunks[1].old_len,
            hunks[1].new_start,
            hunks[1].new_len
        ),
        (14, 3, 16, 2)
    );
}

#[test]
fn context_larger_than_file_and_huge_context() {
    for context in [50, usize::MAX] {
        let hunks = diff_lines("a\nb\nc\n", "a\nB\nc\n", context);
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].lines, vec![ctx("a"), del("b"), add("B"), ctx("c")]);
    }
}

#[test]
fn crlf_lines_keep_carriage_return() {
    let hunks = diff_lines("a\r\nb\r\n", "a\r\nc\r\n", 1);
    assert_eq!(hunks[0].lines, vec![ctx("a\r"), del("b\r"), add("c\r")]);
}

#[test]
fn line_ending_conversion_changes_every_line() {
    let stat = diff_stat("a\r\nb\r\n", "a\nb\n");
    assert_eq!(
        stat,
        DiffStat {
            added: 2,
            removed: 2
        }
    );
}

#[test]
fn trailing_newline_change_is_a_line_change() {
    let hunks = diff_lines("a\nb", "a\nb\n", 3);
    assert_eq!(hunks[0].lines, vec![ctx("a"), del("b"), add("b")]);
}

#[test]
fn repeated_lines_are_aligned_minimally() {
    let old = "\n\n}\n\n}\n";
    let new = "\n}\n\n\n}\n\n";
    let stat = diff_stat(old, new);
    let common = lcs_len(&split_lines(old), &split_lines(new));
    assert_eq!(stat.removed, 5 - common);
    assert_eq!(stat.added, 6 - common);
}

#[test]
fn whole_file_rewrite() {
    let hunks = diff_lines("a\nb\n", "c\nd\ne\n", 3);
    assert_eq!(hunks.len(), 1);
    assert_eq!(
        hunks[0].lines,
        vec![del("a"), del("b"), add("c"), add("d"), add("e")]
    );
    assert_eq!(
        diff_stat("a\nb\n", "c\nd\ne\n"),
        DiffStat {
            added: 3,
            removed: 2
        }
    );
}

// -------------------------------------------------------------- unified_diff

#[test]
fn unified_diff_gnu_format() {
    let old = numbered(1..=9);
    let new = old.replace("5\n", "five\n");
    let expected = "--- a/f.txt\n+++ b/f.txt\n@@ -2,7 +2,7 @@\n 2\n 3\n 4\n-5\n+five\n 6\n 7\n 8\n";
    assert_eq!(unified_diff("f.txt", "f.txt", &old, &new, 3), expected);
}

#[test]
fn unified_diff_single_line_ranges_omit_length() {
    let patch = unified_diff("f", "f", "a\nb\nc\n", "a\nB\nc\n", 0);
    assert_eq!(patch, "--- a/f\n+++ b/f\n@@ -2 +2 @@\n-b\n+B\n");
}

#[test]
fn unified_diff_added_file() {
    let patch = unified_diff("new.rs", "new.rs", "", "x\ny\n", 3);
    assert_eq!(
        patch,
        "--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1,2 @@\n+x\n+y\n"
    );
}

#[test]
fn unified_diff_deleted_file() {
    let patch = unified_diff("old.rs", "old.rs", "x\n", "", 3);
    assert_eq!(patch, "--- a/old.rs\n+++ /dev/null\n@@ -1 +0,0 @@\n-x\n");
}

#[test]
fn unified_diff_explicit_dev_null_and_renames() {
    let patch = unified_diff("/dev/null", "b.txt", "", "x\n", 3);
    assert!(patch.starts_with("--- /dev/null\n+++ b/b.txt\n"));
    let renamed = unified_diff("old/a.txt", "new/a.txt", "1\n", "2\n", 3);
    assert!(renamed.starts_with("--- a/old/a.txt\n+++ b/new/a.txt\n"));
}

#[test]
fn unified_diff_no_newline_on_old_side() {
    let patch = unified_diff("f", "f", "a\nb", "a\nc\n", 3);
    assert_eq!(
        patch,
        "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n"
    );
}

#[test]
fn unified_diff_no_newline_on_new_side() {
    let patch = unified_diff("f", "f", "a\nb\n", "a\nc", 3);
    assert_eq!(
        patch,
        "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n\\ No newline at end of file\n"
    );
}

#[test]
fn unified_diff_no_newline_on_both_sides() {
    let patch = unified_diff("f", "f", "a\nb", "a\nc", 3);
    assert_eq!(
        patch,
        "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+c\n\\ No newline at end of file\n"
    );
}

#[test]
fn unified_diff_no_newline_on_context_line() {
    let patch = unified_diff("f", "f", "a\nb\nc", "A\nb\nc", 3);
    assert_eq!(
        patch,
        "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n-a\n+A\n b\n c\n\\ No newline at end of file\n"
    );
}

#[test]
fn unified_diff_only_trailing_newline_added() {
    let patch = unified_diff("f", "f", "a", "a\n", 3);
    assert_eq!(
        patch,
        "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n\\ No newline at end of file\n+a\n"
    );
    assert_eq!(apply_patch("a", &patch).as_deref(), Ok("a\n"));
}

#[test]
fn unified_diff_marker_absent_when_last_line_outside_hunk() {
    let old = format!("{}last", numbered(1..=10));
    let new = old.replacen("2\n", "two\n", 1);
    let patch = unified_diff("f", "f", &old, &new, 1);
    assert!(!patch.contains("No newline"), "{patch}");
}

#[test]
fn unified_diff_preserves_crlf_bytes() {
    let patch = unified_diff("f", "f", "a\r\nb\r\n", "a\r\nc\r\n", 3);
    assert_eq!(
        patch,
        "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\r\n-b\r\n+c\r\n"
    );
    assert_eq!(
        apply_patch("a\r\nb\r\n", &patch).as_deref(),
        Ok("a\r\nc\r\n")
    );
}

#[test]
fn unified_diff_multiple_hunks_apply() {
    let old = numbered(1..=40);
    let new = edited(40, &[(3, ""), (20, "20\n20.5\n"), (39, "XXXIX\n")]);
    let patch = unified_diff("f", "f", &old, &new, 3);
    assert_eq!(patch.matches("@@ -").count(), 3);
    assert_eq!(apply_patch(&old, &patch), Ok(new));
}

// ---------------------------------------------------------------- diff_stat

#[test]
fn diff_stat_counts() {
    assert_eq!(
        diff_stat("", "a\nb\n"),
        DiffStat {
            added: 2,
            removed: 0
        }
    );
    assert_eq!(
        diff_stat("a\nb\n", ""),
        DiffStat {
            added: 0,
            removed: 2
        }
    );
    assert_eq!(
        diff_stat("a\nb\nc\n", "a\nx\nc\nd\n"),
        DiffStat {
            added: 2,
            removed: 1
        }
    );
    assert_eq!(
        diff_stat("a", "a\n"),
        DiffStat {
            added: 1,
            removed: 1
        }
    );
}

// ---------------------------------------------------------------- is_binary

#[test]
fn is_binary_detects_nul_and_invalid_utf8() {
    assert!(!is_binary(b""));
    assert!(!is_binary(b"plain text\n"));
    assert!(!is_binary("h\u{e9}llo \u{1F600}\n".as_bytes()));
    assert!(is_binary(b"abc\0def"));
    assert!(is_binary(&[0xff, 0xfe, b'a']));
    assert!(is_binary(b"valid start \xc3"));
}

#[test]
fn is_binary_nul_window_is_8_kib() {
    let mut data = vec![b'a'; 10_000];
    data[8191] = 0;
    assert!(is_binary(&data));
    data[8191] = b'a';
    // NUL is valid UTF-8, so past the sniff window it is not binary.
    data[8192] = 0;
    assert!(!is_binary(&data));
}

// -------------------------------------------------------------- properties

#[test]
fn property_unified_diff_round_trips() {
    let mut rng = Rng::new(1);
    for _ in 0..2_000 {
        let old = rng.text(12);
        let new = if rng.chance(50) {
            rng.mutate(&old)
        } else {
            rng.text(12)
        };
        for context in 0..4 {
            let patch = unified_diff("f", "f", &old, &new, context);
            assert_eq!(
                apply_patch(&old, &patch).as_ref(),
                Ok(&new),
                "old={old:?} new={new:?} ctx={context}\n{patch}"
            );
        }
    }
}

#[test]
fn property_matching_is_optimal_and_valid() {
    let mut rng = Rng::new(2);
    for _ in 0..2_000 {
        let (old, new) = (rng.text(15), rng.text(15));
        let (a, b) = (split_lines(&old), split_lines(&new));
        let pairs = match_lines(&a, &b);
        assert!(pairs.windows(2).all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1));
        assert!(pairs.iter().all(|&(i, j)| a[i] == b[j]));
        assert_eq!(pairs.len(), lcs_len(&a, &b), "old={old:?} new={new:?}");
    }
}

#[test]
fn property_hunks_are_consistent() {
    let mut rng = Rng::new(3);
    for _ in 0..1_000 {
        let old = rng.text(20);
        let new = rng.mutate(&old);
        let context = rng.below(4);
        let hunks = diff_lines(&old, &new, context);
        let stat = diff_stat(&old, &new);
        let (mut added, mut removed) = (0, 0);
        let mut prev_old_end = 0;
        for hunk in &hunks {
            let olds = hunk
                .lines
                .iter()
                .filter(|l| !matches!(l, DiffLine::Added(_)))
                .count();
            let news = hunk
                .lines
                .iter()
                .filter(|l| !matches!(l, DiffLine::Removed(_)))
                .count();
            assert_eq!((olds, news), (hunk.old_len, hunk.new_len));
            assert!(hunk
                .lines
                .iter()
                .any(|l| !matches!(l, DiffLine::Context(_))));
            let begin = zero_based_start(hunk.old_start, hunk.old_len);
            assert!(begin >= prev_old_end);
            prev_old_end = begin + hunk.old_len;
            added += hunk
                .lines
                .iter()
                .filter(|l| matches!(l, DiffLine::Added(_)))
                .count();
            removed += hunk
                .lines
                .iter()
                .filter(|l| matches!(l, DiffLine::Removed(_)))
                .count();
        }
        assert_eq!(DiffStat { added, removed }, stat);
        assert_eq!(hunks.is_empty(), old == new);
    }
}

#[test]
fn property_context_lines_bounded() {
    let mut rng = Rng::new(4);
    for _ in 0..500 {
        let old = rng.text(30);
        let new = rng.mutate(&old);
        for hunk in diff_lines(&old, &new, 2) {
            let lead = hunk
                .lines
                .iter()
                .take_while(|l| matches!(l, DiffLine::Context(_)))
                .count();
            let trail = hunk
                .lines
                .iter()
                .rev()
                .take_while(|l| matches!(l, DiffLine::Context(_)))
                .count();
            assert!(lead <= 2 && trail <= 2, "{hunk:?}");
        }
    }
}

// ------------------------------------------------------------- performance

#[test]
fn large_file_with_scattered_edits_is_fast() {
    let old: String = (0..50_000).map(|i| format!("line {i}\n")).collect();
    let mut new = old.clone();
    for i in (0..50_000).step_by(997) {
        new = new.replacen(&format!("line {i}\n"), &format!("changed {i}\n"), 1);
    }
    let start = std::time::Instant::now();
    let stat = diff_stat(&old, &new);
    assert_eq!(
        stat,
        DiffStat {
            added: 51,
            removed: 51
        }
    );
    assert!(start.elapsed().as_secs() < 5);
}

#[test]
fn completely_different_large_files_are_fast() {
    let old: String = (0..100_000).map(|i| format!("old {i}\n")).collect();
    let new: String = (0..100_000).map(|i| format!("new {i}\n")).collect();
    let start = std::time::Instant::now();
    assert_eq!(
        diff_stat(&old, &new),
        DiffStat {
            added: 100_000,
            removed: 100_000
        }
    );
    assert!(start.elapsed().as_secs() < 5);
}

#[test]
fn pathological_input_hits_cost_limit_but_stays_valid() {
    // Many edits over a tiny shared alphabet: no lines can be discarded.
    let mut rng = Rng::new(5);
    let old: String = (0..20_000)
        .map(|_| ["a\n", "b\n", "c\n"][rng.below(3)])
        .collect();
    let new: String = (0..20_000)
        .map(|_| ["a\n", "b\n", "c\n"][rng.below(3)])
        .collect();
    let start = std::time::Instant::now();
    let patch = unified_diff("f", "f", &old, &new, 3);
    assert!(start.elapsed().as_secs() < 20);
    assert_eq!(apply_patch(&old, &patch), Ok(new));
}
