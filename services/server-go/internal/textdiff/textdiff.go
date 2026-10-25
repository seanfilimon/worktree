// Package textdiff computes line-oriented differences between two texts and
// renders them as unified diffs in the format produced by GNU diff -u.
//
// The core is Myers' O((N+M)·D) algorithm in its linear-space
// (divide-and-conquer, middle snake) form, preceded by common prefix/suffix
// trimming. To keep worst-case inputs fast (two large, mostly unrelated
// files) the search for each middle snake is bounded by a cost limit; once
// the limit is hit the furthest-reaching path found so far is used as the
// split point. The result is then still a correct edit script, merely not
// guaranteed to be minimal. For typical edits the limit is never reached
// and the output is a minimal diff.
//
// Lines keep their terminator: "a\n" and "a" (a final line without a
// newline) are different lines, exactly as in GNU diff, and the rendered
// patch marks the latter with "\ No newline at end of file".
package textdiff

import (
	"bytes"
	"strconv"
	"strings"
	"unicode/utf8"
)

// DefaultContext is the number of unchanged lines shown around each change
// by diff -u.
const DefaultContext = 3

// binarySniffLen is how much of a file IsBinary scans for NUL bytes.
const binarySniffLen = 8 << 10

// minCostLimit is the smallest bound on the edit cost explored per middle
// snake search (see Options.MaxCost).
const minCostLimit = 1024

// Options configures Unified.
type Options struct {
	// Context is the number of unchanged lines shown before and after each
	// change. Negative values are treated as 0.
	Context int
	// OldName and NewName are written verbatim on the "---" and "+++"
	// header lines, e.g. "a/main.go" and "b/main.go", or "/dev/null".
	OldName, NewName string
	// MaxCost bounds the edit distance explored by each middle-snake
	// search before falling back to a heuristic split. 0 selects a default
	// that scales with the input size. Smaller values are faster on large,
	// dissimilar inputs at the price of less minimal diffs.
	MaxCost int
}

// Result is the outcome of Unified.
type Result struct {
	// Patch is the unified diff including the "---"/"+++" headers, or ""
	// when the texts are identical.
	Patch string
	// Additions and Deletions count added and removed lines.
	Additions, Deletions int
}

// IsBinary reports whether data should be treated as binary rather than
// text: it contains a NUL byte within its first 8 KiB, or it is not valid
// UTF-8.
func IsBinary(data []byte) bool {
	head := data
	if len(head) > binarySniffLen {
		head = head[:binarySniffLen]
	}
	return bytes.IndexByte(head, 0) >= 0 || !utf8.Valid(data)
}

// Unified diffs oldText against newText and renders the result as a unified
// diff. It does not check for binary content; callers should use IsBinary
// first.
func Unified(oldText, newText []byte, o Options) Result {
	a, b := splitLines(oldText), splitLines(newText)
	ops := diffLines(a, b, o.MaxCost)
	var res Result
	for _, op := range ops {
		switch op {
		case opDelete:
			res.Deletions++
		case opInsert:
			res.Additions++
		}
	}
	if res.Additions == 0 && res.Deletions == 0 {
		return res
	}
	ctx := o.Context
	if ctx < 0 {
		ctx = 0
	}
	res.Patch = render(a, b, ops, ctx, o.OldName, o.NewName)
	return res
}

// splitLines splits text after every '\n'. Each element keeps its
// terminator; the last element lacks one when text does not end in '\n'.
func splitLines(text []byte) []string {
	if len(text) == 0 {
		return nil
	}
	s := string(text)
	n := strings.Count(s, "\n")
	if !strings.HasSuffix(s, "\n") {
		n++
	}
	lines := make([]string, 0, n)
	for len(s) > 0 {
		i := strings.IndexByte(s, '\n')
		if i < 0 {
			lines = append(lines, s)
			break
		}
		lines = append(lines, s[:i+1])
		s = s[i+1:]
	}
	return lines
}

// op is one step of an edit script.
type op uint8

const (
	opEqual  op = iota // line kept: advances both sides
	opDelete           // line of a removed: advances a
	opInsert           // line of b added: advances b
)

// diffLines returns an edit script transforming a into b. Within each run
// of changes deletions precede insertions, as in GNU diff.
func diffLines(a, b []string, maxCost int) []op {
	// Intern lines so that comparisons are integer comparisons.
	ids := make(map[string]int, len(a)+len(b))
	intern := func(lines []string) []int {
		out := make([]int, len(lines))
		for i, l := range lines {
			id, ok := ids[l]
			if !ok {
				id = len(ids)
				ids[l] = id
			}
			out[i] = id
		}
		return out
	}
	d := &differ{a: intern(a), delA: make([]bool, len(a))}
	bIDs := intern(b)
	d.b = bIDs
	d.insB = make([]bool, len(b))
	d.costLimit = maxCost
	if d.costLimit <= 0 {
		d.costLimit = max(minCostLimit, isqrt(len(a)+len(b))*16)
	}

	if !shareAnyLine(d.a, d.b) {
		// Nothing can match: skip the search entirely.
		for i := range d.delA {
			d.delA[i] = true
		}
		for j := range d.insB {
			d.insB[j] = true
		}
	} else {
		d.compare(0, len(a), 0, len(b))
	}

	ops := make([]op, 0, len(a)+len(b))
	i, j := 0, 0
	for i < len(a) || j < len(b) {
		switch {
		case i < len(a) && d.delA[i]:
			ops = append(ops, opDelete)
			i++
		case j < len(b) && d.insB[j]:
			ops = append(ops, opInsert)
			j++
		default:
			ops = append(ops, opEqual)
			i++
			j++
		}
	}
	return ops
}

