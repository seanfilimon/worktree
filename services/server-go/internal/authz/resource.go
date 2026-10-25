package authz

import (
	"fmt"
	"strings"
)

// Resource kinds of policy resource strings ("wt:<kind>/...").
const (
	kindTenant = "tenant"
	kindTree   = "tree"
	kindBranch = "branch"
	kindPath   = "path"
)

// resourcePrefix starts every resource string except the bare "*".
const resourcePrefix = "wt:"

// minSegments / maxSegments bound the number of '/'-separated segments
// after "wt:<kind>/" for a concrete resource. Branch names and paths may
// contain '/', so they have no upper bound.
var (
	minSegments = map[string]int{kindTenant: 1, kindTree: 2, kindBranch: 3, kindPath: 3}
	maxSegments = map[string]int{kindTenant: 1, kindTree: 2, kindBranch: -1, kindPath: -1}
)

// TenantResource returns "wt:tenant/<tenant>".
func TenantResource(tenant string) string { return "wt:tenant/" + tenant }

// TreeResource returns "wt:tree/<tenant>/<tree>".
func TreeResource(tenant, tree string) string { return "wt:tree/" + tenant + "/" + tree }

// BranchResource returns "wt:branch/<tenant>/<tree>/<branch>".
func BranchResource(tenant, tree, branch string) string {
	return "wt:branch/" + tenant + "/" + tree + "/" + branch
}

// PathResource returns "wt:path/<tenant>/<tree>/<path>" (leading and
// trailing slashes of path are dropped).
func PathResource(tenant, tree, path string) string {
	return "wt:path/" + tenant + "/" + tree + "/" + strings.Trim(path, "/")
}

// parsedResource is a resource string split into kind and segments.
type parsedResource struct {
	any  bool // the bare "*"
	kind string
	segs []string
}

// parseResource splits a resource string or pattern. It does not check
// segment counts (see validateResourcePattern).
func parseResource(s string) (parsedResource, error) {
	s = strings.TrimSpace(s)
	if s == "*" {
		return parsedResource{any: true}, nil
	}
	rest, ok := strings.CutPrefix(s, resourcePrefix)
	if !ok {
		return parsedResource{}, fmt.Errorf("resource %q must be \"*\" or start with %q", s, resourcePrefix)
	}
	kind, body, ok := strings.Cut(rest, "/")
	if !ok || body == "" {
		return parsedResource{}, fmt.Errorf("resource %q has no path after the kind", s)
	}
	if _, known := minSegments[kind]; !known {
		return parsedResource{}, fmt.Errorf("resource %q has unknown kind %q (want tenant, tree, branch or path)", s, kind)
	}
	segs := strings.Split(body, "/")
	for _, seg := range segs {
		if seg == "" {
			return parsedResource{}, fmt.Errorf("resource %q has an empty segment", s)
		}
	}
	return parsedResource{kind: kind, segs: segs}, nil
}

// validateResourcePattern checks a policy resource pattern.
func validateResourcePattern(s string) error {
	r, err := parseResource(s)
	if err != nil || r.any {
		return err
	}
	fixed, globstar := 0, false
	for _, seg := range r.segs {
		if seg == "**" {
			globstar = true
			continue
		}
		if strings.Contains(seg, "**") {
			return fmt.Errorf("resource %q: \"**\" must be a whole segment", s)
		}
		fixed++
	}
	lo, hi := minSegments[r.kind], maxSegments[r.kind]
	if hi >= 0 && fixed > hi {
		return fmt.Errorf("resource %q: a %s resource has at most %d segment(s)", s, r.kind, hi)
	}
	if !globstar && fixed < lo {
		return fmt.Errorf("resource %q: a %s resource needs at least %d segment(s)", s, r.kind, lo)
	}
	return nil
}

// resourcePatternMatches reports whether pattern covers the concrete
// resource string res. "*" in a segment matches any characters within that
// segment; a "**" segment matches zero or more whole segments.
func resourcePatternMatches(pattern parsedResource, res parsedResource) bool {
	if pattern.any {
		return true
	}
	return pattern.kind == res.kind && matchSegments(pattern.segs, res.segs)
}

// matchSegments matches '/'-split segments with "**" support.
func matchSegments(pat, name []string) bool {
	for len(pat) > 0 {
		if pat[0] == "**" {
			// Collapse consecutive globstars, then try every split point.
			for len(pat) > 0 && pat[0] == "**" {
				pat = pat[1:]
			}
			if len(pat) == 0 {
				return true
			}
			for i := 0; i <= len(name); i++ {
				if matchSegments(pat, name[i:]) {
					return true
				}
			}
			return false
		}
		if len(name) == 0 || !globMatch(pat[0], name[0]) {
			return false
		}
		pat, name = pat[1:], name[1:]
	}
	return len(name) == 0
}

// globMatch matches s against a pattern in which '*' matches any run of
// characters (including none). No other metacharacters exist, so every
// pattern is valid.
func globMatch(pattern, s string) bool {
	if !strings.Contains(pattern, "*") {
		return pattern == s
	}
	parts := strings.Split(pattern, "*")
	if !strings.HasPrefix(s, parts[0]) {
		return false
	}
	s = s[len(parts[0]):]
	last := parts[len(parts)-1]
	for _, mid := range parts[1 : len(parts)-1] {
		i := strings.Index(s, mid)
		if i < 0 {
			return false
		}
		s = s[i+len(mid):]
	}
	return strings.HasSuffix(s, last) && len(s) >= len(last)
}
