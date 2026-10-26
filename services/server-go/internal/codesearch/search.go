package codesearch

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"path"
	"regexp"
	"regexp/syntax"
	"sort"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
)

// Query limits (docs/ENGINEERING_PLAN.md §14.3).
const (
	DefaultLimit   = 50
	MaxLimit       = 1000
	DefaultContext = 2
	MaxContext     = 10
	// MinLiteral is the shortest literal query, in characters.
	MinLiteral = 3
	// MaxPattern bounds the query length, in bytes.
	MaxPattern = 1000
	// Budget is the time a query may take; hits found until then are
	// returned with Truncated set.
	Budget = 5 * time.Second
	// MaxLineLen bounds the text of a returned line (bytes, cut at a
	// character boundary).
	MaxLineLen = 1000
)

// ErrInvalidQuery is matched (errors.Is) by every query validation error;
// so is domain.ErrInvalid. The error text is the client-facing message.
var ErrInvalidQuery = errors.New("invalid code search query")

type queryError struct{ msg string }

func (e *queryError) Error() string { return e.msg }

func (e *queryError) Is(target error) bool {
	return target == ErrInvalidQuery || target == domain.ErrInvalid
}

func invalidf(format string, args ...any) error {
	return &queryError{msg: fmt.Sprintf(format, args...)}
}

// Query is one code search.
type Query struct {
	// Pattern is the literal text (at least MinLiteral characters) or, with
	// Regex, an RE2 expression. Matching is per line.
	Pattern string
	Regex   bool
	// CaseSensitive disables Unicode case folding.
	CaseSensitive bool
	// Path restricts the files searched (see MatchPath); "" = every file.
	Path string
	// Context is the number of lines returned before and after each hit
	// (0..MaxContext).
	Context int
	// Limit is the maximum number of hits (1..MaxLimit; 0 = DefaultLimit).
	Limit int
	// Budget overrides the time budget (0 = Budget).
	Budget time.Duration
	// Branch searches this branch instead of the default branch (trees
	// without it contribute nothing); AllBranches searches every branch
	// tip (up to Options.MaxBranches per tree, §15.9). At most one is set.
	Branch      string
	AllBranches bool
}

// Target is one tree to search; the caller has checked tree:read.
type Target struct {
	TenantSlug string
	Tree       *domain.Tree
}

// ReadFilter reports which of paths (files of tree with matches) the
// caller may not read; their hits are dropped. A nil function restricts
// nothing. An error fails the search.
type ReadFilter func(ctx context.Context, tree *domain.Tree, paths []string) (restricted func(path string) bool, err error)

// Result is the answer to a search.
type Result struct {
	Hits          []domain.CodeSearchHit `json:"hits"`
	Truncated     bool                   `json:"truncated"`
	SearchedTrees int                    `json:"searched_trees"`
}

// matcher is a compiled query.
type matcher struct {
	lit    []byte         // case-sensitive literal search
	re     *regexp.Regexp // everything else
	filter *tquery
}

func (m *matcher) matchLine(line []byte) bool {
	if m.re == nil {
		return bytes.Contains(line, m.lit)
	}
	return m.re.Match(line)
}

// mayMatchFile is a cheap whole-file check before the line scan. Only
// literal searches use it: regular expressions are matched per line
// (anchors and "\r\n" endings make a whole-file match unreliable).
func (m *matcher) mayMatchFile(data []byte) bool {
	return m.re != nil || bytes.Contains(data, m.lit)
}

// compile validates q and builds its matcher.
func compile(q Query) (*matcher, error) {
	if q.Pattern == "" {
		return nil, invalidf("q is required")
	}
	if len(q.Pattern) > MaxPattern {
		return nil, invalidf("q must be at most %d bytes", MaxPattern)
	}
	if !utf8.ValidString(q.Pattern) {
		return nil, invalidf("q must be valid UTF-8")
	}
	if !q.Regex {
		if utf8.RuneCountInString(q.Pattern) < MinLiteral {
			return nil, invalidf("q must be at least %d characters", MinLiteral)
		}
		if strings.ContainsAny(q.Pattern, "\r\n") {
			return nil, invalidf("q must not contain line breaks")
		}
	}
	if q.Context < 0 || q.Context > MaxContext {
		return nil, invalidf("context must be 0-%d", MaxContext)
	}
	if q.Path != "" && !validGlob(q.Path) {
		return nil, invalidf("path %q is not a valid glob", q.Path)
	}
	if err := validBranchScope(q.Branch, q.AllBranches); err != nil {
		return nil, err
	}
	if !q.Regex && q.CaseSensitive {
		lit := []byte(q.Pattern)
		return &matcher{lit: lit, filter: stringQuery(q.Pattern)}, nil
	}
	expr := q.Pattern
	if !q.Regex {
		expr = regexp.QuoteMeta(expr)
	}
	flags := syntax.Perl
	if !q.CaseSensitive {
		flags |= syntax.FoldCase
	}
	parsed, err := syntax.Parse(expr, flags)
	if err != nil {
		return nil, invalidf("%v", err)
	}
	prefix := ""
	if !q.CaseSensitive {
		prefix = "(?i)"
	}
	re, err := regexp.Compile(prefix + expr)
	if err != nil {
		return nil, invalidf("%v", err)
	}
	return &matcher{re: re, filter: regexpQuery(parsed)}, nil
}