// shareAnyLine reports whether a and b have at least one line in common.
func shareAnyLine(a, b []int) bool {
	if len(a) == 0 || len(b) == 0 {
		return false
	}
	seen := make(map[int]struct{}, len(a))
	for _, x := range a {
		seen[x] = struct{}{}
	}
	for _, y := range b {
		if _, ok := seen[y]; ok {
			return true
		}
	}
	return false
}

func isqrt(n int) int {
	if n <= 0 {
		return 0
	}
	x := n
	y := (x + 1) / 2
	for y < x {
		x = y
		y = (x + n/x) / 2
	}
	return x
}

// differ holds the state of one linear-space Myers run. delA[i] / insB[j]
// mark lines of a deleted / lines of b inserted.
type differ struct {
	a, b       []int
	delA, insB []bool
	costLimit  int
	v1, v2     []int // scratch for bisect, reused across calls
}

// compare marks the differences between a[xlo:xhi] and b[ylo:yhi].
func (d *differ) compare(xlo, xhi, ylo, yhi int) {
	for {
		// Trim the common prefix and suffix.
		for xlo < xhi && ylo < yhi && d.a[xlo] == d.b[ylo] {
			xlo++
			ylo++
		}
		for xlo < xhi && ylo < yhi && d.a[xhi-1] == d.b[yhi-1] {
			xhi--
			yhi--
		}
		switch {
		case xlo == xhi:
			for j := ylo; j < yhi; j++ {
				d.insB[j] = true
			}
			return
		case ylo == yhi:
			for i := xlo; i < xhi; i++ {
				d.delA[i] = true
			}
			return
		}
		x, y, ok := d.bisect(xlo, xhi, ylo, yhi)
		if !ok {
			// No usable split: replace the whole range.
			for i := xlo; i < xhi; i++ {
				d.delA[i] = true
			}
			for j := ylo; j < yhi; j++ {
				d.insB[j] = true
			}
			return
		}
		// Recurse into the smaller half, loop on the larger one to bound
		// the stack depth.
		if (x-xlo)+(y-ylo) < (xhi-x)+(yhi-y) {
			d.compare(xlo, x, ylo, y)
			xlo, ylo = x, y
		} else {
			d.compare(x, xhi, y, yhi)
			xhi, yhi = x, y
		}
	}
}

// bisect finds a split point (x, y), xlo <= x <= xhi and ylo <= y <= yhi,
// that lies on an optimal (or, past the cost limit, a good) edit path from
// (xlo, ylo) to (xhi, yhi), strictly inside the range so that both halves
// are smaller. The range must have no common prefix or suffix and both
// sides must be non-empty. ok is false when no such point was found.
func (d *differ) bisect(xlo, xhi, ylo, yhi int) (x, y int, ok bool) {
	a, b := d.a[xlo:xhi], d.b[ylo:yhi]
	n, m := len(a), len(b)
	maxD := (n + m + 1) / 2
	limit := min(maxD, d.costLimit)
	vOffset := limit + 1
	vLen := 2*limit + 3
	if cap(d.v1) < vLen {
		d.v1 = make([]int, vLen)
		d.v2 = make([]int, vLen)
	}
	v1, v2 := d.v1[:vLen], d.v2[:vLen]
	for i := range v1 {
		v1[i] = -1
		v2[i] = -1
	}
	v1[vOffset+1] = 0
	v2[vOffset+1] = 0
	delta := n - m
	front := delta%2 != 0 // the paths overlap during the forward pass
	var k1start, k1end, k2start, k2end int
	bestX, bestY, best := 0, 0, -1

	// split validates a candidate point: it must lie inside the range and
	// leave two non-empty halves.
	split := func(sx, sy int) (int, int, bool) {
		if sx < 0 || sy < 0 || sx > n || sy > m || (sx == 0 && sy == 0) || (sx == n && sy == m) {
			return 0, 0, false
		}
		return xlo + sx, ylo + sy, true
	}

	for dd := 0; dd <= limit; dd++ {
		// Forward path.
		for k1 := -dd + k1start; k1 <= dd-k1end; k1 += 2 {
			k1Off := vOffset + k1
			var x1 int
			if k1 == -dd || (k1 != dd && v1[k1Off-1] < v1[k1Off+1]) {
				x1 = v1[k1Off+1]
			} else {
				x1 = v1[k1Off-1] + 1
			}
			y1 := x1 - k1
			for x1 < n && y1 < m && a[x1] == b[y1] {
				x1++
				y1++
			}
			v1[k1Off] = x1
			switch {
			case x1 > n:
				k1end += 2 // ran off the right edge
			case y1 > m:
				k1start += 2 // ran off the bottom edge
			default:
				if x1+y1 > best {
					bestX, bestY, best = x1, y1, x1+y1
				}
				if front {
					k2Off := vOffset + delta - k1
					if k2Off >= 0 && k2Off < vLen && v2[k2Off] != -1 {
						// Mirror the reverse path onto forward coordinates.
						if x2 := n - v2[k2Off]; x1 >= x2 {
							if sx, sy, ok := split(x1, y1); ok {
								return sx, sy, true
							}
						}
					}
				}
			}
		}
		// Reverse path.
		for k2 := -dd + k2start; k2 <= dd-k2end; k2 += 2 {
			k2Off := vOffset + k2
			var x2 int
			if k2 == -dd || (k2 != dd && v2[k2Off-1] < v2[k2Off+1]) {
				x2 = v2[k2Off+1]
			} else {
				x2 = v2[k2Off-1] + 1
			}
			y2 := x2 - k2
			for x2 < n && y2 < m && a[n-x2-1] == b[m-y2-1] {
				x2++
				y2++
			}
			v2[k2Off] = x2
			switch {
			case x2 > n:
				k2end += 2
			case y2 > m:
				k2start += 2
			default:
				if !front {
					k1Off := vOffset + delta - k2
					if k1Off >= 0 && k1Off < vLen && v1[k1Off] != -1 {
						x1 := v1[k1Off]
						y1 := vOffset + x1 - k1Off
						if x1 >= n-x2 {
							if sx, sy, ok := split(x1, y1); ok {
								return sx, sy, true
							}
						}
					}
				}
			}
		}
	}
	// Cost limit reached: split at the furthest-reaching forward point.
	if best < 0 {
		return 0, 0, false
	}
	return split(bestX, bestY)
}

