package merge3

import (
	"slices"
	"strings"
)

// Labels are the names written after the conflict markers.
type Labels struct {
	Ours, Base, Theirs string
}

// DefaultLabels are "ours", "base" and "theirs".
func DefaultLabels() Labels { return Labels{Ours: "ours", Base: "base", Theirs: "theirs"} }

// Region is one conflicting region of a text merge. The line slices hold
// each side's lines without their "\n" terminators (a CRLF "\r" is kept).
type Region struct {
	// StartLine is the 1-based line of the merged output where "<<<<<<<"
	// appears.
	StartLine int `json:"start_line"`
	// OursStart is the 1-based first line of Ours in the ours file (the
	// line before the region when Ours is empty, as in unified diffs).
	OursStart int `json:"ours_start"`
	// TheirsStart is OursStart for the theirs file.
	TheirsStart int      `json:"theirs_start"`
	Ours        []string `json:"ours"`
	Base        []string `json:"base"`
	Theirs      []string `json:"theirs"`
}

// TextResult is the outcome of Text.
type TextResult struct {
	// Merged is the merged text; with conflicts it contains diff3-style
	// markers "<<<<<<< ours" / "||||||| base" / "=======" / ">>>>>>> theirs".
	Merged    string
	Conflicts []Region
}

// Clean reports whether the merge has no conflicts.
func (r TextResult) Clean() bool { return len(r.Conflicts) == 0 }

// Text is a line-based three-way merge with diff3 semantics, the same
// algorithm as the Rust SDK's merge_text (crates/worktree-sdk/src/merge):
// non-overlapping changes from both sides are combined and identical
// changes on both sides are taken once.
//
// The base is aligned with each side by a longest-common-subsequence match.
// Base lines matched on both sides are stable; each stretch between stable
// lines is resolved as follows: unchanged on one side → take the other
// side; changed identically → take it once; otherwise conflict. As in Git,
// edits touching adjacent lines (no stable line between them) conflict.
// Lines both sides agree on at the start or end of a conflicting stretch
// are moved out of the markers (Git's zdiff3) while the "|||||||" section
// always shows the full base.
//
// Marker lines end with "\r\n" when the first line of ours (or, if ours is
// empty, theirs, then base) does, and "\n" otherwise; a conflicting section
// whose last line lacks a newline gets one so markers always start a line.
// For a two-way merge (add/add) pass "" as base.
func Text(base, ours, theirs string, labels Labels) TextResult {
	baseLines := splitLines(base)
	ourLines := splitLines(ours)
	theirLines := splitLines(theirs)
	oursOf := baseToSide(baseLines, ourLines)
	theirsOf := baseToSide(baseLines, theirLines)

	eol := "\n"
	for _, lines := range [][]string{ourLines, theirLines, baseLines} {
		if len(lines) > 0 {
			if strings.HasSuffix(lines[0], "\r\n") {
				eol = "\r\n"
			}
			break
		}
	}
	out := &textOutput{eol: eol, labels: labels}
	out.text.Grow(max(len(ours), len(theirs)))

	// Positions just past the previous stable line in base/ours/theirs.
	b, o, t := 0, 0, 0
	for j := 0; j <= len(baseLines); j++ {
		var oj, tj int
		if j == len(baseLines) {
			oj, tj = len(ourLines), len(theirLines)
		} else {
			if oursOf[j] < 0 || theirsOf[j] < 0 {
				continue
			}
			oj, tj = oursOf[j], theirsOf[j]
		}
		out.chunk(baseLines[b:j], o, ourLines[o:oj], t, theirLines[t:tj])
		if j < len(baseLines) {
			out.pushLine(baseLines[j])
		}
		b, o, t = j+1, oj+1, tj+1
	}
	return TextResult{Merged: out.text.String(), Conflicts: out.conflicts}
}

