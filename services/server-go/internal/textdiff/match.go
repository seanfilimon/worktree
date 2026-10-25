package textdiff

// v0.6 blame workstream (docs/ENGINEERING_PLAN.md §14.4): exported line
// matching for internal/blame. Additive only; the diff itself is unchanged.

// Lines splits text into lines the way the diff does: each line keeps its
// '\n' terminator, the last one lacks it when text does not end in '\n'.
func Lines(text []byte) []string { return splitLines(text) }

// Matches returns the pairs (i, j) of lines with a[i] kept as b[j] by the
// edit script Unified renders, strictly increasing in both coordinates.
func Matches(a, b []string) [][2]int {
	ops := diffLines(a, b, 0)
	out := make([][2]int, 0, min(len(a), len(b)))
	i, j := 0, 0
	for _, o := range ops {
		switch o {
		case opEqual:
			out = append(out, [2]int{i, j})
			i++
			j++
		case opDelete:
			i++
		case opInsert:
			j++
		}
	}
	return out
}