// render formats ops as a unified diff with ctx lines of context.
func render(a, b []string, ops []op, ctx int, oldName, newName string) string {
	// aBefore[k] / bBefore[k] = lines of a / b consumed before ops[k].
	aBefore := make([]int, len(ops)+1)
	bBefore := make([]int, len(ops)+1)
	for k, o := range ops {
		aBefore[k+1], bBefore[k+1] = aBefore[k], bBefore[k]
		if o != opInsert {
			aBefore[k+1]++
		}
		if o != opDelete {
			bBefore[k+1]++
		}
	}

	var sb strings.Builder
	sb.WriteString("--- ")
	sb.WriteString(oldName)
	sb.WriteString("\n+++ ")
	sb.WriteString(newName)
	sb.WriteByte('\n')

	k := 0
	for {
		// Next change.
		for k < len(ops) && ops[k] == opEqual {
			k++
		}
		if k == len(ops) {
			break
		}
		start := max(0, k-ctx)
		// Extend over changes separated by at most 2*ctx equal lines.
		end := k
		for {
			for end < len(ops) && ops[end] != opEqual {
				end++
			}
			// ops[end:] starts with an equal run (or is empty).
			next := end
			for next < len(ops) && ops[next] == opEqual {
				next++
			}
			if next < len(ops) && next-end <= 2*ctx {
				end = next
				continue
			}
			break
		}
		stop := min(len(ops), end+ctx)

		aStart, aCount := aBefore[start], aBefore[stop]-aBefore[start]
		bStart, bCount := bBefore[start], bBefore[stop]-bBefore[start]
		sb.WriteString("@@ -")
		writeRange(&sb, aStart, aCount)
		sb.WriteString(" +")
		writeRange(&sb, bStart, bCount)
		sb.WriteString(" @@\n")
		for i := start; i < stop; i++ {
			switch ops[i] {
			case opEqual:
				writeLine(&sb, ' ', b[bBefore[i]])
			case opDelete:
				writeLine(&sb, '-', a[aBefore[i]])
			case opInsert:
				writeLine(&sb, '+', b[bBefore[i]])
			}
		}
		k = stop
	}
	return sb.String()
}

// writeRange writes a GNU unified hunk range for count lines starting at
// 0-based index start: "l,c", "l" when c == 1, and "start,0" (the line
// before the empty range) when c == 0.
func writeRange(sb *strings.Builder, start, count int) {
	switch count {
	case 0:
		sb.WriteString(strconv.Itoa(start))
		sb.WriteString(",0")
	case 1:
		sb.WriteString(strconv.Itoa(start + 1))
	default:
		sb.WriteString(strconv.Itoa(start + 1))
		sb.WriteByte(',')
		sb.WriteString(strconv.Itoa(count))
	}
}

func writeLine(sb *strings.Builder, prefix byte, line string) {
	sb.WriteByte(prefix)
	sb.WriteString(line)
	if !strings.HasSuffix(line, "\n") {
		sb.WriteString("\n\\ No newline at end of file\n")
	}
}
