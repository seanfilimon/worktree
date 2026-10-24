use super::*;
use crate::diff::tests::Rng;
use crate::store::ObjectHash;

fn merge(base: &str, ours: &str, theirs: &str) -> TextMergeResult {
    merge_text(base, ours, theirs, &MergeLabels::default())
}

fn clean(result: TextMergeResult) -> String {
    match result {
        TextMergeResult::Clean(text) => text,
        other => panic!("expected clean merge, got {other:?}"),
    }
}

fn conflicted(result: TextMergeResult) -> (String, Vec<ConflictRegion>) {
    match result {
        TextMergeResult::Conflicted { merged, conflicts } => (merged, conflicts),
        other => panic!("expected conflicts, got {other:?}"),
    }
}

fn strings(lines: &[&str]) -> Vec<String> {
    lines.iter().map(|s| (*s).to_owned()).collect()
}

/// Lines `1..=n` with the listed line numbers replaced by the given text.
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

// ---------------------------------------------------------------- merge_text

#[test]
fn identical_inputs_merge_cleanly() {
    assert_eq!(clean(merge("a\nb\n", "a\nb\n", "a\nb\n")), "a\nb\n");
    assert_eq!(clean(merge("", "", "")), "");
}

#[test]
fn one_side_changes_are_taken() {
    assert_eq!(
        clean(merge("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n")),
        "a\nB\nc\n"
    );
    assert_eq!(
        clean(merge("a\nb\nc\n", "a\nb\nc\n", "a\nb\nC\n")),
        "a\nb\nC\n"
    );
}

#[test]
fn identical_changes_are_taken_once() {
    assert_eq!(
        clean(merge("a\nb\nc\n", "a\nX\nc\nend\n", "a\nX\nc\nend\n")),
        "a\nX\nc\nend\n"
    );
}

#[test]
fn non_overlapping_changes_combine() {
    let base = edited(10, &[]);
    let ours = edited(10, &[(2, "two\n")]);
    let theirs = edited(10, &[(7, "seven\n")]);
    assert_eq!(
        clean(merge(&base, &ours, &theirs)),
        edited(10, &[(2, "two\n"), (7, "seven\n")])
    );
}

#[test]
fn insertions_and_deletions_combine() {
    let base = edited(10, &[]);
    let ours = edited(10, &[(1, "0\n1\n"), (5, "")]);
    let theirs = edited(10, &[(8, "8\n8.5\n"), (10, "")]);
    assert_eq!(
        clean(merge(&base, &ours, &theirs)),
        edited(10, &[(1, "0\n1\n"), (5, ""), (8, "8\n8.5\n"), (10, "")])
    );
}

#[test]
fn identical_change_on_both_sides_plus_independent_one() {
    let base = edited(9, &[]);
    let ours = edited(9, &[(2, "X\n"), (8, "ours8\n")]);
    let theirs = edited(9, &[(2, "X\n")]);
    assert_eq!(clean(merge(&base, &ours, &theirs)), ours);
}

#[test]
fn overlapping_changes_conflict_with_diff3_markers() {
    let (merged, conflicts) = conflicted(merge("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n"));
    assert_eq!(
        merged,
        "a\n<<<<<<< ours\nours\n||||||| base\nb\n=======\ntheirs\n>>>>>>> theirs\nc\n"
    );
    assert_eq!(
        conflicts,
        vec![ConflictRegion {
            start_line: 2,
            ours: strings(&["ours"]),
            base: strings(&["b"]),
            theirs: strings(&["theirs"]),
        }]
    );
}

#[test]
fn conflict_at_file_start_has_start_line_one() {
    let (merged, conflicts) = conflicted(merge("a\nz\n", "A\nz\n", "AA\nz\n"));
    assert!(merged.starts_with("<<<<<<< ours\n"));
    assert_eq!(conflicts[0].start_line, 1);
}