// validGlob reports whether pattern is a usable path glob.
func validGlob(pattern string) bool {
	if len(pattern) > 1024 || strings.ContainsRune(pattern, 0) {
		return false
	}
	for _, seg := range strings.Split(strings.ReplaceAll(pattern, `\`, "/"), "/") {
		if seg == "**" {
			continue
		}
		if _, err := path.Match(seg, ""); err != nil {
			return false
		}
	}
	return true
}

// MatchPath reports whether the path glob covers path. The syntax is the
// license-rule glob ("**" any depth, "*", "?", "[...]"; anchored at the
// tree root; a pattern naming a directory covers everything beneath it),
// except that a pattern without "/" matches the file name at any depth
// ("*.go" = "**/*.go").
func MatchPath(pattern, path string) bool {
	if pattern == "" {
		return true
	}
	if license.Match(pattern, path) {
		return true
	}
	if !strings.Contains(strings.Trim(pattern, "/"), "/") {
		return license.Match("**/"+strings.Trim(pattern, "/"), path)
	}
	return false
}

// fileHits is the matches of one file.
type fileHits struct {
	path string
	hits []domain.CodeSearchHit
}

// errLimit stops a search once one hit more than the limit was found.
var errLimit = errors.New("codesearch: limit reached")

// Search runs q over targets. Hits are ordered by tenant, tree, branch
// (default branch first, then by name; a file version already searched on
// an earlier branch is skipped), path and
// line. When more than Limit hits exist, or the time budget runs out, the
// hits found so far are returned with Truncated set.
func (ix *Index) Search(ctx context.Context, q Query, targets []Target, filter ReadFilter) (*Result, error) {
	m, err := compile(q)
	if err != nil {
		return nil, err
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

	targets = append([]Target(nil), targets...)
	sort.SliceStable(targets, func(i, j int) bool {
		if targets[i].TenantSlug != targets[j].TenantSlug {
			return targets[i].TenantSlug < targets[j].TenantSlug
		}
		return targets[i].Tree.Name < targets[j].Tree.Name
	})
	res := &Result{Hits: []domain.CodeSearchHit{}}
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
			ix.d.Logger.WarnContext(ctx, "codesearch: indexing failed; tree skipped",
				"tree", t.Tree.ID, "err", err)
			continue
		}
		res.SearchedTrees++
		seen := map[docKey]bool{}
		for _, v := range views {
			err = ix.searchView(ctx, m, q, t, v, seen, filter, limit, res)
			switch {
			case errors.Is(err, errLimit):
				res.Truncated = true
				return res, nil
			case err != nil && ctx.Err() != nil:
				res.Truncated = true
				return res, nil
			case err != nil:
				return nil, err
			}
		}
	}
	return res, nil
}

// validBranchScope checks the branch selection of a query.
func validBranchScope(branch string, all bool) error {
	if branch != "" && all {
		return invalidf("branch and branches=all are mutually exclusive")
	}
	if len(branch) > 255 || strings.ContainsAny(branch, "\x00\r\n") {
		return invalidf("branch %q is not a valid branch name", branch)
	}
	return nil
}

// views returns the branch tips of tree a query covers, in search order:
// the default branch (or the named branch), or with all every branch up
// to MaxBranches — the default branch first, then the most recently
// updated ones — ordered default first, then by name.
func (ix *Index) views(ctx context.Context, tree *domain.Tree, branch string, all bool) ([]*view, error) {
	var names []string
	switch {
	case branch != "":
		names = []string{branch}
	case !all:
		names = []string{tree.DefaultBranch}
	default:
		bs, err := ix.d.Store.ListBranches(ctx, tree.ID)
		if err != nil {
			return nil, err
		}
		sort.SliceStable(bs, func(i, j int) bool {
			if (bs[i].Name == tree.DefaultBranch) != (bs[j].Name == tree.DefaultBranch) {
				return bs[i].Name == tree.DefaultBranch
			}
			if !bs[i].UpdatedAt.Equal(bs[j].UpdatedAt) {
				return bs[i].UpdatedAt.After(bs[j].UpdatedAt)
			}
			return bs[i].Name < bs[j].Name
		})
		ix.mu.Lock()
		maxBranches := ix.opt.MaxBranches
		ix.mu.Unlock()
		for _, b := range bs[:min(len(bs), maxBranches)] {
			names = append(names, b.Name)
		}
		if len(names) == 0 {
			names = []string{tree.DefaultBranch}
		}
		sort.SliceStable(names, func(i, j int) bool {
			if (names[i] == tree.DefaultBranch) != (names[j] == tree.DefaultBranch) {
				return names[i] == tree.DefaultBranch
			}
			return names[i] < names[j]
		})
	}
	var out []*view
	var firstErr error
	for _, name := range names {
		v, err := ix.ensureBranch(ctx, tree, name)
		if err != nil {
			if ctx.Err() != nil {
				return nil, err
			}
			if firstErr == nil {
				firstErr = err
			}
			ix.d.Logger.WarnContext(ctx, "codesearch: indexing a branch failed; branch skipped",
				"tree", tree.ID, "branch", name, "err", err)
			continue
		}
		if v != nil {
			out = append(out, v)
		}
	}
	if len(out) == 0 && firstErr != nil {
		return nil, firstErr
	}
	return out, nil
}

// docKey identifies a file version across the branches of a tree: hits in
// a file already searched on an earlier branch are not reported again.
type docKey struct {
	path string
	hash hash.Hash
}

// filterBatch is how many matching files are permission-checked at once.
const filterBatch = 64

func (ix *Index) searchView(ctx context.Context, m *matcher, q Query, t Target, v *view, seen map[docKey]bool,
	filter ReadFilter, limit int, res *Result) error {
	if v.seg == nil {
		return nil
	}
	idx := v.seg
	var batch []fileHits
	flush := func() error {
		if len(batch) == 0 {
			return nil
		}
		var restricted func(string) bool
		if filter != nil {
			paths := make([]string, len(batch))
			for i, f := range batch {
				paths[i] = f.path
			}
			var err error
			if restricted, err = filter(ctx, t.Tree, paths); err != nil {
				return err
			}
		}
		for _, f := range batch {
			if restricted != nil && restricted(f.path) {
				continue
			}
			for _, h := range f.hits {
				if len(res.Hits) == limit {
					return errLimit
				}
				res.Hits = append(res.Hits, h)
			}
		}
		batch = batch[:0]
		return nil
	}
	pending := 0
	for _, id := range idx.candidates(m.filter) {
		if err := ctx.Err(); err != nil {
			return err
		}
		d := &idx.docs[id]
		k := docKey{d.path, d.hash}
		if seen[k] {
			continue
		}
		seen[k] = true
		if !MatchPath(q.Path, d.path) || !m.mayMatchFile(d.content) {
			continue
		}
		hits := scanDoc(m, d, q.Context, limit+1-len(res.Hits)-pending)
		if len(hits) == 0 {
			continue
		}
		for i := range hits {
			hits[i].TenantSlug, hits[i].Tree = t.TenantSlug, t.Tree.Name
			hits[i].Branch, hits[i].SnapshotID = v.branch, v.snapshotID
		}
		batch = append(batch, fileHits{path: d.path, hits: hits})
		pending += len(hits)
		if len(batch) == filterBatch || len(res.Hits)+pending > limit {
			if err := flush(); err != nil {
				return err
			}
			pending = 0
		}
	}
	return flush()
}

// scanDoc returns up to limit hits of d (limit < 1 still returns one, so
// that a limit overflow is detected).
func scanDoc(m *matcher, d *segDoc, ctxLines, limit int) []domain.CodeSearchHit {
	limit = max(limit, 1)
	lines := splitLines(d.content)
	var out []domain.CodeSearchHit
	for i, line := range lines {
		if !m.matchLine(line) {
			continue
		}
		h := domain.CodeSearchHit{Path: d.path, Line: i + 1, Text: clip(line)}
		for j := max(i-ctxLines, 0); j < i; j++ {
			h.Before = append(h.Before, clip(lines[j]))
		}
		for j := i + 1; j <= i+ctxLines && j < len(lines); j++ {
			h.After = append(h.After, clip(lines[j]))
		}
		out = append(out, h)
		if len(out) == limit {
			break
		}
	}
	return out
}

// splitLines splits data at "\n", dropping a trailing "\r" from each line
// and the empty line after a final newline.
func splitLines(data []byte) [][]byte {
	lines := bytes.Split(data, []byte{'\n'})
	if n := len(lines); n > 0 && len(lines[n-1]) == 0 {
		lines = lines[:n-1]
	}
	for i, l := range lines {
		lines[i] = bytes.TrimSuffix(l, []byte{'\r'})
	}
	return lines
}

// clip returns line as a string of at most MaxLineLen bytes, cut at a
// character boundary.
func clip(line []byte) string {
	if len(line) <= MaxLineLen {
		return string(line)
	}
	cut := MaxLineLen
	for cut > 0 && !utf8.RuneStart(line[cut]) {
		cut--
	}
	return string(line[:cut])
}
