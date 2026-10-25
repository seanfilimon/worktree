package merge3

import (
	"fmt"
	"math/rand"
	"reflect"
	"strings"
	"testing"
	"time"
)

// These cases mirror crates/worktree-sdk/src/merge/tests.rs so the server
// and the CLI merge text identically.

func merge(base, ours, theirs string) TextResult {
	return Text(base, ours, theirs, DefaultLabels())
}

func clean(t *testing.T, r TextResult) string {
	t.Helper()
	if !r.Clean() {
		t.Fatalf("expected a clean merge, got conflicts %+v:\n%s", r.Conflicts, r.Merged)
	}
	return r.Merged
}

func conflicted(t *testing.T, r TextResult) (string, []Region) {
	t.Helper()
	if r.Clean() {
		t.Fatalf("expected conflicts, got clean merge:\n%s", r.Merged)
	}
	return r.Merged, r.Conflicts
}

// edited returns lines 1..=n with the listed line numbers replaced.
func edited(n int, edits map[int]string) string {
	var sb strings.Builder
	for i := 1; i <= n; i++ {
		if text, ok := edits[i]; ok {
			sb.WriteString(text)
		} else {
			fmt.Fprintf(&sb, "%d\n", i)
		}
	}
	return sb.String()
}

func eq(t *testing.T, got, want any) {
	t.Helper()
	if !reflect.DeepEqual(got, want) {
		t.Fatalf("got  %#v\nwant %#v", got, want)
	}
}

func TestTextIdenticalAndOneSided(t *testing.T) {
	eq(t, clean(t, merge("a\nb\n", "a\nb\n", "a\nb\n")), "a\nb\n")
	eq(t, clean(t, merge("", "", "")), "")
	eq(t, clean(t, merge("a\nb\nc\n", "a\nB\nc\n", "a\nb\nc\n")), "a\nB\nc\n")
	eq(t, clean(t, merge("a\nb\nc\n", "a\nb\nc\n", "a\nb\nC\n")), "a\nb\nC\n")
	eq(t, clean(t, merge("a\nb\nc\n", "a\nX\nc\nend\n", "a\nX\nc\nend\n")), "a\nX\nc\nend\n")
}

func TestTextCombinesIndependentChanges(t *testing.T) {
	base := edited(10, nil)
	eq(t, clean(t, merge(base, edited(10, map[int]string{2: "two\n"}), edited(10, map[int]string{7: "seven\n"}))),
		edited(10, map[int]string{2: "two\n", 7: "seven\n"}))

	ours := edited(10, map[int]string{1: "0\n1\n", 5: ""})
	theirs := edited(10, map[int]string{8: "8\n8.5\n", 10: ""})
	eq(t, clean(t, merge(base, ours, theirs)), edited(10, map[int]string{1: "0\n1\n", 5: "", 8: "8\n8.5\n", 10: ""}))

	base = edited(9, nil)
	ours = edited(9, map[int]string{2: "X\n", 8: "ours8\n"})
	eq(t, clean(t, merge(base, ours, edited(9, map[int]string{2: "X\n"}))), ours)

	eq(t, clean(t, merge("1\n2\n3\n4\n5\n", "1\nTWO\n3\n4\n5\n", "1\n2\n3\nFOUR\n5\n")), "1\nTWO\n3\nFOUR\n5\n")
	eq(t, clean(t, merge("a\nb\nc\n", "a\nc\n", "a\nc\n")), "a\nc\n")
	eq(t, clean(t, merge("a\nb\n", "", "a\nb\n")), "")
	eq(t, clean(t, merge("a\nb\n", "a\nb\n", "")), "")
}

func TestTextOverlappingChangesConflict(t *testing.T) {
	merged, cs := conflicted(t, merge("a\nb\nc\n", "a\nours\nc\n", "a\ntheirs\nc\n"))
	eq(t, merged, "a\n<<<<<<< ours\nours\n||||||| base\nb\n=======\ntheirs\n>>>>>>> theirs\nc\n")
	eq(t, cs, []Region{{StartLine: 2, OursStart: 2, TheirsStart: 2, Ours: []string{"ours"}, Base: []string{"b"}, Theirs: []string{"theirs"}}})

	merged, cs = conflicted(t, merge("a\nz\n", "A\nz\n", "AA\nz\n"))
	if !strings.HasPrefix(merged, "<<<<<<< ours\n") || cs[0].StartLine != 1 {
		t.Fatalf("conflict at file start: %q %+v", merged, cs)
	}
}