#[test]
fn multiple_conflicts_report_their_output_lines() {
    let base = edited(12, &[]);
    let ours = edited(12, &[(3, "o3\n"), (10, "o10a\no10b\n")]);
    let theirs = edited(12, &[(3, "t3\n"), (6, "t6\n"), (10, "t10\n")]);
    let (merged, conflicts) = conflicted(merge(&base, &ours, &theirs));
    assert_eq!(conflicts.len(), 2);
    let lines: Vec<&str> = merged.lines().collect();
    for conflict in &conflicts {
        assert!(
            lines[conflict.start_line - 1].starts_with("<<<<<<< "),
            "{merged}"
        );
    }
    assert_eq!(conflicts[0].start_line, 3);
    // 2 lines + 7-line conflict + lines 4..=9 (6 lines, with theirs' 6) = 15.
    assert_eq!(conflicts[1].start_line, 16);
    assert_eq!(conflicts[1].ours, strings(&["o10a", "o10b"]));
    assert!(merged.contains("t6\n"));
}

#[test]
fn custom_labels_appear_in_markers() {
    let labels = MergeLabels {
        ours: "main".into(),
        base: "merge-base".into(),
        theirs: "feature/x".into(),
    };
    let (merged, _) = conflicted(merge_text("b\n", "o\n", "t\n", &labels));
    assert_eq!(
        merged,
        "<<<<<<< main\no\n||||||| merge-base\nb\n=======\nt\n>>>>>>> feature/x\n"
    );
}

#[test]
fn empty_labels_give_bare_markers() {
    let labels = MergeLabels {
        ours: String::new(),
        base: String::new(),
        theirs: String::new(),
    };
    let (merged, _) = conflicted(merge_text("b\n", "o\n", "t\n", &labels));
    assert_eq!(merged, "<<<<<<<\no\n|||||||\nb\n=======\nt\n>>>>>>>\n");
}

#[test]
fn adjacent_edits_conflict() {
    let (merged, conflicts) =
        conflicted(merge("1\n2\n3\n4\n", "1\nTWO\n3\n4\n", "1\n2\nTHREE\n4\n"));
    assert_eq!(
        merged,
        "1\n<<<<<<< ours\nTWO\n3\n||||||| base\n2\n3\n=======\n2\nTHREE\n>>>>>>> theirs\n4\n"
    );
    assert_eq!(conflicts[0].base, strings(&["2", "3"]));
}

#[test]
fn edits_separated_by_one_stable_line_merge() {
    assert_eq!(
        clean(merge(
            "1\n2\n3\n4\n5\n",
            "1\nTWO\n3\n4\n5\n",
            "1\n2\n3\nFOUR\n5\n"
        )),
        "1\nTWO\n3\nFOUR\n5\n"
    );
}

#[test]
fn insertions_at_same_point_conflict_with_empty_base() {
    let (merged, conflicts) = conflicted(merge("a\nb\n", "a\nours\nb\n", "a\ntheirs\nb\n"));
    assert_eq!(
        merged,
        "a\n<<<<<<< ours\nours\n||||||| base\n=======\ntheirs\n>>>>>>> theirs\nb\n"
    );
    assert!(conflicts[0].base.is_empty());
}

#[test]
fn add_add_two_way_merge() {
    assert_eq!(clean(merge("", "same\n", "same\n")), "same\n");
    let (merged, conflicts) = conflicted(merge("", "x\n", "y\n"));
    assert_eq!(
        merged,
        "<<<<<<< ours\nx\n||||||| base\n=======\ny\n>>>>>>> theirs\n"
    );
    assert_eq!(conflicts[0].start_line, 1);
}

#[test]
fn add_add_moves_common_lines_out_of_markers() {
    let (merged, conflicts) = conflicted(merge("", "head\nmine\ntail\n", "head\nyours\ntail\n"));
    assert_eq!(
        merged,
        "head\n<<<<<<< ours\nmine\n||||||| base\n=======\nyours\n>>>>>>> theirs\ntail\n"
    );
    assert_eq!(conflicts[0].start_line, 2);
}

#[test]
fn conflict_trimming_keeps_full_base() {
    let (merged, conflicts) = conflicted(merge(
        "a\nold\nz\n",
        "a\nkeep\nours\nz\n",
        "a\nkeep\ntheirs\nz\n",
    ));
    assert_eq!(
        merged,
        "a\nkeep\n<<<<<<< ours\nours\n||||||| base\nold\n=======\ntheirs\n>>>>>>> theirs\nz\n"
    );
    assert_eq!(conflicts[0].base, strings(&["old"]));
    assert_eq!(conflicts[0].start_line, 3);
}