// splitLines splits text into lines, each including its "\n" terminator
// (the last line may lack one). Empty text has no lines.
func splitLines(text string) []string {
	if text == "" {
		return nil
	}
	lines := strings.SplitAfter(text, "\n")
	if lines[len(lines)-1] == "" {
		lines = lines[:len(lines)-1]
	}
	return lines
}

// baseToSide maps each base line to the index of the side line it is
// matched with (-1 = unmatched).
func baseToSide(base, side []string) []int {
	m := make([]int, len(base))
	for i := range m {
		m[i] = -1
	}
	for _, p := range matchLines(base, side) {
		m[p[0]] = p[1]
	}
	return m
}

// textOutput accumulates merged text and conflict records.
type textOutput struct {
	text      strings.Builder
	lines     int // complete lines written
	eol       string
	labels    Labels
	conflicts []Region
}

func (w *textOutput) pushLine(line string) {
	w.text.WriteString(line)
	w.lines++
}

// pushTerminated pushes line, terminating it with eol if it has no newline.
func (w *textOutput) pushTerminated(line string) {
	w.pushLine(line)
	if !strings.HasSuffix(line, "\n") {
		w.text.WriteString(w.eol)
	}
}

func (w *textOutput) pushMarker(marker, label string) {
	w.text.WriteString(marker)
	if label != "" {
		w.text.WriteByte(' ')
		w.text.WriteString(label)
	}
	w.text.WriteString(w.eol)
	w.lines++
}

// chunk resolves one unstable stretch between stable lines. oOff/tOff are
// the 0-based indices of the first ours/theirs line of the stretch.
func (w *textOutput) chunk(base []string, oOff int, ours []string, tOff int, theirs []string) {
	switch {
	case slices.Equal(ours, base) || slices.Equal(ours, theirs):
		for _, l := range theirs {
			w.pushLine(l)
		}
	case slices.Equal(theirs, base):
		for _, l := range ours {
			w.pushLine(l)
		}
	default:
		prefix := 0
		for prefix < len(ours) && prefix < len(theirs) && ours[prefix] == theirs[prefix] {
			prefix++
		}
		maxSuffix := min(len(ours), len(theirs)) - prefix
		suffix := 0
		for suffix < maxSuffix && ours[len(ours)-1-suffix] == theirs[len(theirs)-1-suffix] {
			suffix++
		}
		for _, l := range ours[:prefix] {
			w.pushLine(l)
		}
		w.conflict(base, oOff+prefix, ours[prefix:len(ours)-suffix], tOff+prefix, theirs[prefix:len(theirs)-suffix])
		for _, l := range ours[len(ours)-suffix:] {
			w.pushLine(l)
		}
	}
}

// conflict writes a marked-up conflict. The text written so far always ends
// with a newline here: only a file's last line can be unterminated, and
// such a line is either part of this conflict or of the common suffix
// after it.
func (w *textOutput) conflict(base []string, oOff int, ours []string, tOff int, theirs []string) {
	start := w.lines + 1
	w.pushMarker("<<<<<<<", w.labels.Ours)
	for _, l := range ours {
		w.pushTerminated(l)
	}
	w.pushMarker("|||||||", w.labels.Base)
	for _, l := range base {
		w.pushTerminated(l)
	}
	w.pushMarker("=======", "")
	for _, l := range theirs {
		w.pushTerminated(l)
	}
	w.pushMarker(">>>>>>>", w.labels.Theirs)

	oneBased := func(off, n int) int {
		if n == 0 {
			return off
		}
		return off + 1
	}
	w.conflicts = append(w.conflicts, Region{
		StartLine:   start,
		OursStart:   oneBased(oOff, len(ours)),
		TheirsStart: oneBased(tOff, len(theirs)),
		Ours:        stripNewlines(ours),
		Base:        stripNewlines(base),
		Theirs:      stripNewlines(theirs),
	})
}

func stripNewlines(lines []string) []string {
	out := make([]string, len(lines))
	for i, l := range lines {
		out[i] = strings.TrimSuffix(l, "\n")
	}
	return out
}
