package iam

import (
	"context"
	"strings"
	"sync"

	"github.com/ramizik/worktree/server-go/internal/auth"
)

type Decision int

const (
	Allow Decision = iota
	Deny
)

func (d Decision) String() string {
	if d == Allow {
		return "allow"
	}
	return "deny"
}

// Authorizer evaluates whether principal may perform action on resource.
// action uses the canonical permission name (e.g. "staged:create").
// resource is a slash-delimited path: tenant/worktree/branch/snapshot_id.
type Authorizer interface {
	Authorize(ctx context.Context, principal auth.Principal, action string, resource string) (Decision, error)
}

// AllowAllAuthorizer permits every request. It is only valid in tests or an
// explicitly configured development mode; production startup rejects it.
type AllowAllAuthorizer struct{}

func (AllowAllAuthorizer) Authorize(_ context.Context, _ auth.Principal, _, _ string) (Decision, error) {
	return Allow, nil
}

// DenyAllAuthorizer rejects every request. Used in tests.
type DenyAllAuthorizer struct{}

func (DenyAllAuthorizer) Authorize(_ context.Context, _ auth.Principal, _, _ string) (Decision, error) {
	return Deny, nil
}

type Effect string

const (
	EffectAllow Effect = "allow"
	EffectDeny  Effect = "deny"
)

type PolicyRule struct {
	Effect     Effect            `json:"effect" toml:"effect"`
	Tenant     string            `json:"tenant,omitempty" toml:"tenant,omitempty"`
	Account    string            `json:"account,omitempty" toml:"account,omitempty"`
	Actions    []string          `json:"actions" toml:"actions"`
	Resources  []string          `json:"resources" toml:"resources"`
	Conditions map[string]string `json:"conditions,omitempty" toml:"conditions,omitempty"`
}

type PolicyAuthorizer struct {
	mu          sync.RWMutex
	tenantRules map[string][]PolicyRule
}

func NewPolicyAuthorizer(rules map[string][]PolicyRule) *PolicyAuthorizer {
	copied := make(map[string][]PolicyRule, len(rules))
	for k, v := range rules {
		copied[k] = append([]PolicyRule(nil), v...)
	}
	return &PolicyAuthorizer{tenantRules: copied}
}

func NewDefaultPolicyAuthorizer() *PolicyAuthorizer {
	return NewPolicyAuthorizer(map[string][]PolicyRule{
		// Note: using "" (empty string) for global/default rules if needed,
		// but typically we'd inject this via system initialization.
		"": {
			{
				Effect:    EffectAllow,
				Actions:   []string{"staged:create", "staged:list"},
				Resources: []string{"tenant:${tenant}/*"},
				Conditions: map[string]string{
					"scope": "staged:*",
				},
			},
			{
				Effect: EffectAllow,
				Actions: []string{
					"branch:push", "branch:pull",
					"object:check", "object:read", "object:write",
					"ref:list",
				},
				Resources: []string{"tenant:${tenant}/*", "tenant:${tenant}"},
			},
		},
	})
}

func (a *PolicyAuthorizer) UpdateTenantRules(ctx context.Context, tenant string, rules []PolicyRule) error {
	copied := append([]PolicyRule(nil), rules...)
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.tenantRules == nil {
		a.tenantRules = make(map[string][]PolicyRule)
	}
	a.tenantRules[tenant] = copied
	return nil
}

func (a *PolicyAuthorizer) Authorize(ctx context.Context, principal auth.Principal, action string, resource string) (Decision, error) {
	if err := ctx.Err(); err != nil {
		return Deny, err
	}
	if !principal.Authenticated {
		return Deny, nil
	}

	a.mu.RLock()
	globalRules := a.tenantRules[""]
	tenantRules := a.tenantRules[principal.Tenant]
	a.mu.RUnlock()

	matchedAllow := false

	// Check global rules first
	for _, rule := range globalRules {
		if !ruleMatchesPrincipal(rule, principal) {
			continue
		}
		if !matchesAny(rule.Actions, action) {
			continue
		}
		if !resourceMatchesAny(rule.Resources, principal, resource) {
			continue
		}
		if !conditionsMatch(rule.Conditions, principal, action) {
			continue
		}
		if rule.Effect == EffectDeny {
			return Deny, nil
		}
		if rule.Effect == EffectAllow {
			matchedAllow = true
		}
	}

	// Then check tenant-specific rules
	for _, rule := range tenantRules {
		if !ruleMatchesPrincipal(rule, principal) {
			continue
		}
		if !matchesAny(rule.Actions, action) {
			continue
		}
		if !resourceMatchesAny(rule.Resources, principal, resource) {
			continue
		}
		if !conditionsMatch(rule.Conditions, principal, action) {
			continue
		}
		if rule.Effect == EffectDeny {
			return Deny, nil
		}
		if rule.Effect == EffectAllow {
			matchedAllow = true
		}
	}

	if matchedAllow {
		return Allow, nil
	}
	return Deny, nil
}

func ruleMatchesPrincipal(rule PolicyRule, principal auth.Principal) bool {
	if rule.Tenant != "" && rule.Tenant != principal.Tenant {
		return false
	}
	if rule.Account != "" && rule.Account != principal.Account {
		return false
	}
	return true
}

func conditionsMatch(conditions map[string]string, principal auth.Principal, action string) bool {
	for key, value := range conditions {
		switch key {
		case "scope":
			if !principal.HasScope(value) && !principal.HasScope(action) {
				return false
			}
		default:
			return false
		}
	}
	return true
}

func matchesAny(patterns []string, value string) bool {
	if len(patterns) == 0 {
		return false
	}
	for _, pattern := range patterns {
		if wildcardMatch(pattern, value) {
			return true
		}
	}
	return false
}

func resourceMatchesAny(patterns []string, principal auth.Principal, resource string) bool {
	if len(patterns) == 0 {
		return false
	}
	normalizedResource := normalizeResource(resource)
	for _, pattern := range patterns {
		pattern = strings.ReplaceAll(pattern, "${tenant}", principal.Tenant)
		pattern = strings.ReplaceAll(pattern, "${account}", principal.Account)
		if wildcardMatch(pattern, normalizedResource) {
			return true
		}
	}
	return false
}

func normalizeResource(resource string) string {
	if resource == "" || resource == "staged" {
		return resource
	}
	if strings.HasPrefix(resource, "tenant:") {
		return resource
	}
	return "tenant:" + resource
}

func wildcardMatch(pattern, value string) bool {
	if pattern == "*" || pattern == value {
		return true
	}
	if strings.HasSuffix(pattern, "*") {
		return strings.HasPrefix(value, strings.TrimSuffix(pattern, "*"))
	}
	return false
}
