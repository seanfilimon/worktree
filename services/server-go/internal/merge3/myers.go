package merge3

import "math"

// costLimit is the number of edit steps a single bisection may explore
// before it falls back to a greedy split. Matches up to roughly twice this
// many edits per region are found optimally. Same value as the Rust SDK
// (crates/worktree-sdk/src/diff/myers.rs).
const costLimit = 1024

// matchLines returns matched line pairs (i, j) with a[i] == b[j], strictly
// increasing in both coordinates, forming a longest common subsequence
// (optimal unless the cost limit is hit).
//
// It is Myers' O(ND) difference algorithm in its linear-space ("middle
// snake") form, a direct port of the Rust SDK's matcher so that server and
// client merges align lines identically: lines are interned to integers and
// lines occurring on one side only are discarded up front (they can never
// match), which makes wholesale rewrites cost O(N).
func matchLines(a, b []string) [][2]int {
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
	aIDs := intern(a)
	bIDs := intern(b)

	inA := make([]bool, len(ids))
	inB := make([]bool, len(ids))
	for _, id := range aIDs {
		inA[id] = true
	}
	for _, id := range bIDs {
		inB[id] = true
	}
	// Keep only lines that occur on both sides, remembering their indices.
	var fa, ia, fb, ib []int
	for i, id := range aIDs {
		if inB[id] {
			fa = append(fa, id)
			ia = append(ia, i)
		}
	}
	for j, id := range bIDs {
		if inA[id] {
			fb = append(fb, id)
			ib = append(ib, j)
		}
	}

	m := newMyers(fa, fb)
	m.compare(0, len(fa), 0, len(fb))
	out := make([][2]int, len(m.out))
	for k, p := range m.out {
		out[k] = [2]int{ia[p[0]], ib[p[1]]}
	}
	return out
}

// myers is the working state of one matching run. fd/bd hold the
// furthest-reaching x coordinate per diagonal for the forward and backward
// searches; they are allocated once and reused by every bisection.
type myers struct {
	a, b   []int
	fd, bd []int
	out    [][2]int
}

func newMyers(a, b []int) *myers {
	diagonals := len(a) + len(b) + 3
	return &myers{
		a:   a,
		b:   b,
		fd:  make([]int, diagonals),
		bd:  make([]int, diagonals),
		out: make([][2]int, 0, min(len(a), len(b))),
	}
}

// compare appends the matching of a[xoff:xlim] against b[yoff:ylim] to out,
// in increasing order.
func (m *myers) compare(xoff, xlim, yoff, ylim int) {
	for xoff < xlim && yoff < ylim && m.a[xoff] == m.b[yoff] {
		m.out = append(m.out, [2]int{xoff, yoff})
		xoff++
		yoff++
	}
	suffix := 0
	for xoff < xlim && yoff < ylim && m.a[xlim-1] == m.b[ylim-1] {
		xlim--
		ylim--
		suffix++
	}
	if xoff < xlim && yoff < ylim {
		xmid, ymid := m.split(xoff, xlim, yoff, ylim)
		m.compare(xoff, xmid, yoff, ymid)
		m.compare(xmid, xlim, ymid, ylim)
	}
	for i := 0; i < suffix; i++ {
		m.out = append(m.out, [2]int{xlim + i, ylim + i})
	}
}

// split finds a point on a minimal edit path through the box
// [xoff, xlim) x [yoff, ylim). Both ranges must be non-empty and the box
// must not start or end with a match (compare strips those first), which
// guarantees the returned point is not a corner of the box.
func (m *myers) split(xoff, xlim, yoff, ylim int) (int, int) {
	a, b := m.a, m.b
	dmin := xoff - ylim
	dmax := xlim - yoff
	fmid := xoff - yoff
	bmid := xlim - ylim
	odd := (fmid-bmid)&1 != 0
	// Diagonal d lives at index d - dmin + 1; the extra slot on each side
	// holds the sentinels written when the searched range grows.
	at := func(d int) int { return d - dmin + 1 }

	fd, bd := m.fd, m.bd
	fd[at(fmid)] = xoff
	bd[at(bmid)] = xlim
	fmin, fmax, bmin, bmax := fmid, fmid, bmid, bmid

	for cost := 1; ; cost++ {
		// Forward search: extend each diagonal by one edit, then slide.
		if fmin > dmin {
			fmin--
			fd[at(fmin-1)] = -1
		} else {
			fmin++
		}
		if fmax < dmax {
			fmax++
			fd[at(fmax+1)] = -1
		} else {
			fmax--
		}
		for d := fmax; d >= fmin; d -= 2 {
			tlo, thi := fd[at(d-1)], fd[at(d+1)]
			x := thi
			if tlo >= thi {
				x = tlo + 1
			}
			y := x - d
			for x < xlim && y < ylim && a[x] == b[y] {
				x++
				y++
			}
			fd[at(d)] = x
			if odd && bmin <= d && d <= bmax && bd[at(d)] <= x {
				return x, y
			}
		}

		// Backward search from the bottom-right corner.
		if bmin > dmin {
			bmin--
			bd[at(bmin-1)] = math.MaxInt
		} else {
			bmin++
		}
		if bmax < dmax {
			bmax++
			bd[at(bmax+1)] = math.MaxInt
		} else {
			bmax--
		}
		for d := bmax; d >= bmin; d -= 2 {
			tlo, thi := bd[at(d-1)], bd[at(d+1)]
			x := thi - 1
			if tlo < thi {
				x = tlo
			}
			y := x - d
			for xoff < x && yoff < y && a[x-1] == b[y-1] {
				x--
				y--
			}
			bd[at(d)] = x
			if !odd && fmin <= d && d <= fmax && x <= fd[at(d)] {
				return x, y
			}
		}

		if cost >= costLimit {
			// Too expensive: split at the forward point that made the most
			// progress. Any interior point yields a valid matching.
			bx, by := xoff, yoff
			for d := fmax; d >= fmin; d -= 2 {
				x := min(fd[at(d)], xlim)
				y := x - d
				if y > ylim {
					x = ylim + d
					y = ylim
				}
				if x+y > bx+by {
					bx, by = x, y
				}
			}
			if (bx == xoff && by == yoff) || (bx == xlim && by == ylim) {
				bx, by = xoff+(xlim-xoff+1)/2, yoff+(ylim-yoff)/2
			}
			return bx, by
		}
	}
}