func TestTextMultipleConflictsReportOutputLines(t *testing.T) {
	base := edited(12, nil)
	ours := edited(12, map[int]string{3: "o3\n", 10: "o10a\no10b\n"})
	theirs := edited(12, map[int]string{3: "t3\n", 6: "t6\n", 10: "t10\n"})
	merged, cs := conflicted(t, merge(base, ours, theirs))
	eq(t, len(cs), 2)
	lines := strings.Split(merged, "\n")
	for _, c := range cs {
		if !strings.HasPrefix(lines[c.StartLine-1], "<<<<<<< ") {
			t.Fatalf("line %d is not a marker:\n%s", c.StartLine, merged)
		}
	}
	eq(t, cs[0].StartLine, 3)
	eq(t, cs[1].StartLine, 16)
	eq(t, cs[1].Ours, []string{"o10a", "o10b"})
	if !strings.Contains(merged, "t6\n") {
		t.Fatal("theirs' independent edit is missing")
	}
	eq(t, [4]int{cs[0].OursStart, len(cs[0].Ours), cs[0].TheirsStart, len(cs[0].Theirs)}, [4]int{3, 1, 3, 1})
	eq(t, [4]int{cs[1].OursStart, len(cs[1].Ours), cs[1].TheirsStart, len(cs[1].Theirs)}, [4]int{10, 2, 10, 1})
}

func TestTextConflictPositions(t *testing.T) {
	_, cs := conflicted(t, merge("a\nb\nc\nd\n", "a\nx\ny\nb\nc\nOURS\n", "b\nc\nTHEIRS\n"))
	eq(t, len(cs), 2)
	last := cs[len(cs)-1]
	eq(t, [4]int{last.OursStart, len(last.Ours), last.TheirsStart, len(last.Theirs)}, [4]int{6, 1, 3, 1})

	// Ours deletes line 2, theirs edits it.
	_, cs = conflicted(t, merge("a\nb\nc\n", "a\nc\n", "a\nB\nc\n"))
	eq(t, [4]int{cs[0].OursStart, len(cs[0].Ours), cs[0].TheirsStart, len(cs[0].Theirs)}, [4]int{1, 0, 2, 1})
}

func TestTextLabels(t *testing.T) {
	merged, _ := conflicted(t, Text("b\n", "o\n", "t\n", Labels{Ours: "main", Base: "merge-base", Theirs: "feature/x"}))
	eq(t, merged, "<<<<<<< main\no\n||||||| merge-base\nb\n=======\nt\n>>>>>>> feature/x\n")
	merged, _ = conflicted(t, Text("b\n", "o\n", "t\n", Labels{}))
	eq(t, merged, "<<<<<<<\no\n|||||||\nb\n=======\nt\n>>>>>>>\n")
}

func TestTextAdjacentAndInsertions(t *testing.T) {
	merged, cs := conflicted(t, merge("1\n2\n3\n4\n", "1\nTWO\n3\n4\n", "1\n2\nTHREE\n4\n"))
	eq(t, merged, "1\n<<<<<<< ours\nTWO\n3\n||||||| base\n2\n3\n=======\n2\nTHREE\n>>>>>>> theirs\n4\n")
	eq(t, cs[0].Base, []string{"2", "3"})

	merged, cs = conflicted(t, merge("a\nb\n", "a\nours\nb\n", "a\ntheirs\nb\n"))
	eq(t, merged, "a\n<<<<<<< ours\nours\n||||||| base\n=======\ntheirs\n>>>>>>> theirs\nb\n")
	eq(t, len(cs[0].Base), 0)
}

func TestTextAddAdd(t *testing.T) {
	eq(t, clean(t, merge("", "same\n", "same\n")), "same\n")
	merged, cs := conflicted(t, merge("", "x\n", "y\n"))
	eq(t, merged, "<<<<<<< ours\nx\n||||||| base\n=======\ny\n>>>>>>> theirs\n")
	eq(t, cs[0].StartLine, 1)
	merged, cs = conflicted(t, merge("", "head\nmine\ntail\n", "head\nyours\ntail\n"))
	eq(t, merged, "head\n<<<<<<< ours\nmine\n||||||| base\n=======\nyours\n>>>>>>> theirs\ntail\n")
	eq(t, cs[0].StartLine, 2)
}

func TestTextTrimmingKeepsFullBase(t *testing.T) {
	merged, cs := conflicted(t, merge("a\nold\nz\n", "a\nkeep\nours\nz\n", "a\nkeep\ntheirs\nz\n"))
	eq(t, merged, "a\nkeep\n<<<<<<< ours\nours\n||||||| base\nold\n=======\ntheirs\n>>>>>>> theirs\nz\n")
	eq(t, cs[0].Base, []string{"old"})
	eq(t, cs[0].StartLine, 3)

	merged, cs = conflicted(t, merge("a\nb\n", "", "a\nB\n"))
	eq(t, merged, "<<<<<<< ours\n||||||| base\na\nb\n=======\na\nB\n>>>>>>> theirs\n")
	eq(t, len(cs[0].Ours), 0)
}

