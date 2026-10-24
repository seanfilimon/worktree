package license

import (
	"fmt"
	"path"
	"sort"
	"strings"
)

// Glob syntax
//
// A LicenseRule.PathGlob is a '/'-separated pattern matched against
// tree-relative file paths ('\' is accepted and treated as '/'; a leading
// "/" or "./" is ignored). Patterns are anchored at the tree root.
//
//   - "**" as a whole segment matches zero or more path segments
//     ("**/*.rs" matches "a.rs" and "src/x/a.rs");
//   - "*" matches any run of characters within one segment;
//   - "?" matches exactly one character within one segment;
//   - "[...]" character classes follow path.Match;
//   - any other character matches itself (there are no escapes).
//
// A pattern that matches a directory applies to everything beneath it, so
// "vendor", "vendor/" and "vendor/**" are equivalent, and "services/*"
// covers "services/billing/src/main.rs". A pattern without a slash is not
// matched at every depth (unlike gitignore): "*.rs" covers top-level files
// only; use "**/*.rs" for any depth.
//
// Specificity
//
// When several rules match a path, the most specific wins:
//
//  1. the longest literal prefix (characters before the first wildcard;
//     a "/" directly preceding a "**" segment is not counted, so that
//     "vendor/**" ranks like "vendor");
//  2. then the fewest wildcards ("**" segment, "*" run, "?" or "[...]"
//     each count as one);
//  3. then the most literal characters overall;
//  4. then the lexically smallest normalized pattern (deterministic
//     tie-break).
//
// Consequently an exact file path beats its directory, a deeper directory
// beats a shallower one, and "src/*.rs" beats "src/**".

// glob is a compiled path pattern.
type glob struct {
	raw      string   // pattern as given
	norm     string   // normalized pattern
	segs     []string // normalized segments; "**" for any-depth
	prefix   int      // literal prefix length (specificity rule 1)
	wild     int      // wildcard count (rule 2)
	literals int      // literal character count (rule 3)
}

const maxGlobLen = 4096

// normalizeGlob converts separators, strips leading "/" and "./", turns a
// trailing "/" into "/**", drops empty segments and collapses repeated
// "**" segments.
func normalizeGlob(s string) []string {
	s = strings.ReplaceAll(s, `\`, "/")
	for {
		switch {
		case strings.HasPrefix(s, "./"):
			s = s[2:]
			continue
		case strings.HasPrefix(s, "/"):
			s = s[1:]
			continue
		}
		break
	}
	dir := strings.HasSuffix(s, "/")
	var segs []string
	for _, seg := range strings.Split(s, "/") {
		if seg == "" || seg == "." {
			continue
		}
		if seg == "**" && len(segs) > 0 && segs[len(segs)-1] == "**" {
			continue
		}
		segs = append(segs, seg)
	}
	if dir && len(segs) > 0 && segs[len(segs)-1] != "**" {
		segs = append(segs, "**")
	}
	if len(segs) == 0 {
		segs = []string{"**"}
	}
	return segs
}

// compileGlob validates and compiles a pattern.
func compileGlob(raw string) (*glob, error) {
	if strings.TrimSpace(raw) == "" {
		return nil, fmt.Errorf("path glob is empty")
	}
	if len(raw) > maxGlobLen {
		return nil, fmt.Errorf("path glob longer than %d bytes", maxGlobLen)
	}
	if strings.ContainsRune(raw, 0) {
		return nil, fmt.Errorf("path glob contains a NUL byte")
	}
	segs := normalizeGlob(raw)
	for _, seg := range segs {
		if seg == ".." {
			return nil, fmt.Errorf("path glob %q must not contain '..'", raw)
		}
		if seg == "**" {
			continue
		}
		if _, err := path.Match(seg, ""); err != nil {
			return nil, fmt.Errorf("path glob %q: malformed segment %q: %v", raw, seg, err)
		}
	}
	g := &glob{raw: raw, norm: strings.Join(segs, "/"), segs: segs}
	g.prefix, g.wild, g.literals = specificity(g.norm)
	return g, nil
}

// specificity computes the ranking keys described in the package docs.
func specificity(norm string) (prefix, wild, literals int) {
	i := strings.IndexAny(norm, "*?[")
	if i < 0 {
		prefix = len(norm)
	} else {
		prefix = i
		// "a/**" ranks like "a".
		if strings.HasPrefix(norm[i:], "**") && i > 0 && norm[i-1] == '/' {
			prefix--
		}
	}
	for _, seg := range strings.Split(norm, "/") {
		if seg == "**" {
			wild++
			continue
		}
		inStar := false
		for j := 0; j < len(seg); j++ {
			switch seg[j] {
			case '*':
				if !inStar {
					wild++
				}
				inStar = true
				continue
			case '?':
				wild++
			case '[':
				wild++
				if k := strings.IndexByte(seg[j+1:], ']'); k >= 0 {
					j += k + 1
				}
			default:
				literals++
			}
			inStar = false
		}
	}
	return prefix, wild, literals
}

// moreSpecific reports whether a ranks before b.
func moreSpecific(a, b *glob) bool {
	if a.prefix != b.prefix {
		return a.prefix > b.prefix
	}
	if a.wild != b.wild {
		return a.wild < b.wild
	}
	if a.literals != b.literals {
		return a.literals > b.literals
	}
	if a.norm != b.norm {
		return a.norm < b.norm
	}
	return a.raw < b.raw
}

func sortBySpecificity[T any](xs []T, g func(T) *glob) {
	sort.SliceStable(xs, func(i, j int) bool { return moreSpecific(g(xs[i]), g(xs[j])) })
}

// match reports whether the pattern covers the (normalized) path segments:
// the pattern matches the path itself or one of its ancestor directories.
func (g *glob) match(p []string) bool {
	np, ns := len(g.segs), len(p)
	// memo[i*(ns+1)+j]: 0 unknown, 1 false, 2 true.
	memo := make([]uint8, (np+1)*(ns+1))
	var m func(i, j int) bool
	m = func(i, j int) bool {
		if i == np {
			return true // pattern consumed: path itself or a descendant
		}
		k := i*(ns+1) + j
		if memo[k] != 0 {
			return memo[k] == 2
		}
		var ok bool
		if g.segs[i] == "**" {
			ok = m(i+1, j) || (j < ns && m(i, j+1))
		} else if j < ns {
			matched, _ := path.Match(g.segs[i], p[j]) // validated at compile time
			ok = matched && m(i+1, j+1)
		}
		if ok {
			memo[k] = 2
		} else {
			memo[k] = 1
		}
		return ok
	}
	return m(0, 0)
}

// splitPath normalizes a tree-relative path into segments. It reports false
// for paths that are empty or escape the tree root.
func splitPath(p string) ([]string, bool) {
	p = strings.ReplaceAll(p, `\`, "/")
	p = strings.TrimLeft(p, "/")
	if p == "" {
		return nil, false
	}
	p = path.Clean(p)
	if p == "." || p == ".." || strings.HasPrefix(p, "../") {
		return nil, false
	}
	return strings.Split(p, "/"), true
}

// Match reports whether pattern (see "Glob syntax" above) covers the
// tree-relative path p. Malformed patterns and paths escaping the root
// never match.
func Match(pattern, p string) bool {
	g, err := compileGlob(pattern)
	if err != nil {
		return false
	}
	segs, ok := splitPath(p)
	if !ok {
		return false
	}
	return g.match(segs)
}