#[test]
fn deleting_everything_on_one_side() {
    assert_eq!(clean(merge("a\nb\n", "", "a\nb\n")), "");
    assert_eq!(clean(merge("a\nb\n", "a\nb\n", "")), "");
    let (merged, conflicts) = conflicted(merge("a\nb\n", "", "a\nB\n"));
    assert_eq!(
        merged,
        "<<<<<<< ours\n||||||| base\na\nb\n=======\na\nB\n>>>>>>> theirs\n"
    );
    assert!(conflicts[0].ours.is_empty());
}

#[test]
fn both_delete_same_lines() {
    assert_eq!(clean(merge("a\nb\nc\n", "a\nc\n", "a\nc\n")), "a\nc\n");
}

#[test]
fn missing_trailing_newline_is_preserved() {
    assert_eq!(clean(merge("a\nb", "A\nb", "a\nb")), "A\nb");
    assert_eq!(clean(merge("a\nm\nb", "x\nm\nb", "a\nm\nb\n")), "x\nm\nb\n");
    assert_eq!(clean(merge("a\nb\n", "a\nb\n", "a\nb")), "a\nb");
}

#[test]
fn conflict_on_unterminated_last_line_gets_own_marker_lines() {
    let (merged, conflicts) = conflicted(merge("a\nb", "a\nours", "a\ntheirs"));
    assert_eq!(
        merged,
        "a\n<<<<<<< ours\nours\n||||||| base\nb\n=======\ntheirs\n>>>>>>> theirs\n"
    );
    assert_eq!(conflicts[0].ours, strings(&["ours"]));
    assert_eq!(conflicts[0].base, strings(&["b"]));
}

#[test]
fn crlf_files_get_crlf_markers() {
    let (merged, conflicts) = conflicted(merge("a\r\nb\r\n", "a\r\no\r\n", "a\r\nt\r\n"));
    assert_eq!(
        merged,
        "a\r\n<<<<<<< ours\r\no\r\n||||||| base\r\nb\r\n=======\r\nt\r\n>>>>>>> theirs\r\n"
    );
    assert_eq!(conflicts[0].ours, strings(&["o\r"]));
}

#[test]
fn crlf_content_is_merged_byte_exact() {
    let merged = clean(merge(
        "1\r\n2\r\n3\r\n4\r\n5\r\n",
        "1\r\nI\r\n3\r\n4\r\n5\r\n",
        "1\r\n2\r\n3\r\n4\r\nV\r\n",
    ));
    assert_eq!(merged, "1\r\nI\r\n3\r\n4\r\nV\r\n");
}

#[test]
fn line_ending_change_conflicts_with_content_change() {
    let (_, conflicts) = conflicted(merge("a\nb\n", "a\r\nb\r\n", "a\nB\n"));
    assert_eq!(conflicts.len(), 1);
}

#[test]
fn moved_block_and_edit_elsewhere() {
    let base = edited(12, &[]);
    // Ours moves lines 2-3 to the end; theirs edits line 8.
    let ours = format!("{}2\n3\n", edited(12, &[(2, ""), (3, "")]));
    let theirs = edited(12, &[(8, "eight\n")]);
    assert_eq!(
        clean(merge(&base, &ours, &theirs)),
        format!("{}2\n3\n", edited(12, &[(2, ""), (3, ""), (8, "eight\n")]))
    );
}

// ------------------------------------------------------ merge_text properties

#[test]
fn property_one_sided_and_identical_merges() {
    let mut rng = Rng::new(10);
    for _ in 0..2_000 {
        let base = rng.text(12);
        let x = if rng.chance(50) {
            rng.mutate(&base)
        } else {
            rng.text(12)
        };
        assert_eq!(clean(merge(&base, &x, &base)), x, "base={base:?} x={x:?}");
        assert_eq!(clean(merge(&base, &base, &x)), x, "base={base:?} x={x:?}");
        assert_eq!(clean(merge(&base, &x, &x)), x, "base={base:?} x={x:?}");
    }
}

