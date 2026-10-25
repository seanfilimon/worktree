package textdiff

import (
	"fmt"
	"math/rand"
	"strconv"
	"strings"
	"testing"
	"time"
)

func TestUnifiedGolden(t *testing.T) {
	tests := []struct {
		name     string
		old, new string
		ctx      int
		want     string
		add, del int
	}{
		{name: "identical", old: "a\nb\n", new: "a\nb\n", ctx: 3, want: ""},
		{name: "both empty", old: "", new: "", ctx: 3, want: ""},
		{
			name: "added file", old: "", new: "one\ntwo\n", ctx: 3, add: 2,
			want: "--- a\n+++ b\n@@ -0,0 +1,2 @@\n+one\n+two\n",
		},
		{
			name: "deleted file", old: "one\n", new: "", ctx: 3, del: 1,
			want: "--- a\n+++ b\n@@ -1 +0,0 @@\n-one\n",
		},
		{
			name: "single replacement", old: "a\nb\nc\n", new: "a\nB\nc\n", ctx: 3, add: 1, del: 1,
			want: "--- a\n+++ b\n@@ -1,3 +1,3 @@\n a\n-b\n+B\n c\n",
		},
		{
			name: "context zero insertion", old: "1\n2\n3\n4\n5\n", new: "1\n2\nx\n3\n4\n5\n", ctx: 0, add: 1,
			want: "--- a\n+++ b\n@@ -2,0 +3 @@\n+x\n",
		},
		{
			name: "context zero deletion", old: "1\n2\n3\n", new: "1\n3\n", ctx: 0, del: 1,
			want: "--- a\n+++ b\n@@ -2 +1,0 @@\n-2\n",
		},
		{
			name: "newline added at eof", old: "a\nb", new: "a\nb\n", ctx: 3, add: 1, del: 1,
			want: "--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\n-b\n\\ No newline at end of file\n+b\n",
		},
		{
			name: "newline removed at eof", old: "a\n", new: "a", ctx: 3, add: 1, del: 1,
			want: "--- a\n+++ b\n@@ -1 +1 @@\n-a\n+a\n\\ No newline at end of file\n",
		},
		{
			name: "unterminated context line", old: "a\nb\nc", new: "A\nb\nc", ctx: 3, add: 1, del: 1,
			want: "--- a\n+++ b\n@@ -1,3 +1,3 @@\n-a\n+A\n b\n c\n\\ No newline at end of file\n",
		},
		{
			name: "two separate hunks",
			old:  "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\n12\n",
			new:  "X\n2\n3\n4\n5\n6\n7\n8\n9\n10\n11\nY\n",
			ctx:  2, add: 2, del: 2,
			want: "--- a\n+++ b\n@@ -1,3 +1,3 @@\n-1\n+X\n 2\n 3\n@@ -10,3 +10,3 @@\n 10\n 11\n-12\n+Y\n",
		},
		{
			name: "close changes merge into one hunk",
			old:  "1\n2\n3\n4\n5\n6\n",
			new:  "X\n2\n3\n4\n5\nY\n",
			ctx:  2, add: 2, del: 2,
			want: "--- a\n+++ b\n@@ -1,6 +1,6 @@\n-1\n+X\n 2\n 3\n 4\n 5\n-6\n+Y\n",
		},
		{
			name: "crlf preserved", old: "a\r\nb\r\n", new: "a\r\nc\r\n", ctx: 1, add: 1, del: 1,
			want: "--- a\n+++ b\n@@ -1,2 +1,2 @@\n a\r\n-b\r\n+c\r\n",
		},
		{
			name: "negative context acts as zero", old: "a\nb\nc\n", new: "a\nB\nc\n", ctx: -5, add: 1, del: 1,
			want: "--- a\n+++ b\n@@ -2 +2 @@\n-b\n+B\n",
		},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			res := Unified([]byte(tt.old), []byte(tt.new), Options{Context: tt.ctx, OldName: "a", NewName: "b"})
			if res.Patch != tt.want {
				t.Fatalf("patch mismatch\n got: %q\nwant: %q", res.Patch, tt.want)
			}
			if res.Additions != tt.add || res.Deletions != tt.del {
				t.Fatalf("counts = +%d -%d, want +%d -%d", res.Additions, res.Deletions, tt.add, tt.del)
			}
			if tt.want != "" {
				assertApplies(t, tt.old, tt.new, res.Patch)
			}
		})
	}
}

