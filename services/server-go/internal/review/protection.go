package review

import (
	"errors"
	"fmt"
	"path"
	"slices"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// globstar is the pattern segment that matches any number of branch-name
// segments.
const globstar = "**"

// Effective is the protection that applies to one branch: every
// domain.BranchProtection rule whose Pattern matches the branch, merged into
// the most restrictive combination (the spec's "ceiling model": a rule can
// only add restrictions, never relax another rule).
type Effective struct {
	// Branch is the branch the protection was resolved for.
	Branch string `json:"branch"`
	// Patterns lists the patterns of the matching rules, in rule order.
	Patterns []string `json:"patterns"`
	// NoDirectPush is set when any matching rule forbids direct pushes.
	NoDirectPush bool `json:"no_direct_push"`
	// RequiredApprovals is the maximum over the matching rules.
	RequiredApprovals int `json:"required_approvals"`
	// NoDelete is set when any matching rule forbids deleting the branch.
	NoDelete bool `json:"no_delete"`
	// RestrictPushers is set when at least one matching rule has a non-empty
	// AllowedPushers list. Pushes are then limited to AllowedPushers.
	RestrictPushers bool `json:"restrict_pushers"`
	// AllowedPushers holds the usernames present in every non-empty
	// AllowedPushers list of the matching rules (sorted). A user must be
	// allowed by each restricting rule, so the lists intersect; an empty
	// slice with RestrictPushers set means nobody may push directly.
	AllowedPushers []string `json:"allowed_pushers"`
}

// Protected reports whether at least one rule matched the branch.
func (e Effective) Protected() bool { return len(e.Patterns) > 0 }

// MayPush reports whether username passes the AllowedPushers restriction.
// It does not consider NoDirectPush.
func (e Effective) MayPush(username string) bool {
	if !e.RestrictPushers {
		return true
	}
	if username == "" {
		return false
	}
	_, found := slices.BinarySearch(e.AllowedPushers, username)
	return found
}

// Resolve computes the effective protection of branch from a tree's rules.
func Resolve(rules []domain.BranchProtection, branch string) Effective {
	eff := Effective{Branch: branch, Patterns: []string{}, AllowedPushers: []string{}}
	var allowed map[string]struct{}
	for i := range rules {
		r := &rules[i]
		if !MatchPattern(r.Pattern, branch) {
			continue
		}
		eff.Patterns = append(eff.Patterns, r.Pattern)
		eff.NoDirectPush = eff.NoDirectPush || r.NoDirectPush
		eff.NoDelete = eff.NoDelete || r.NoDelete
		eff.RequiredApprovals = max(eff.RequiredApprovals, r.RequiredApprovals)

		list := pusherSet(r.AllowedPushers)
		if len(list) == 0 {
			continue // role-based only: adds no restriction
		}
		if !eff.RestrictPushers {
			eff.RestrictPushers = true
			allowed = list
			continue
		}
		for u := range allowed {
			if _, ok := list[u]; !ok {
				delete(allowed, u)
			}
		}
	}
	for u := range allowed {
		eff.AllowedPushers = append(eff.AllowedPushers, u)
	}
	slices.Sort(eff.AllowedPushers)
	return eff
}

func pusherSet(names []string) map[string]struct{} {
	set := make(map[string]struct{}, len(names))
	for _, n := range names {
		if n = strings.TrimSpace(n); n != "" {
			set[n] = struct{}{}
		}
	}
	return set
}

// MatchPattern reports whether branch matches a protection pattern.
//
// Patterns use path.Match syntax ('*' and '?' never cross '/'), extended
// with "**" as a whole segment:
//   - a trailing "**" matches one or more segments, so "release/**" matches
//     "release/1.0" and "release/1.x/rc1" but not "release" itself, and a
//     bare "**" matches every branch;
//   - a "**" elsewhere matches zero or more segments ("a/**/b" matches
//     "a/b" and "a/x/y/b").
//
// A malformed pattern only matches a branch with exactly the same name.
func MatchPattern(pattern, branch string) bool {
	if pattern == "" || branch == "" {
		return false
	}
	if pattern == branch {
		return true
	}
	segs := strings.Split(pattern, "/")
	if !slices.Contains(segs, globstar) {
		ok, err := path.Match(pattern, branch)
		return err == nil && ok
	}
	// Collapse runs of "**" so matching stays polynomial.
	segs = slices.CompactFunc(segs, func(a, b string) bool { return a == globstar && b == globstar })
	return matchSegments(segs, strings.Split(branch, "/"))
}

func matchSegments(pat, name []string) bool {
	for len(pat) > 0 {
		if pat[0] == globstar {
			rest := pat[1:]
			if len(rest) == 0 {
				return len(name) > 0
			}
			for i := 0; i <= len(name); i++ {
				if matchSegments(rest, name[i:]) {
					return true
				}
			}
			return false
		}
		if len(name) == 0 {
			return false
		}
		if ok, err := path.Match(pat[0], name[0]); err != nil || !ok {
			return false
		}
		pat, name = pat[1:], name[1:]
	}
	return len(name) == 0
}

// ValidatePattern reports whether pattern is a well-formed protection
// pattern. It returns an error wrapping domain.ErrInvalid otherwise. API
// handlers can use it before storing a domain.BranchProtection.
func ValidatePattern(pattern string) error {
	if strings.TrimSpace(pattern) == "" {
		return fmt.Errorf("protection pattern must not be empty: %w", domain.ErrInvalid)
	}
	for _, seg := range strings.Split(pattern, "/") {
		if seg == "" {
			return fmt.Errorf("protection pattern %q has an empty segment: %w", pattern, domain.ErrInvalid)
		}
		if seg == globstar {
			continue
		}
		if _, err := path.Match(seg, ""); errors.Is(err, path.ErrBadPattern) {
			return fmt.Errorf("protection pattern %q is malformed: %w", pattern, domain.ErrInvalid)
		}
	}
	return nil
}