#[test]
fn property_clean_merges_are_symmetric() {
    let mut rng = Rng::new(11);
    let mut clean_count = 0;
    for _ in 0..2_000 {
        let base = rng.text(15);
        let (x, y) = (rng.mutate(&base), rng.mutate(&base));
        match (merge(&base, &x, &y), merge(&base, &y, &x)) {
            (TextMergeResult::Clean(a), TextMergeResult::Clean(b)) => {
                assert_eq!(a, b);
                clean_count += 1;
            }
            (
                TextMergeResult::Conflicted { conflicts: a, .. },
                TextMergeResult::Conflicted { conflicts: b, .. },
            ) => {
                assert_eq!(a.len(), b.len());
                for (ca, cb) in a.iter().zip(&b) {
                    assert_eq!(
                        (&ca.ours, &ca.base, &ca.theirs),
                        (&cb.theirs, &cb.base, &cb.ours)
                    );
                }
            }
            (a, b) => panic!("asymmetric: {a:?} vs {b:?}"),
        }
    }
    assert!(clean_count > 100, "generator should produce clean merges");
}

#[test]
fn property_conflict_regions_match_markers() {
    let mut rng = Rng::new(12);
    for _ in 0..2_000 {
        let base = rng.text(15);
        let (x, y) = (rng.mutate(&base), rng.mutate(&base));
        let TextMergeResult::Conflicted { merged, conflicts } = merge(&base, &x, &y) else {
            continue;
        };
        // Lines are compared without a trailing CR: an unterminated last line
        // inside markers is terminated with the CRLF marker line ending.
        let lines: Vec<&str> = merged
            .split_inclusive('\n')
            .map(|l| l.trim_end_matches('\n'))
            .collect();
        let is_marker = |line: &str, marker: &str| line.trim_end_matches('\r') == marker;
        assert_eq!(
            lines.iter().filter(|l| l.starts_with("<<<<<<<")).count(),
            conflicts.len()
        );
        for c in &conflicts {
            let mut at = c.start_line - 1;
            assert!(is_marker(lines[at], "<<<<<<< ours"), "{merged}");
            let mut section = |end: &str| {
                at += 1;
                let start = at;
                while !is_marker(lines[at], end) {
                    at += 1;
                }
                lines[start..at]
                    .iter()
                    .map(|l| l.trim_end_matches('\r'))
                    .collect::<Vec<_>>()
            };
            let trimmed = |v: &[String]| {
                v.iter()
                    .map(|l| l.trim_end_matches('\r').to_owned())
                    .collect::<Vec<_>>()
            };
            assert_eq!(section("||||||| base"), trimmed(&c.ours));
            assert_eq!(section("======="), trimmed(&c.base));
            assert_eq!(section(">>>>>>> theirs"), trimmed(&c.theirs));
        }
    }
}

#[test]
fn property_disjoint_edits_merge_cleanly() {
    let mut rng = Rng::new(13);
    for _ in 0..500 {
        // Unique lines; ours edits the first half, theirs the second, with a
        // stable separator line between the halves.
        let n = 4 + rng.below(20);
        let mid = n / 2;
        let base = edited(n, &[]);
        let a = 1 + rng.below(mid - 1);
        let b = mid + 2 + rng.below(n - mid - 1);
        let ours = edited(n, &[(a, "ours\n")]);
        let theirs = edited(n, &[(b, "theirs\n")]);
        assert_eq!(
            clean(merge(&base, &ours, &theirs)),
            edited(n, &[(a, "ours\n"), (b, "theirs\n")])
        );
    }
}

#[test]
fn large_merge_is_fast() {
    let base: String = (0..50_000).map(|i| format!("line {i}\n")).collect();
    let ours = base.replacen("line 100\n", "ours 100\n", 1);
    let theirs = base.replacen("line 40000\n", "theirs 40000\n", 1);
    let start = std::time::Instant::now();
    let merged = clean(merge(&base, &ours, &theirs));
    assert!(start.elapsed().as_secs() < 5);
    assert!(merged.contains("ours 100\n") && merged.contains("theirs 40000\n"));
    assert_eq!(merged.len(), base.len() + 2);
}

// ----------------------------------------------------------- merge_manifests

fn entry(path: &str, content: u8, executable: bool) -> ManifestEntry {
    ManifestEntry {
        path: path.to_owned(),
        hash: ObjectHash::from_bytes([content; 32]),
        size: u64::from(content),
        executable,
    }
}

fn manifest(entries: Vec<ManifestEntry>) -> Manifest {
    Manifest { entries }
}