func TestIsBinary(t *testing.T) {
	big := strings.Repeat("x", binarySniffLen) + "\x00"
	tests := []struct {
		name string
		data string
		want bool
	}{
		{"empty", "", false},
		{"ascii", "hello\nworld\n", false},
		{"utf8", "héllo 世界\n", false},
		{"nul", "abc\x00def", true},
		{"invalid utf8", "abc\xff\xfe", true},
		{"nul after sniff window", big, false},
		{"invalid utf8 late", strings.Repeat("y", 3*binarySniffLen) + "\xc3", true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := IsBinary([]byte(tt.data)); got != tt.want {
				t.Fatalf("IsBinary = %v, want %v", got, tt.want)
			}
		})
	}
}

func TestSplitLines(t *testing.T) {
	tests := []struct {
		in   string
		want []string
	}{
		{"", nil},
		{"a", []string{"a"}},
		{"a\n", []string{"a\n"}},
		{"a\nb", []string{"a\n", "b"}},
		{"\n\n", []string{"\n", "\n"}},
	}
	for _, tt := range tests {
		got := splitLines([]byte(tt.in))
		if fmt.Sprint(got) != fmt.Sprint(tt.want) || len(got) != len(tt.want) {
			t.Errorf("splitLines(%q) = %q, want %q", tt.in, got, tt.want)
		}
	}
}

// TestRoundTripRandom checks on many random inputs that applying the patch
// to the old text yields the new text, for several context sizes and cost
// limits (including a tiny one that forces the heuristic split).
func TestRoundTripRandom(t *testing.T) {
	rng := rand.New(rand.NewSource(1))
	for iter := 0; iter < 2000; iter++ {
		old := randomText(rng, rng.Intn(40))
		nw := mutate(rng, old)
		ctx := rng.Intn(5)
		maxCost := 0
		if iter%3 == 0 {
			maxCost = 1 + rng.Intn(3)
		}
		res := Unified([]byte(old), []byte(nw), Options{Context: ctx, OldName: "a/f", NewName: "b/f", MaxCost: maxCost})
		if old == nw {
			if res.Patch != "" {
				t.Fatalf("iter %d: non-empty patch for identical input", iter)
			}
			continue
		}
		if res.Patch == "" {
			t.Fatalf("iter %d: empty patch for different inputs %q -> %q", iter, old, nw)
		}
		assertApplies(t, old, nw, res.Patch)
	}
}

// TestMinimal compares the number of edits with the LCS-optimal value on
// small inputs, where the cost limit is never reached.
func TestMinimal(t *testing.T) {
	rng := rand.New(rand.NewSource(2))
	for iter := 0; iter < 1000; iter++ {
		a := splitLines([]byte(randomText(rng, rng.Intn(30))))
		b := splitLines([]byte(randomText(rng, rng.Intn(30))))
		ops := diffLines(a, b, 0)
		edits := 0
		for _, o := range ops {
			if o != opEqual {
				edits++
			}
		}
		want := len(a) + len(b) - 2*lcs(a, b)
		if edits != want {
			t.Fatalf("iter %d: %d edits, optimal is %d\na=%q\nb=%q", iter, edits, want, a, b)
		}
	}
}

