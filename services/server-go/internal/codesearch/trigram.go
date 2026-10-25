package codesearch

import (
	"regexp/syntax"
	"slices"
	"sort"
	"unicode"
	"unicode/utf8"
)

// Trigram prefilter.
//
// Every indexed file contributes the set of its byte trigrams, computed
// over the content with ASCII letters folded to lower case (non-ASCII
// bytes are kept as they are). A query is turned into a boolean formula
// over trigrams that every matching file must satisfy (a necessary, never
// a sufficient, condition): candidates are the files whose trigram sets
// satisfy the formula, and only they are scanned line by line.
//
// The formula is derived from the parsed regular expression (literal
// searches are compiled to an equivalent expression first). Case-folded
// literals expand each rune to its full Unicode simple-fold orbit (so
// "k" also covers "K" and the Kelvin sign), which keeps the prefilter
// exact for case-insensitive searches.

// trigram is three bytes packed big-endian into the low 24 bits.
type trigram uint32

// foldASCII lower-cases an ASCII letter and leaves every other byte alone.
func foldASCII(b byte) byte {
	if 'A' <= b && b <= 'Z' {
		return b + ('a' - 'A')
	}
	return b
}

func makeTrigram(a, b, c byte) trigram {
	return trigram(uint32(foldASCII(a))<<16 | uint32(foldASCII(b))<<8 | uint32(foldASCII(c)))
}

// stringTrigrams returns the distinct trigrams of s, sorted.
func stringTrigrams(s string) []trigram {
	if len(s) < 3 {
		return nil
	}
	seen := make(map[trigram]struct{}, len(s))
	out := make([]trigram, 0, len(s)-2)
	for i := 0; i+3 <= len(s); i++ {
		t := makeTrigram(s[i], s[i+1], s[i+2])
		if _, dup := seen[t]; dup {
			continue
		}
		seen[t] = struct{}{}
		out = append(out, t)
	}
	sort.Slice(out, func(i, j int) bool { return out[i] < out[j] })
	return out
}

// ---- boolean trigram queries ----

type qop uint8

const (
	qAll  qop = iota // every file
	qNone            // no file
	qAnd             // every trigram and every sub-query
	qOr              // any trigram or any sub-query
)

// tquery is a boolean formula over trigrams.
type tquery struct {
	op  qop
	tri []trigram
	sub []*tquery
}

var (
	allQuery  = &tquery{op: qAll}
	noneQuery = &tquery{op: qNone}
)

// andQuery combines qs with AND, simplifying trivial operands.
func andQuery(qs ...*tquery) *tquery {
	out := &tquery{op: qAnd}
	for _, q := range qs {
		switch q.op {
		case qAll:
		case qNone:
			return noneQuery
		case qAnd:
			out.tri = append(out.tri, q.tri...)
			out.sub = append(out.sub, q.sub...)
		default:
			out.sub = append(out.sub, q)
		}
	}
	out.tri = dedupe(out.tri)
	if len(out.tri) == 0 && len(out.sub) == 0 {
		return allQuery
	}
	if len(out.tri) == 0 && len(out.sub) == 1 {
		return out.sub[0]
	}
	return out
}

// orQuery combines qs with OR, simplifying trivial operands.
func orQuery(qs ...*tquery) *tquery {
	out := &tquery{op: qOr}
	for _, q := range qs {
		switch q.op {
		case qAll:
			return allQuery
		case qNone:
		case qOr:
			out.tri = append(out.tri, q.tri...)
			out.sub = append(out.sub, q.sub...)
		default:
			if q.op == qAnd && len(q.tri) == 1 && len(q.sub) == 0 {
				out.tri = append(out.tri, q.tri[0])
				continue
			}
			out.sub = append(out.sub, q)
		}
	}
	out.tri = dedupe(out.tri)
	if len(out.tri) == 0 && len(out.sub) == 0 {
		return noneQuery
	}
	if len(out.tri) == 0 && len(out.sub) == 1 {
		return out.sub[0]
	}
	return out
}

// dedupe sorts ts and removes duplicates.
func dedupe(ts []trigram) []trigram {
	slices.Sort(ts)
	return slices.Compact(ts)
}

// stringQuery requires every trigram of s; strings shorter than three
// bytes impose no condition.
func stringQuery(s string) *tquery {
	tri := stringTrigrams(s)
	if len(tri) == 0 {
		return allQuery
	}
	return &tquery{op: qAnd, tri: tri}
}

// ---- regular expression analysis ----

// maxExact bounds the exact-string sets carried through the analysis.
const maxExact = 64

// info describes what a sub-expression can match. When exact is non-nil
// the sub-expression matches exactly one of its strings; match is always
// a necessary condition for the sub-expression to match.
type info struct {
	exact map[string]struct{}
	match *tquery
}

func anyInfo() info { return info{match: allQuery} }

// exactInfo returns the info of a sub-expression matching exactly one of
// ss. Strings are stored ASCII-folded: strings equal after folding have
// the same trigrams, so keeping one of them keeps the sets small.
func exactInfo(ss ...string) info {
	set := make(map[string]struct{}, len(ss))
	for _, s := range ss {
		set[foldASCIIString(s)] = struct{}{}
	}
	return info{exact: set, match: allQuery}
}

