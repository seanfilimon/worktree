//! Longest-common-subsequence matching of line sequences.
//!
//! This is Myers' O(ND) difference algorithm in its linear-space
//! ("middle snake") form, following the formulation used by GNU diff
//! (gnulib `diffseq.h`). Two preprocessing steps keep it fast on real
//! inputs:
//!
//! * lines are interned to integers, so comparisons are O(1);
//! * lines that occur on only one side are discarded up front — they can
//!   never be part of a match — which makes wholesale rewrites cost O(N).
//!
//! For pathological inputs a cost limit bounds the work per bisection; past
//! it the split point is chosen greedily, yielding a valid (possibly
//! non-minimal) matching instead of quadratic run time.

use std::collections::HashMap;

/// Number of edit steps a single bisection may explore before it falls back
/// to the greedy split. Matches up to roughly twice this many edits per
/// region are found optimally.
const COST_LIMIT: isize = 1024;

/// Returns matched line pairs `(i, j)` with `a[i] == b[j]`, strictly
/// increasing in both coordinates, forming a longest common subsequence
/// (optimal unless the cost limit is hit).
pub(crate) fn match_lines<'a>(a: &[&'a str], b: &[&'a str]) -> Vec<(usize, usize)> {
    let mut ids: HashMap<&str, usize> = HashMap::with_capacity(a.len() + b.len());
    let mut intern = |lines: &[&'a str]| -> Vec<usize> {
        lines
            .iter()
            .map(|&line| {
                let next = ids.len();
                *ids.entry(line).or_insert(next)
            })
            .collect()
    };
    let a_ids = intern(a);
    let b_ids = intern(b);

    let distinct = ids.len();
    let mut in_a = vec![false; distinct];
    let mut in_b = vec![false; distinct];
    a_ids.iter().for_each(|&id| in_a[id] = true);
    b_ids.iter().for_each(|&id| in_b[id] = true);

    // Keep only lines that occur on both sides, remembering original indices.
    let (fa, ia): (Vec<usize>, Vec<usize>) = a_ids
        .iter()
        .enumerate()
        .filter(|&(_, &id)| in_b[id])
        .map(|(i, &id)| (id, i))
        .unzip();
    let (fb, ib): (Vec<usize>, Vec<usize>) = b_ids
        .iter()
        .enumerate()
        .filter(|&(_, &id)| in_a[id])
        .map(|(j, &id)| (id, j))
        .unzip();

    let mut myers = Myers::new(&fa, &fb);
    myers.compare(0, fa.len(), 0, fb.len());
    myers.out.into_iter().map(|(i, j)| (ia[i], ib[j])).collect()
}

/// Working state for one matching run. `fd`/`bd` hold the furthest-reaching
/// x coordinate per diagonal for the forward and backward searches; they are
/// allocated once and reused by every bisection.
struct Myers<'a> {
    a: &'a [usize],
    b: &'a [usize],
    fd: Vec<isize>,
    bd: Vec<isize>,
    out: Vec<(usize, usize)>,
}

impl<'a> Myers<'a> {
    fn new(a: &'a [usize], b: &'a [usize]) -> Self {
        let diagonals = a.len() + b.len() + 3;
        Self {
            a,
            b,
            fd: vec![0; diagonals],
            bd: vec![0; diagonals],
            out: Vec::with_capacity(a.len().min(b.len())),
        }
    }

    /// Appends the matching of `a[xoff..xlim]` against `b[yoff..ylim]` to
    /// `out`, in increasing order.
    fn compare(&mut self, mut xoff: usize, mut xlim: usize, mut yoff: usize, mut ylim: usize) {
        while xoff < xlim && yoff < ylim && self.a[xoff] == self.b[yoff] {
            self.out.push((xoff, yoff));
            xoff += 1;
            yoff += 1;
        }
        let mut suffix = 0;
        while xoff < xlim && yoff < ylim && self.a[xlim - 1] == self.b[ylim - 1] {
            xlim -= 1;
            ylim -= 1;
            suffix += 1;
        }
        if xoff < xlim && yoff < ylim {
            let (xmid, ymid) = self.split(xoff, xlim, yoff, ylim);
            self.compare(xoff, xmid, yoff, ymid);
            self.compare(xmid, xlim, ymid, ylim);
        }
        self.out.extend((0..suffix).map(|i| (xlim + i, ylim + i)));
    }