/// One side of a single-path manifest: `(content byte, executable)` if present.
type Side = Option<(u8, bool)>;

/// Merges single-path manifests given as (content, executable) options.
fn resolve_one(base: Side, ours: Side, theirs: Side) -> Option<PathResolution> {
    let side = |s: Side| manifest(s.map(|(c, x)| entry("f", c, x)).into_iter().collect());
    let mut out = merge_manifests(&side(base), &side(ours), &side(theirs));
    assert!(out.len() <= 1);
    out.pop().map(|(path, resolution)| {
        assert_eq!(path, "f");
        resolution
    })
}

#[test]
fn manifest_resolution_table() {
    use PathResolution::*;
    let f = |c: u8| entry("f", c, false);
    let cases: Vec<(&str, Side, Side, Side, Option<PathResolution>)> = vec![
        (
            "unchanged",
            Some((1, false)),
            Some((1, false)),
            Some((1, false)),
            Some(KeepOurs(f(1))),
        ),
        (
            "ours modified",
            Some((1, false)),
            Some((2, false)),
            Some((1, false)),
            Some(KeepOurs(f(2))),
        ),
        (
            "theirs modified",
            Some((1, false)),
            Some((1, false)),
            Some((3, false)),
            Some(TakeTheirs(f(3))),
        ),
        (
            "both same modify",
            Some((1, false)),
            Some((4, false)),
            Some((4, false)),
            Some(KeepOurs(f(4))),
        ),
        (
            "both modified",
            Some((1, false)),
            Some((2, false)),
            Some((3, false)),
            Some(MergeContent {
                base: Some(f(1)),
                ours: f(2),
                theirs: f(3),
            }),
        ),
        (
            "ours deleted",
            Some((1, false)),
            None,
            Some((1, false)),
            Some(Delete),
        ),
        (
            "theirs deleted",
            Some((1, false)),
            Some((1, false)),
            None,
            Some(Delete),
        ),
        ("both deleted", Some((1, false)), None, None, None),
        (
            "ours deleted, theirs modified",
            Some((1, false)),
            None,
            Some((3, false)),
            Some(Conflict {
                kind: ConflictKind::ModifyDelete,
                base: Some(f(1)),
                ours: None,
                theirs: Some(f(3)),
            }),
        ),
        (
            "ours modified, theirs deleted",
            Some((1, false)),
            Some((2, false)),
            None,
            Some(Conflict {
                kind: ConflictKind::ModifyDelete,
                base: Some(f(1)),
                ours: Some(f(2)),
                theirs: None,
            }),
        ),
        (
            "ours added",
            None,
            Some((2, false)),
            None,
            Some(KeepOurs(f(2))),
        ),
        (
            "theirs added",
            None,
            None,
            Some((3, false)),
            Some(TakeTheirs(f(3))),
        ),
        (
            "add/add identical",
            None,
            Some((5, false)),
            Some((5, false)),
            Some(KeepOurs(f(5))),
        ),
        (
            "add/add different",
            None,
            Some((2, false)),
            Some((3, false)),
            Some(MergeContent {
                base: None,
                ours: f(2),
                theirs: f(3),
            }),
        ),
        (
            "add/add same hash, different mode",
            None,
            Some((5, true)),
            Some((5, false)),
            Some(KeepOurs(entry("f", 5, true))),
        ),
        ("absent everywhere", None, None, None, None),
    ];
    for (name, base, ours, theirs, expected) in cases {
        assert_eq!(resolve_one(base, ours, theirs), expected, "case: {name}");
    }
}

