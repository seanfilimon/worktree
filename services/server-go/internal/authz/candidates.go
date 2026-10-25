package authz

import (
	"context"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// candidateKey carries candidate policies on a context.
type candidateKey struct{}

type candidateSet struct {
	tenantID string
	policies []domain.Policy
}

// WithCandidatePolicies returns a context under which evaluations for
// tenantID use policies instead of the tenant's stored policies: a dry run
// of a policy change (POST /iam/simulate "candidate_policies", §13.7).
// Only enabled candidates take part, exactly like stored policies.
// Evaluations for other tenants are unaffected.
func WithCandidatePolicies(ctx context.Context, tenantID string, policies []domain.Policy) context.Context {
	cp := make([]domain.Policy, len(policies))
	copy(cp, policies)
	return context.WithValue(ctx, candidateKey{}, &candidateSet{tenantID: tenantID, policies: cp})
}

// candidatesFor returns the candidate policies attached to ctx for
// tenantID, if any.
func candidatesFor(ctx context.Context, tenantID string) ([]domain.Policy, bool) {
	cs, _ := ctx.Value(candidateKey{}).(*candidateSet)
	if cs == nil || cs.tenantID != tenantID {
		return nil, false
	}
	return cs.policies, true
}

// ExpandActions expands action patterns ("tree:read", "release:*", "*")
// into the concrete catalog actions they cover, in catalog order, without
// duplicates. Unknown patterns expand to nothing. server:admin is never
// included: no role or policy can grant it.
func ExpandActions(patterns []string) []domain.Action {
	var out []domain.Action
	for _, info := range catalog {
		if areaOf(string(info.Action)) == AreaServer {
			continue
		}
		for _, p := range patterns {
			if actionPatternMatches(strings.TrimSpace(p), info.Action) {
				out = append(out, info.Action)
				break
			}
		}
	}
	return out
}

// ValidatePrincipal checks one policy statement principal pattern (the
// syntax accepted by ValidateStatements).
func ValidatePrincipal(p string) error {
	return validatePrincipalPattern(strings.TrimSpace(p))
}