func TestLargeFilesAreFast(t *testing.T) {
	const n = 10000
	var base strings.Builder
	for i := 0; i < n; i++ {
		fmt.Fprintf(&base, "line %d: the quick brown fox jumps over the lazy dog\n", i)
	}
	baseLines := strings.SplitAfter(base.String(), "\n")
	baseLines = baseLines[:len(baseLines)-1]

	scattered := append([]string(nil), baseLines...)
	for i := 0; i < len(scattered); i += 97 {
		scattered[i] = "edited " + scattered[i]
	}
	var unrelated, interleaved strings.Builder
	for i := 0; i < n; i++ {
		fmt.Fprintf(&unrelated, "completely different %d\n", i)
		if i%2 == 0 {
			interleaved.WriteString(baseLines[i])
		} else {
			fmt.Fprintf(&interleaved, "other %d\n", i)
		}
	}
	shuffled := append([]string(nil), baseLines...)
	rand.New(rand.NewSource(3)).Shuffle(len(shuffled), func(i, j int) { shuffled[i], shuffled[j] = shuffled[j], shuffled[i] })

	cases := []struct {
		name string
		new  string
	}{
		{"scattered edits", strings.Join(scattered, "")},
		{"unrelated", unrelated.String()},
		{"interleaved", interleaved.String()},
		{"shuffled", strings.Join(shuffled, "")},
		{"appended", base.String() + "tail\n"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			start := time.Now()
			res := Unified([]byte(base.String()), []byte(tc.new), Options{Context: 3, OldName: "a", NewName: "b"})
			elapsed := time.Since(start)
			if elapsed > 5*time.Second {
				t.Fatalf("diff took %v", elapsed)
			}
			assertApplies(t, base.String(), tc.new, res.Patch)
		})
	}
	// The scattered case is easy and must be minimal.
	res := Unified([]byte(base.String()), []byte(strings.Join(scattered, "")), Options{})
	edits := (n + 96) / 97
	if res.Additions != edits || res.Deletions != edits {
		t.Fatalf("scattered: +%d -%d, want +%d -%d", res.Additions, res.Deletions, edits, edits)
	}
}

func BenchmarkUnified10kScattered(b *testing.B) {
	var old, nw strings.Builder
	for i := 0; i < 10000; i++ {
		fmt.Fprintf(&old, "line %d\n", i)
		if i%50 == 0 {
			fmt.Fprintf(&nw, "changed %d\n", i)
		} else {
			fmt.Fprintf(&nw, "line %d\n", i)
		}
	}
	ob, nb := []byte(old.String()), []byte(nw.String())
	b.ResetTimer()
	for i := 0; i < b.N; i++ {
		Unified(ob, nb, Options{Context: 3})
	}
}

// ---- helpers ----

// randomText builds n lines from a small alphabet (so that lines repeat and
// the diff has real choices to make), sometimes without a final newline.
func randomText(rng *rand.Rand, n int) string {
	var sb strings.Builder
	for i := 0; i < n; i++ {
		sb.WriteString(string(rune('a' + rng.Intn(5))))
		sb.WriteByte('\n')
	}
	s := sb.String()
	if n > 0 && rng.Intn(4) == 0 {
		s = strings.TrimSuffix(s, "\n")
	}
	return s
}

// mutate applies random line insertions, deletions and replacements.
func mutate(rng *rand.Rand, s string) string {
	lines := strings.SplitAfter(s, "\n")
	if len(lines) > 0 && lines[len(lines)-1] == "" {
		lines = lines[:len(lines)-1]
	}
	for k := rng.Intn(6); k > 0; k-- {
		switch pos := rng.Intn(len(lines) + 1); rng.Intn(3) {
		case 0:
			ins := string(rune('a'+rng.Intn(7))) + "\n"
			lines = append(lines[:pos], append([]string{ins}, lines[pos:]...)...)
		case 1:
			if pos < len(lines) {
				lines = append(lines[:pos], lines[pos+1:]...)
			}
		default:
			if pos < len(lines) {
				lines[pos] = string(rune('a'+rng.Intn(7))) + "\n"
			}
		}
	}
	out := strings.Join(lines, "")
	switch rng.Intn(5) {
	case 0:
		out = strings.TrimSuffix(out, "\n")
	case 1:
		if out != "" && !strings.HasSuffix(out, "\n") {
			out += "\n"
		}
	}
	return out
}

