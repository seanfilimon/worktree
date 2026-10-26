package iam

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Scope limits.
const (
	maxScopes   = 64
	maxScopeLen = 64
)

// knownActions lists every authorization action a token scope may name.
var knownActions = []domain.Action{
	domain.ActTreeRead,
	domain.ActTreeWrite,
	domain.ActTreeCreate,
	domain.ActTreeAdmin,
	domain.ActBranchCreate,
	domain.ActBranchDelete,
	domain.ActBranchPush,
	domain.ActMRCreate,
	domain.ActMRApprove,
	domain.ActMRMerge,
	domain.ActTenantRead,
	domain.ActTenantAdmin,
	domain.ActServerAdmin,
	domain.ActIAMRead,
	domain.ActIAMAdmin,
	domain.ActReleaseCreate,
	domain.ActReleaseDelete,
	domain.ActCIReport,
	domain.ActTreeFork,
	domain.ActBranchForcePush,
	domain.ActProtectionBypass,
}

var (
	knownActionSet = func() map[domain.Action]bool {
		m := make(map[domain.Action]bool, len(knownActions))
		for _, a := range knownActions {
			m[a] = true
		}
		return m
	}()
	knownAreas = func() map[string]bool {
		m := map[string]bool{}
		for _, a := range knownActions {
			area, _, _ := strings.Cut(string(a), ":")
			m[area] = true
		}
		return m
	}()
)

// KnownActions returns every concrete action, in a stable order.
func KnownActions() []domain.Action {
	return append([]domain.Action(nil), knownActions...)
}

// IsKnownAction reports whether act is a concrete action of the catalog.
func IsKnownAction(act domain.Action) bool { return knownActionSet[act] }

// ValidateScopePattern checks one token scope: a concrete known action
// ("tree:read"), an area wildcard over a known area ("tree:*") or "*".
// Errors wrap domain.ErrInvalid.
func ValidateScopePattern(pattern string) error {
	if pattern == "*" {
		return nil
	}
	if pattern == "" || len(pattern) > maxScopeLen {
		return fmt.Errorf("scope %q: must be 1-%d characters: %w", pattern, maxScopeLen, domain.ErrInvalid)
	}
	area, verb, ok := strings.Cut(pattern, ":")
	if !ok || area == "" || verb == "" {
		return fmt.Errorf("scope %q: must be an action (area:verb), area:* or *: %w", pattern, domain.ErrInvalid)
	}
	if !knownAreas[area] {
		return fmt.Errorf("scope %q: unknown area %q: %w", pattern, area, domain.ErrInvalid)
	}
	if verb == "*" {
		return nil
	}
	if !knownActionSet[domain.Action(pattern)] {
		return fmt.Errorf("scope %q: unknown action: %w", pattern, domain.ErrInvalid)
	}
	return nil
}

// ValidateScopes trims, validates and de-duplicates token scopes, keeping
// the first-seen order. An empty or nil input yields nil (an unrestricted
// token). Errors wrap domain.ErrInvalid.
func ValidateScopes(scopes []string) ([]string, error) {
	if len(scopes) > maxScopes {
		return nil, fmt.Errorf("at most %d scopes are allowed: %w", maxScopes, domain.ErrInvalid)
	}
	var out []string
	seen := make(map[string]bool, len(scopes))
	for _, s := range scopes {
		s = strings.ToLower(strings.TrimSpace(s))
		if err := ValidateScopePattern(s); err != nil {
			return nil, err
		}
		if !seen[s] {
			seen[s] = true
			out = append(out, s)
		}
	}
	return out, nil
}

// ScopeMatches reports whether a single scope pattern covers act.
func ScopeMatches(pattern string, act domain.Action) bool {
	if pattern == "*" {
		return true
	}
	if area, ok := strings.CutSuffix(pattern, ":*"); ok {
		actArea, _, _ := strings.Cut(string(act), ":")
		return area != "" && area == actArea
	}
	return pattern == string(act)
}

// ScopeAllows reports whether token scopes permit act. Empty scopes are
// unrestricted. branch:push is also covered by a scope granting tree:write
// (pushing to a branch is a tree write).
func ScopeAllows(scopes []string, act domain.Action) bool {
	if len(scopes) == 0 {
		return true
	}
	for _, s := range scopes {
		if ScopeMatches(s, act) {
			return true
		}
		if act == domain.ActBranchPush && ScopeMatches(s, domain.ActTreeWrite) {
			return true
		}
	}
	return false
}

// CreateScopedToken issues a personal API token (Kind api) restricted to
// scopes (see ValidateScopes; empty means unrestricted). ttl 0 never
// expires; negative ttl is invalid.
func (s *Service) CreateScopedToken(ctx context.Context, accountID, name string, ttl time.Duration, scopes []string) (string, *domain.APIToken, error) {
	name = strings.TrimSpace(name)
	if err := validateTokenName(name); err != nil {
		return "", nil, fmt.Errorf("iam: %w", err)
	}
	if ttl < 0 {
		return "", nil, fmt.Errorf("iam: token ttl must not be negative: %w", domain.ErrInvalid)
	}
	sc, err := ValidateScopes(scopes)
	if err != nil {
		return "", nil, fmt.Errorf("iam: %w", err)
	}
	acct, err := s.activeAccount(ctx, accountID)
	if err != nil {
		return "", nil, err
	}
	return s.issue(ctx, &domain.APIToken{
		AccountID: acct.ID,
		Name:      name,
		Kind:      domain.TokenAPI,
		Scopes:    sc,
	}, ttl)
}