func TestTextNewlinesAndCRLF(t *testing.T) {
	eq(t, clean(t, merge("a\nb", "A\nb", "a\nb")), "A\nb")
	eq(t, clean(t, merge("a\nm\nb", "x\nm\nb", "a\nm\nb\n")), "x\nm\nb\n")
	eq(t, clean(t, merge("a\nb\n", "a\nb\n", "a\nb")), "a\nb")

	merged, cs := conflicted(t, merge("a\nb", "a\nours", "a\ntheirs"))
	eq(t, merged, "a\n<<<<<<< ours\nours\n||||||| base\nb\n=======\ntheirs\n>>>>>>> theirs\n")
	eq(t, cs[0].Ours, []string{"ours"})
	eq(t, cs[0].Base, []string{"b"})

	merged, cs = conflicted(t, merge("a\r\nb\r\n", "a\r\no\r\n", "a\r\nt\r\n"))
	eq(t, merged, "a\r\n<<<<<<< ours\r\no\r\n||||||| base\r\nb\r\n=======\r\nt\r\n>>>>>>> theirs\r\n")
	eq(t, cs[0].Ours, []string{"o\r"})

	eq(t, clean(t, merge("1\r\n2\r\n3\r\n4\r\n5\r\n", "1\r\nI\r\n3\r\n4\r\n5\r\n", "1\r\n2\r\n3\r\n4\r\nV\r\n")),
		"1\r\nI\r\n3\r\n4\r\nV\r\n")
	_, cs = conflicted(t, merge("a\nb\n", "a\r\nb\r\n", "a\nB\n"))
	eq(t, len(cs), 1)
}

func TestTextMovedBlockAndEditElsewhere(t *testing.T) {
	base := edited(12, nil)
	ours := edited(12, map[int]string{2: "", 3: ""}) + "2\n3\n"
	theirs := edited(12, map[int]string{8: "eight\n"})
	eq(t, clean(t, merge(base, ours, theirs)), edited(12, map[int]string{2: "", 3: "", 8: "eight\n"})+"2\n3\n")
}

// ---- properties ----

// rng generates small random texts over a tiny alphabet so lines repeat.
type rng struct{ *rand.Rand }

func newRng(seed int64) rng { return rng{rand.New(rand.NewSource(seed))} }

func (r rng) text(maxLines int) string {
	n := r.Intn(maxLines + 1)
	var sb strings.Builder
	for i := 0; i < n; i++ {
		sb.WriteString(string(rune('a' + r.Intn(5))))
		if i < n-1 || r.Intn(8) != 0 {
			sb.WriteByte('\n')
		}
	}
	return sb.String()
}

// mutate applies a few random line edits to text.
func (r rng) mutate(text string) string {
	lines := splitLines(text)
	for k := r.Intn(3) + 1; k > 0; k-- {
		switch op := r.Intn(3); {
		case op == 0 || len(lines) == 0: // insert
			at := r.Intn(len(lines) + 1)
			lines = append(lines[:at], append([]string{string(rune('p'+r.Intn(4))) + "\n"}, lines[at:]...)...)
		case op == 1: // delete
			at := r.Intn(len(lines))
			lines = append(lines[:at], lines[at+1:]...)
		default: // replace
			lines[r.Intn(len(lines))] = string(rune('v'+r.Intn(4))) + "\n"
		}
	}
	return strings.Join(lines, "")
}

func TestTextPropertyOneSidedAndIdentical(t *testing.T) {
	r := newRng(10)
	for i := 0; i < 2000; i++ {
		base := r.text(12)
		x := r.mutate(base)
		if got := clean(t, merge(base, x, base)); got != x {
			t.Fatalf("ours-only %q %q: %q", base, x, got)
		}
		if got := clean(t, merge(base, base, x)); got != x {
			t.Fatalf("theirs-only %q %q: %q", base, x, got)
		}
		if got := clean(t, merge(base, x, x)); got != x {
			t.Fatalf("identical %q %q: %q", base, x, got)
		}
	}
}

func TestTextPropertySymmetricAndMarkersMatch(t *testing.T) {
	r := newRng(11)
	swapped := Labels{Ours: "theirs", Base: "base", Theirs: "ours"}
	for i := 0; i < 3000; i++ {
		base := r.text(10)
		ours, theirs := r.mutate(base), r.mutate(base)
		a := merge(base, ours, theirs)
		b := Text(base, theirs, ours, swapped)
		if a.Clean() != b.Clean() || (a.Clean() && a.Merged != b.Merged) {
			t.Fatalf("asymmetric merge of %q / %q / %q:\n%q\n%q", base, ours, theirs, a.Merged, b.Merged)
		}
		lines := splitLines(a.Merged)
		for _, c := range a.Conflicts {
			if !strings.HasPrefix(lines[c.StartLine-1], "<<<<<<<") {
				t.Fatalf("region %+v does not start at a marker:\n%s", c, a.Merged)
			}
			if got := strings.TrimSuffix(lines[c.StartLine+len(c.Ours)], "\n"); !strings.HasPrefix(got, "|||||||") {
				t.Fatalf("base marker misplaced for %+v:\n%s", c, a.Merged)
			}
		}
		if strings.Count(a.Merged, "<<<<<<< ours") != len(a.Conflicts) {
			t.Fatalf("marker count mismatch:\n%s", a.Merged)
		}
	}
}