#[test]
fn manifest_executable_bit_merges_independently() {
    use PathResolution::*;
    // Theirs toggles the bit, ours edits content: ours' content, theirs' bit.
    assert_eq!(
        resolve_one(Some((1, false)), Some((2, false)), Some((1, true))),
        Some(KeepOurs(entry("f", 2, true)))
    );
    // Ours toggles the bit, theirs edits content.
    assert_eq!(
        resolve_one(Some((1, false)), Some((1, true)), Some((3, false))),
        Some(TakeTheirs(entry("f", 3, true)))
    );
    // Same new content on both sides, only theirs toggled the bit.
    assert_eq!(
        resolve_one(Some((1, false)), Some((2, false)), Some((2, true))),
        Some(KeepOurs(entry("f", 2, true)))
    );
    // Only one side toggled the bit.
    assert_eq!(
        resolve_one(Some((1, true)), Some((1, true)), Some((1, false))),
        Some(TakeTheirs(entry("f", 1, false)))
    );
    // Mode change vs. delete is a modify/delete conflict.
    assert!(matches!(
        resolve_one(Some((1, false)), Some((1, true)), None),
        Some(Conflict {
            kind: ConflictKind::ModifyDelete,
            ..
        })
    ));
    // Both changed content: mode is reported separately.
    assert!(matches!(
        resolve_one(Some((1, false)), Some((2, true)), Some((3, false))),
        Some(MergeContent { .. })
    ));
}

#[test]
fn merge_executable_rules() {
    let (no, yes) = (entry("f", 1, false), entry("f", 1, true));
    assert!(merge_executable(Some(&no), &no, &yes));
    assert!(merge_executable(Some(&no), &yes, &no));
    assert!(merge_executable(Some(&no), &yes, &yes));
    assert!(!merge_executable(Some(&yes), &no, &yes));
    assert!(!merge_executable(Some(&yes), &yes, &no));
    assert!(merge_executable(None, &yes, &no));
    assert!(!merge_executable(None, &no, &yes));
}

#[test]
fn manifest_merge_covers_many_paths_sorted() {
    use PathResolution::*;
    let base = manifest(vec![
        entry("src/lib.rs", 1, false),
        entry("README.md", 2, false),
        entry("gone.txt", 3, false),
        entry("old.txt", 4, false),
    ]);
    // Deliberately unsorted inputs.
    let ours = manifest(vec![
        entry("src/main.rs", 9, false),
        entry("src/lib.rs", 10, false),
        entry("README.md", 2, false),
    ]);
    let theirs = manifest(vec![
        entry("old.txt", 4, false),
        entry("README.md", 11, false),
        entry("src/lib.rs", 1, false),
        entry("Zeta", 12, true),
    ]);
    let merged = merge_manifests(&base, &ours, &theirs);
    let expected = vec![
        (
            "README.md".to_owned(),
            TakeTheirs(entry("README.md", 11, false)),
        ),
        ("Zeta".to_owned(), TakeTheirs(entry("Zeta", 12, true))),
        ("old.txt".to_owned(), Delete),
        (
            "src/lib.rs".to_owned(),
            KeepOurs(entry("src/lib.rs", 10, false)),
        ),
        (
            "src/main.rs".to_owned(),
            KeepOurs(entry("src/main.rs", 9, false)),
        ),
    ];
    assert_eq!(merged, expected);
}

#[test]
fn manifest_merge_of_empty_manifests_is_empty() {
    let empty = Manifest::default();
    assert!(merge_manifests(&empty, &empty, &empty).is_empty());
    let base = manifest(vec![entry("a", 1, false)]);
    assert!(merge_manifests(&base, &empty, &empty).is_empty());
}

#[test]
fn manifest_merge_identity_properties() {
    let mut rng = Rng::new(14);
    for _ in 0..300 {
        let random = |rng: &mut Rng| {
            let mut entries = Vec::new();
            for i in 0..8 {
                if rng.chance(60) {
                    entries.push(entry(
                        &format!("p{i}"),
                        1 + rng.below(3) as u8,
                        rng.chance(20),
                    ));
                }
            }
            manifest(entries)
        };
        let (base, x) = (random(&mut rng), random(&mut rng));
        for merged in [
            merge_manifests(&base, &x, &base),
            merge_manifests(&base, &base, &x),
            merge_manifests(&base, &x, &x),
        ] {
            let result: Vec<ManifestEntry> = merged
                .into_iter()
                .filter_map(|(_, r)| match r {
                    PathResolution::KeepOurs(e) | PathResolution::TakeTheirs(e) => Some(e),
                    PathResolution::Delete => None,
                    other => panic!("unexpected {other:?}"),
                })
                .collect();
            assert_eq!(result, x.entries);
        }
    }
}

// ---------------------------------------------------------------- merge_base