func foldASCIIString(s string) string {
	b := []byte(s)
	for i := range b {
		b[i] = foldASCII(b[i])
	}
	return string(b)
}

// query converts the info into one necessary condition.
func (i info) query() *tquery {
	if i.exact == nil {
		return i.match
	}
	alts := make([]*tquery, 0, len(i.exact))
	for _, s := range sortedKeys(i.exact) {
		alts = append(alts, stringQuery(s))
	}
	return andQuery(i.match, orQuery(alts...))
}

func sortedKeys(set map[string]struct{}) []string {
	out := make([]string, 0, len(set))
	for s := range set {
		out = append(out, s)
	}
	sort.Strings(out)
	return out
}

func concatInfo(a, b info) info {
	if a.exact != nil && b.exact != nil && len(a.exact)*len(b.exact) <= maxExact {
		set := make(map[string]struct{}, len(a.exact)*len(b.exact))
		for x := range a.exact {
			for y := range b.exact {
				set[x+y] = struct{}{}
			}
		}
		return info{exact: set, match: andQuery(a.match, b.match)}
	}
	return info{match: andQuery(a.query(), b.query())}
}

func alternateInfo(a, b info) info {
	if a.exact != nil && b.exact != nil && len(a.exact)+len(b.exact) <= maxExact {
		set := make(map[string]struct{}, len(a.exact)+len(b.exact))
		for x := range a.exact {
			set[x] = struct{}{}
		}
		for y := range b.exact {
			set[y] = struct{}{}
		}
		return info{exact: set, match: orQuery(a.match, b.match)}
	}
	return info{match: orQuery(a.query(), b.query())}
}

// foldOrbit returns r and every rune that case-folds to it.
func foldOrbit(r rune) []rune {
	out := []rune{r}
	for f := unicode.SimpleFold(r); f != r; f = unicode.SimpleFold(f) {
		out = append(out, f)
	}
	return out
}

// runeSet returns the exact info of one rune out of rs.
func runeSet(rs []rune) info {
	ss := make([]string, 0, len(rs))
	for _, r := range rs {
		ss = append(ss, string(r))
	}
	return exactInfo(ss...)
}

// maxClassRunes is the largest character class expanded into exact strings.
const maxClassRunes = 16

func analyze(re *syntax.Regexp) info {
	switch re.Op {
	case syntax.OpNoMatch:
		return info{exact: map[string]struct{}{}, match: allQuery}
	case syntax.OpEmptyMatch, syntax.OpBeginLine, syntax.OpEndLine,
		syntax.OpBeginText, syntax.OpEndText, syntax.OpWordBoundary, syntax.OpNoWordBoundary:
		return exactInfo("")
	case syntax.OpLiteral:
		acc := exactInfo("")
		for _, r := range re.Rune {
			var next info
			if re.Flags&syntax.FoldCase != 0 {
				next = runeSet(foldOrbit(r))
			} else {
				next = exactInfo(string(r))
			}
			acc = concatInfo(acc, next)
		}
		return acc
	case syntax.OpCharClass:
		var rs []rune
		for i := 0; i+1 < len(re.Rune); i += 2 {
			lo, hi := re.Rune[i], re.Rune[i+1]
			if int(hi-lo)+1+len(rs) > maxClassRunes {
				return anyInfo()
			}
			for r := lo; r <= hi; r++ {
				if !utf8.ValidRune(r) {
					return anyInfo()
				}
				rs = append(rs, r)
			}
		}
		if len(rs) == 0 {
			return info{exact: map[string]struct{}{}, match: allQuery}
		}
		return runeSet(rs)
	case syntax.OpAnyChar, syntax.OpAnyCharNotNL:
		return anyInfo()
	case syntax.OpCapture:
		return analyze(re.Sub[0])
	case syntax.OpStar, syntax.OpQuest:
		return anyInfo()
	case syntax.OpPlus:
		return info{match: analyze(re.Sub[0]).query()}
	case syntax.OpRepeat:
		if re.Min == 0 {
			return anyInfo()
		}
		sub := analyze(re.Sub[0])
		if re.Min == 1 && re.Max == 1 {
			return sub
		}
		return info{match: sub.query()}
	case syntax.OpConcat:
		acc := exactInfo("")
		for _, sub := range re.Sub {
			acc = concatInfo(acc, analyze(sub))
		}
		return acc
	case syntax.OpAlternate:
		if len(re.Sub) == 0 {
			return info{exact: map[string]struct{}{}, match: allQuery}
		}
		acc := analyze(re.Sub[0])
		for _, sub := range re.Sub[1:] {
			acc = alternateInfo(acc, analyze(sub))
		}
		return acc
	}
	return anyInfo()
}

// regexpQuery returns the trigram prefilter of a parsed expression.
func regexpQuery(re *syntax.Regexp) *tquery {
	return analyze(re.Simplify()).query()
}
