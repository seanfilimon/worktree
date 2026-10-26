package codesearch

import (
	"context"
	"sort"
	"strings"
	"time"
	"unicode/utf8"
)

// Symbol search (docs/ENGINEERING_PLAN.md §15.9).

// MaxSymbolQuery bounds a symbol query (bytes).
const MaxSymbolQuery = 200

// Match ranks of a symbol hit, best first.
const (
	MatchExact     = "exact"
	MatchPrefix    = "prefix"
	MatchSubstring = "substring"
)

// SymbolQuery is one symbol search. Names are matched case-insensitively:
// exact matches rank above prefix matches above substring matches (an
// exact match in the query's case ranks first).
type SymbolQuery struct {
	// Query is the name or part of a name (1..MaxSymbolQuery bytes).
	Query string
	// Kinds restricts the kinds returned (empty = every kind).
	Kinds []SymbolKind
	// Path restricts the files searched (see MatchPath).
	Path string
	// Branch / AllBranches select the branches as in Query.
	Branch      string
	AllBranches bool
	// Limit is the maximum number of symbols (1..MaxLimit; 0 = DefaultLimit).
	Limit int
	// Budget overrides the time budget (0 = Budget).
	Budget time.Duration
}

// SymbolHit is one symbol found by a symbol search.
type SymbolHit struct {
	TenantSlug string     `json:"tenant"`
	Tree       string     `json:"tree"`
	Branch     string     `json:"branch"`
	SnapshotID string     `json:"snapshot_id"`
	Path       string     `json:"path"`
	Line       int        `json:"line"` // 1-based
	Name       string     `json:"name"`
	Kind       SymbolKind `json:"kind"` // function, method, class, struct, interface, trait, enum, type or const
	// Container is the enclosing type of a method (or nested declaration).
	Container string `json:"container,omitempty"`
	// Signature is the defining line, trimmed (at most 200 bytes).
	Signature string `json:"signature"`
	Language  string `json:"language"`
	// Match is how the name matched the query: exact, prefix or substring.
	Match string `json:"match"`
}

// SymbolResult is the answer to a symbol search. Symbols are ranked
// (exact, prefix, substring; then shorter names first) and then ordered by
// name, tenant, tree, branch, path and line.
type SymbolResult struct {
	Symbols []SymbolHit `json:"symbols"`
	// Truncated reports that more than limit symbols matched, or the time
	// budget ran out.
	Truncated     bool `json:"truncated"`
	SearchedTrees int  `json:"searched_trees"`
}

// ParseKinds parses a comma-separated kind list ("" = every kind).
func ParseKinds(s string) ([]SymbolKind, error) {
	var out []SymbolKind
	for _, part := range strings.Split(s, ",") {
		part = strings.ToLower(strings.TrimSpace(part))
		if part == "" {
			continue
		}
		k := SymbolKind(part)
		if !ValidKind(k) {
			names := make([]string, len(SymbolKinds))
			for i, k := range SymbolKinds {
				names[i] = string(k)
			}
			return nil, invalidf("kind %q is not one of %s", part, strings.Join(names, ", "))
		}
		out = append(out, k)
	}
	return out, nil
}

// symCand is a symbol matching a query, before permission filtering.
type symCand struct {
	hit      SymbolHit
	rank     int
	branchIx int
}

func symLess(a, b *symCand) bool {
	if a.rank != b.rank {
		return a.rank < b.rank
	}
	if len(a.hit.Name) != len(b.hit.Name) {
		return len(a.hit.Name) < len(b.hit.Name)
	}
	if a.hit.Name != b.hit.Name {
		return a.hit.Name < b.hit.Name
	}
	if a.hit.TenantSlug != b.hit.TenantSlug {
		return a.hit.TenantSlug < b.hit.TenantSlug
	}
	if a.hit.Tree != b.hit.Tree {
		return a.hit.Tree < b.hit.Tree
	}
	if a.branchIx != b.branchIx {
		return a.branchIx < b.branchIx
	}
	if a.hit.Path != b.hit.Path {
		return a.hit.Path < b.hit.Path
	}
	return a.hit.Line < b.hit.Line
}