func lcs(a, b []string) int {
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

func assertApplies(t *testing.T, old, want, patch string) {
	t.Helper()
	got, err := apply(old, patch)
	if err != nil {
		t.Fatalf("apply: %v\npatch:\n%s", err, patch)
	}
	if got != want {
		t.Fatalf("apply mismatch\n got: %q\nwant: %q\npatch:\n%s", got, want, patch)
	}
}

// apply is a strict unified-diff applier: context and removed lines must
// match exactly, hunk counts must match their headers, and hunks must be
// in order and non-overlapping.
func apply(old, patch string) (string, error) {
	src := splitLines([]byte(old))
	lines := splitLines([]byte(patch))
	if len(lines) < 2 || !strings.HasPrefix(lines[0], "--- ") || !strings.HasPrefix(lines[1], "+++ ") {
		return "", fmt.Errorf("missing headers")
	}
	lines = lines[2:]
	var out []string
	pos := 0 // next unconsumed line of src
	for i := 0; i < len(lines); {
		h := lines[i]
		var oldStart, oldCount, newStart, newCount int
		if !strings.HasPrefix(h, "@@ -") || !strings.HasSuffix(h, " @@\n") {
			return "", fmt.Errorf("bad hunk header %q", h)
		}
		fields := strings.Fields(strings.TrimSuffix(strings.TrimPrefix(h, "@@ "), " @@\n"))
		if len(fields) != 2 {
			return "", fmt.Errorf("bad hunk header %q", h)
		}
		var err error
		if oldStart, oldCount, err = parseRange(fields[0], '-'); err != nil {
			return "", err
		}
		if newStart, newCount, err = parseRange(fields[1], '+'); err != nil {
			return "", err
		}
		_ = newStart
		i++
		at := oldStart - 1
		if oldCount == 0 {
			at = oldStart
		}
		if at < pos || at > len(src) {
			return "", fmt.Errorf("hunk %q out of order or range", h)
		}
		out = append(out, src[pos:at]...)
		pos = at
		gotOld, gotNew := 0, 0
		for i < len(lines) && !strings.HasPrefix(lines[i], "@@") {
			l := lines[i]
			i++
			if l == "" || l[0] == '\\' {
				return "", fmt.Errorf("unexpected line %q", l)
			}
			content := l[1:]
			if i < len(lines) && strings.HasPrefix(lines[i], "\\ No newline at end of file") {
				content = strings.TrimSuffix(content, "\n")
				i++
			}
			switch l[0] {
			case ' ', '-':
				if pos >= len(src) {
					return "", fmt.Errorf("line %d: past end of input, patch expects %q", pos+1, content)
				}
				if src[pos] != content {
					return "", fmt.Errorf("line %d: have %q, patch expects %q", pos+1, src[pos], content)
				}
				pos++
				gotOld++
				if l[0] == ' ' {
					out = append(out, content)
					gotNew++
				}
			case '+':
				out = append(out, content)
				gotNew++
			default:
				return "", fmt.Errorf("bad line prefix %q", l)
			}
		}
		if gotOld != oldCount || gotNew != newCount {
			return "", fmt.Errorf("hunk %q has -%d +%d lines", h, gotOld, gotNew)
		}
	}
	out = append(out, src[pos:]...)
	return strings.Join(out, ""), nil
}

func parseRange(s string, sign byte) (start, count int, err error) {
	if s == "" || s[0] != sign {
		return 0, 0, fmt.Errorf("bad range %q", s)
	}
	s = s[1:]
	count = 1
	if i := strings.IndexByte(s, ','); i >= 0 {
		if count, err = strconv.Atoi(s[i+1:]); err != nil {
			return 0, 0, err
		}
		s = s[:i]
	}
	start, err = strconv.Atoi(s)
	return start, count, err
}