    /// Finds a point on a minimal edit path through the box
    /// `[xoff, xlim) x [yoff, ylim)`. Both ranges must be non-empty and the
    /// box must not start or end with a match (callers strip those first),
    /// which guarantees the returned point is not a corner of the box.
    fn split(&mut self, xoff: usize, xlim: usize, yoff: usize, ylim: usize) -> (usize, usize) {
        let (xoff, xlim, yoff, ylim) = (xoff as isize, xlim as isize, yoff as isize, ylim as isize);
        let (a, b) = (self.a, self.b);
        let dmin = xoff - ylim;
        let dmax = xlim - yoff;
        let fmid = xoff - yoff;
        let bmid = xlim - ylim;
        let odd = (fmid - bmid) & 1 != 0;
        // Diagonal d lives at index d - dmin + 1; the extra slot on each side
        // holds the sentinels written when the searched range grows.
        let at = |d: isize| (d - dmin + 1) as usize;

        let fd = &mut self.fd;
        let bd = &mut self.bd;
        fd[at(fmid)] = xoff;
        bd[at(bmid)] = xlim;
        let (mut fmin, mut fmax, mut bmin, mut bmax) = (fmid, fmid, bmid, bmid);

        let mut cost = 1;
        loop {
            // Forward search: extend each diagonal by one edit, then slide.
            if fmin > dmin {
                fmin -= 1;
                fd[at(fmin - 1)] = -1;
            } else {
                fmin += 1;
            }
            if fmax < dmax {
                fmax += 1;
                fd[at(fmax + 1)] = -1;
            } else {
                fmax -= 1;
            }
            let mut d = fmax;
            while d >= fmin {
                let (tlo, thi) = (fd[at(d - 1)], fd[at(d + 1)]);
                let mut x = if tlo >= thi { tlo + 1 } else { thi };
                let mut y = x - d;
                while x < xlim && y < ylim && a[x as usize] == b[y as usize] {
                    x += 1;
                    y += 1;
                }
                fd[at(d)] = x;
                if odd && bmin <= d && d <= bmax && bd[at(d)] <= x {
                    return (x as usize, y as usize);
                }
                d -= 2;
            }

            // Backward search from the bottom-right corner.
            if bmin > dmin {
                bmin -= 1;
                bd[at(bmin - 1)] = isize::MAX;
            } else {
                bmin += 1;
            }
            if bmax < dmax {
                bmax += 1;
                bd[at(bmax + 1)] = isize::MAX;
            } else {
                bmax -= 1;
            }
            let mut d = bmax;
            while d >= bmin {
                let (tlo, thi) = (bd[at(d - 1)], bd[at(d + 1)]);
                let mut x = if tlo < thi { tlo } else { thi - 1 };
                let mut y = x - d;
                while xoff < x && yoff < y && a[(x - 1) as usize] == b[(y - 1) as usize] {
                    x -= 1;
                    y -= 1;
                }
                bd[at(d)] = x;
                if !odd && fmin <= d && d <= fmax && x <= fd[at(d)] {
                    return (x as usize, y as usize);
                }
                d -= 2;
            }

            if cost >= COST_LIMIT {
                // Too expensive: split at the forward point that made the
                // most progress. Any interior point yields a valid matching.
                let mut best = (xoff, yoff);
                let mut d = fmax;
                while d >= fmin {
                    let mut x = fd[at(d)].min(xlim);
                    let mut y = x - d;
                    if y > ylim {
                        x = ylim + d;
                        y = ylim;
                    }
                    if x + y > best.0 + best.1 {
                        best = (x, y);
                    }
                    d -= 2;
                }
                if best == (xoff, yoff) || best == (xlim, ylim) {
                    best = (xoff + (xlim - xoff + 1) / 2, yoff + (ylim - yoff) / 2);
                }
                return (best.0 as usize, best.1 as usize);
            }
            cost += 1;
        }
    }
}