// SearchSymbols finds symbol definitions in targets.
func (ix *Index) SearchSymbols(ctx context.Context, q SymbolQuery, targets []Target, filter ReadFilter) (*SymbolResult, error) {
	needle := strings.TrimSpace(q.Query)
	switch {
	case needle == "":
		return nil, invalidf("q is required")
	case len(needle) > MaxSymbolQuery:
		return nil, invalidf("q must be at most %d bytes", MaxSymbolQuery)
	case !utf8.ValidString(needle):
		return nil, invalidf("q must be valid UTF-8")
	}
	if q.Path != "" && !validGlob(q.Path) {
		return nil, invalidf("path %q is not a valid glob", q.Path)
	}
	if err := validBranchScope(q.Branch, q.AllBranches); err != nil {
		return nil, err
	}
	kinds := map[SymbolKind]bool{}
	for _, k := range q.Kinds {
		if !ValidKind(k) {
			return nil, invalidf("unknown symbol kind %q", k)
		}
		kinds[k] = true
	}
	limit := q.Limit
	if limit <= 0 {
		limit = DefaultLimit
	}
	limit = min(limit, MaxLimit)
	budget := q.Budget
	if budget <= 0 {
		budget = Budget
	}
	ctx, cancel := context.WithTimeout(ctx, budget)
	defer cancel()

	lower := strings.ToLower(needle)
	res := &SymbolResult{Symbols: []SymbolHit{}}
	var all []*symCand
	for _, t := range targets {
		if ctx.Err() != nil {
			res.Truncated = true
			break
		}
		views, err := ix.views(ctx, t.Tree, q.Branch, q.AllBranches)
		if err != nil {
			if ctx.Err() != nil {
				res.Truncated = true
				break
			}
			ix.d.Logger.WarnContext(ctx, "codesearch: indexing failed; tree skipped", "tree", t.Tree.ID, "err", err)
			continue
		}
		res.SearchedTrees++
		cands := treeSymbols(views, t, needle, lower, kinds, q.Path)
		kept, more, err := filterSymbols(ctx, t, cands, filter, limit)
		if err != nil {
			if ctx.Err() != nil {
				res.Truncated = true
				break
			}
			return nil, err
		}
		if more {
			res.Truncated = true
		}
		all = append(all, kept...)
	}
	sort.Slice(all, func(i, j int) bool { return symLess(all[i], all[j]) })
	if len(all) > limit {
		all, res.Truncated = all[:limit], true
	}
	for _, c := range all {
		res.Symbols = append(res.Symbols, c.hit)
	}
	return res, nil
}

// treeSymbols returns the symbols of a tree's views matching the query,
// ranked. A file version seen on an earlier branch is skipped.
func treeSymbols(views []*view, t Target, needle, lower string, kinds map[SymbolKind]bool, pathGlob string) []*symCand {
	seen := map[docKey]bool{}
	var out []*symCand
	for bi, v := range views {
		if v.seg == nil {
			continue
		}
		s := v.seg
		docOK := make([]int8, len(s.docs)) // 0 unknown, 1 searched here, -1 skip
		consider := func(i, rank int) {
			sym := &s.syms[i]
			if len(kinds) > 0 && !kinds[sym.Kind] {
				return
			}
			d := &s.docs[sym.doc]
			switch docOK[sym.doc] {
			case 0:
				k := docKey{d.path, d.hash}
				if seen[k] || !MatchPath(pathGlob, d.path) {
					docOK[sym.doc] = -1
					return
				}
				docOK[sym.doc] = 1
			case -1:
				return
			}
			if rank == 1 && sym.Name == needle {
				rank = 0
			}
			out = append(out, &symCand{rank: rank, branchIx: bi, hit: SymbolHit{
				TenantSlug: t.TenantSlug, Tree: t.Tree.Name, Branch: v.branch, SnapshotID: v.snapshotID,
				Path: d.path, Line: sym.Line, Name: sym.Name, Kind: sym.Kind, Container: sym.Container,
				Signature: sym.Signature, Language: Language(d.path), Match: matchName(rank),
			}})
		}
		// Exact and prefix matches are a contiguous run of the sorted names.
		start := sort.SearchStrings(s.lnames, lower)
		end := start
		for end < len(s.lnames) && strings.HasPrefix(s.lnames[end], lower) {
			rank := 2
			if s.lnames[end] == lower {
				rank = 1
			}
			consider(end, rank)
			end++
		}
		for i, n := range s.lnames {
			if (i < start || i >= end) && strings.Contains(n, lower) {
				consider(i, 3)
			}
		}
		// Every file version of this branch is now seen.
		for i := range s.docs {
			seen[docKey{s.docs[i].path, s.docs[i].hash}] = true
		}
	}
	sort.Slice(out, func(i, j int) bool { return symLess(out[i], out[j]) })
	return out
}

func matchName(rank int) string {
	switch rank {
	case 0, 1:
		return MatchExact
	case 2:
		return MatchPrefix
	}
	return MatchSubstring
}

// filterSymbols walks ranked candidates, dropping those in paths the
// caller may not read, until limit+1 survivors are found. more reports
// that more than limit survived.
func filterSymbols(ctx context.Context, t Target, cands []*symCand, filter ReadFilter, limit int) (kept []*symCand, more bool, err error) {
	if filter == nil {
		if len(cands) > limit {
			return cands[:limit+1], true, nil
		}
		return cands, false, nil
	}
	checked := map[string]bool{} // path -> restricted
	for i := 0; i < len(cands) && len(kept) <= limit; {
		if err := ctx.Err(); err != nil {
			return nil, false, err
		}
		// Collect the next batch of unchecked paths.
		var paths []string
		inBatch := map[string]bool{}
		for j := i; j < len(cands) && len(paths) < filterBatch; j++ {
			p := cands[j].hit.Path
			if _, done := checked[p]; !done && !inBatch[p] {
				inBatch[p] = true
				paths = append(paths, p)
			}
		}
		if len(paths) > 0 {
			restricted, err := filter(ctx, t.Tree, paths)
			if err != nil {
				return nil, false, err
			}
			for _, p := range paths {
				checked[p] = restricted != nil && restricted(p)
			}
		}
		for ; i < len(cands) && len(kept) <= limit; i++ {
			r, done := checked[cands[i].hit.Path]
			if !done {
				break // next batch
			}
			if !r {
				kept = append(kept, cands[i])
			}
		}
	}
	return kept, len(kept) > limit, nil
}