/// Builds a parent map from `(child, [parents])` pairs.
fn dag(edges: &[(&str, &[&str])]) -> HashMap<SnapshotId, Vec<SnapshotId>> {
    edges
        .iter()
        .map(|(id, ps)| {
            (
                (*id).to_owned(),
                ps.iter().map(|p| (*p).to_owned()).collect(),
            )
        })
        .collect()
}

fn base_of(a: &str, b: &str, parents: &HashMap<SnapshotId, Vec<SnapshotId>>) -> Option<String> {
    merge_base(a, b, parents)
}

#[test]
fn merge_base_linear_history() {
    let g = dag(&[
        ("c1", &[]),
        ("c2", &["c1"]),
        ("c3", &["c2"]),
        ("c4", &["c3"]),
    ]);
    assert_eq!(base_of("c4", "c2", &g).as_deref(), Some("c2"));
    assert_eq!(base_of("c2", "c4", &g).as_deref(), Some("c2"));
    assert_eq!(base_of("c3", "c3", &g).as_deref(), Some("c3"));
    assert_eq!(base_of("c1", "c4", &g).as_deref(), Some("c1"));
}

#[test]
fn merge_base_simple_fork() {
    let g = dag(&[
        ("r", &[]),
        ("m1", &["r"]),
        ("m2", &["m1"]),
        ("f1", &["m1"]),
        ("f2", &["f1"]),
    ]);
    assert_eq!(base_of("m2", "f2", &g).as_deref(), Some("m1"));
    assert_eq!(base_of("f2", "m2", &g).as_deref(), Some("m1"));
}

#[test]
fn merge_base_after_previous_merge() {
    // main: r - m1 - M - m3 ; feature: r - f1 - f2 ; M merged f1 into main.
    let g = dag(&[
        ("r", &[]),
        ("m1", &["r"]),
        ("f1", &["r"]),
        ("M", &["m1", "f1"]),
        ("m3", &["M"]),
        ("f2", &["f1"]),
    ]);
    // r is a common ancestor too, but it is an ancestor of f1.
    assert_eq!(base_of("m3", "f2", &g).as_deref(), Some("f1"));
    assert_eq!(base_of("f2", "m3", &g).as_deref(), Some("f1"));
}

#[test]
fn merge_base_criss_cross_breaks_ties_by_id() {
    let g = dag(&[
        ("r", &[]),
        ("x1", &["r"]),
        ("y1", &["r"]),
        ("mx", &["x1", "y1"]),
        ("my", &["y1", "x1"]),
        ("x2", &["mx"]),
        ("y2", &["my"]),
    ]);
    // x1 and y1 are both best common ancestors at distance 2 from either tip.
    assert_eq!(base_of("x2", "y2", &g).as_deref(), Some("x1"));
    assert_eq!(base_of("y2", "x2", &g).as_deref(), Some("x1"));
}

#[test]
fn merge_base_criss_cross_prefers_closer_to_a() {
    let g = dag(&[
        ("r", &[]),
        ("x1", &["r"]),
        ("y1", &["r"]),
        ("y0", &["y1"]),
        ("mx", &["x1", "y0"]),
        ("my", &["y0", "x1"]),
    ]);
    // Best common ancestors of mx and my: x1 (distance 1) and y0 (distance 1);
    // tie by id. Add a longer path to x1 from `a` to break by distance.
    assert_eq!(base_of("mx", "my", &g).as_deref(), Some("x1"));
    let g2 = dag(&[
        ("r", &[]),
        ("x1", &["r"]),
        ("y1", &["r"]),
        ("xa", &["x1"]),
        ("mx", &["xa", "y1"]),
        ("my", &["y1", "x1"]),
    ]);
    // From mx: y1 at distance 1, x1 at distance 2.
    assert_eq!(base_of("mx", "my", &g2).as_deref(), Some("y1"));
}

#[test]
fn merge_base_unrelated_histories() {
    let g = dag(&[("a1", &[]), ("a2", &["a1"]), ("b1", &[]), ("b2", &["b1"])]);
    assert_eq!(base_of("a2", "b2", &g), None);
}

#[test]
fn merge_base_unknown_ids_are_roots() {
    let g = dag(&[("c2", &["c1"])]);
    // c1 has no entry of its own but is still an ancestor.
    assert_eq!(base_of("c2", "c1", &g).as_deref(), Some("c1"));
    assert_eq!(base_of("ghost", "ghost", &g).as_deref(), Some("ghost"));
    assert_eq!(base_of("ghost", "c2", &g), None);
    assert_eq!(base_of("x", "y", &HashMap::new()), None);
}