func TestTextPropertyDisjointEditsMergeCleanly(t *testing.T) {
	r := newRng(12)
	for i := 0; i < 1000; i++ {
		n := 6 + r.Intn(10)
		base := edited(n, nil)
		lo := 1 + r.Intn(n/2-1)
		hi := lo + 2 + r.Intn(n-lo-2) // at least one stable line in between
		ours := edited(n, map[int]string{lo: "ours\n"})
		theirs := edited(n, map[int]string{hi: "theirs\n"})
		want := edited(n, map[int]string{lo: "ours\n", hi: "theirs\n"})
		if got := clean(t, merge(base, ours, theirs)); got != want {
			t.Fatalf("n=%d lo=%d hi=%d: %q", n, lo, hi, got)
		}
	}
}

func TestTextLargeMergeIsFast(t *testing.T) {
	var base, ours, theirs strings.Builder
	for i := 0; i < 20000; i++ {
		fmt.Fprintf(&base, "line %d\n", i)
		switch {
		case i%1000 == 1:
			fmt.Fprintf(&ours, "ours %d\n", i)
			fmt.Fprintf(&theirs, "line %d\n", i)
		case i%1000 == 500:
			fmt.Fprintf(&ours, "line %d\n", i)
			fmt.Fprintf(&theirs, "theirs %d\n", i)
		default:
			fmt.Fprintf(&ours, "line %d\n", i)
			fmt.Fprintf(&theirs, "line %d\n", i)
		}
	}
	start := time.Now()
	got := clean(t, merge(base.String(), ours.String(), theirs.String()))
	if d := time.Since(start); d > 5*time.Second {
		t.Fatalf("merge took %v", d)
	}
	if !strings.Contains(got, "ours 1001\n") || !strings.Contains(got, "theirs 19500\n") {
		t.Fatal("changes missing from the merge")
	}
}

func TestMatchLinesIsACommonSubsequence(t *testing.T) {
	r := newRng(13)
	for i := 0; i < 2000; i++ {
		a, b := splitLines(r.text(15)), splitLines(r.text(15))
		ms := matchLines(a, b)
		pi, pj := -1, -1
		for _, m := range ms {
			if m[0] <= pi || m[1] <= pj || a[m[0]] != b[m[1]] {
				t.Fatalf("bad matching %v of %q / %q", ms, a, b)
			}
			pi, pj = m[0], m[1]
		}
		if want := lcsLen(a, b); len(ms) != want {
			t.Fatalf("matching of %q / %q has %d pairs, LCS is %d", a, b, len(ms), want)
		}
	}
}

func TestMatchLinesFallsBackOnHugeInputs(t *testing.T) {
	// Two long, mostly unrelated texts exceed the cost limit; the result must
	// still be a valid increasing matching.
	r := newRng(14)
	var a, b []string
	for i := 0; i < 6000; i++ {
		a = append(a, fmt.Sprintf("%d\n", r.Intn(50)))
		b = append(b, fmt.Sprintf("%d\n", r.Intn(50)))
	}
	ms := matchLines(a, b)
	pi, pj := -1, -1
	for _, m := range ms {
		if m[0] <= pi || m[1] <= pj || a[m[0]] != b[m[1]] {
			t.Fatalf("invalid matching at %v", m)
		}
		pi, pj = m[0], m[1]
	}
	if len(ms) == 0 {
		t.Fatal("no matches")
	}
}

func lcsLen(a, b []string) int {
	dp := make([][]int, len(a)+1)
	for i := range dp {
		dp[i] = make([]int, len(b)+1)
	}
	for i := len(a) - 1; i >= 0; i-- {
		for j := len(b) - 1; j >= 0; j-- {
			if a[i] == b[j] {
				dp[i][j] = dp[i+1][j+1] + 1
			} else {
				dp[i][j] = max(dp[i+1][j], dp[i][j+1])
			}
		}
	}
	return dp[0][0]
}

func TestSplitLines(t *testing.T) {
	eq(t, splitLines(""), []string(nil))
	eq(t, splitLines("a"), []string{"a"})
	eq(t, splitLines("a\n"), []string{"a\n"})
	eq(t, splitLines("a\nb"), []string{"a\n", "b"})
	eq(t, splitLines("\n\n"), []string{"\n", "\n"})
}