#[test]
fn merge_base_with_joined_root_histories() {
    // Two independent roots joined by a merge; branches fork afterwards.
    let g = dag(&[
        ("ra", &[]),
        ("rb", &[]),
        ("j", &["ra", "rb"]),
        ("p", &["j"]),
        ("q", &["j"]),
        ("s", &["rb"]),
    ]);
    assert_eq!(base_of("p", "q", &g).as_deref(), Some("j"));
    assert_eq!(base_of("p", "s", &g).as_deref(), Some("rb"));
}

#[test]
fn merge_base_octopus_merge() {
    let g = dag(&[
        ("r", &[]),
        ("a", &["r"]),
        ("b", &["r"]),
        ("c", &["r"]),
        ("o", &["a", "b", "c"]),
        ("c2", &["c"]),
    ]);
    assert_eq!(base_of("o", "c2", &g).as_deref(), Some("c"));
    assert_eq!(base_of("o", "b", &g).as_deref(), Some("b"));
}

#[test]
fn merge_base_ignores_parent_order() {
    let g1 = dag(&[
        ("r", &[]),
        ("x", &["r"]),
        ("y", &["r"]),
        ("m", &["x", "y"]),
        ("t", &["y"]),
    ]);
    let g2 = dag(&[
        ("r", &[]),
        ("x", &["r"]),
        ("y", &["r"]),
        ("m", &["y", "x"]),
        ("t", &["y"]),
    ]);
    assert_eq!(base_of("m", "t", &g1).as_deref(), Some("y"));
    assert_eq!(base_of("m", "t", &g2).as_deref(), Some("y"));
}

#[test]
fn merge_base_deep_history_has_no_recursion_limit() {
    let n = 200_000;
    let mut g: HashMap<SnapshotId, Vec<SnapshotId>> = (1..n)
        .map(|i| (format!("c{i}"), vec![format!("c{}", i - 1)]))
        .collect();
    g.insert("c0".into(), vec![]);
    g.insert("side".into(), vec!["c10".into()]);
    assert_eq!(
        base_of(&format!("c{}", n - 1), "side", &g).as_deref(),
        Some("c10")
    );
}

#[test]
fn merge_base_brute_force_agreement() {
    // Random DAGs: compare with a direct reading of the definition.
    let mut rng = Rng::new(15);
    for _ in 0..200 {
        let n = 2 + rng.below(14);
        let ids: Vec<String> = (0..n).map(|i| format!("n{i:02}")).collect();
        let mut g: HashMap<SnapshotId, Vec<SnapshotId>> = HashMap::new();
        for i in 0..n {
            let parents = (0..i)
                .filter(|_| rng.chance(25))
                .map(|p| ids[p].clone())
                .take(3)
                .collect();
            g.insert(ids[i].clone(), parents);
        }
        let ancestors = |id: &str| -> HashMap<String, usize> {
            let mut dist = HashMap::from([(id.to_owned(), 0)]);
            let mut frontier = vec![id.to_owned()];
            let mut d = 0;
            while !frontier.is_empty() {
                d += 1;
                let mut next = Vec::new();
                for f in &frontier {
                    for p in &g[f] {
                        if !dist.contains_key(p) {
                            dist.insert(p.clone(), d);
                            next.push(p.clone());
                        }
                    }
                }
                frontier = next;
            }
            dist
        };
        let (a, b) = (&ids[rng.below(n)], &ids[rng.below(n)]);
        let (from_a, from_b) = (ancestors(a), ancestors(b));
        let common: Vec<String> = from_a
            .keys()
            .filter(|k| from_b.contains_key(*k))
            .cloned()
            .collect();
        let expected = common
            .iter()
            .filter(|c| {
                !common
                    .iter()
                    .any(|o| o != *c && ancestors(o).contains_key(c.as_str()))
            })
            .min_by_key(|c| (from_a[c.as_str()], (*c).clone()))
            .cloned();
        assert_eq!(merge_base(a, b, &g), expected, "a={a} b={b} g={g:?}");
    }
}
