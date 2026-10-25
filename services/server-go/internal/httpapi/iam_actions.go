package httpapi

import (
	"slices"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ActionInfo describes one authorization action (GET /api/v1/iam/actions).
type ActionInfo struct {
	Action      domain.Action `json:"action"`
	Description string        `json:"description"`
	Area        string        `json:"area"` // the part before ':', e.g. "tree"
}

// ActionCatalog lists the actions the server knows and what the built-in
// roles grant. The IAM endpoints use it to validate custom-role
// permissions, token scopes and policy statements.
type ActionCatalog interface {
	// Actions returns every concrete action, grouped by area.
	Actions() []ActionInfo
	// BuiltinPermissions returns the actions a built-in role grants.
	BuiltinPermissions(role string) []domain.Action
}

// iamCatalog is the catalog used by the IAM endpoints: the authorization
// engine's own catalog, so the two can never disagree.
var iamCatalog ActionCatalog = authzCatalog{}

// authzCatalog adapts the authz package to ActionCatalog.
type authzCatalog struct{}

func (authzCatalog) Actions() []ActionInfo {
	acts := authz.Actions()
	out := make([]ActionInfo, 0, len(acts))
	for _, a := range acts {
		out = append(out, ActionInfo{Action: a.Action, Description: a.Description, Area: a.Area})
	}
	return out
}

func (authzCatalog) BuiltinPermissions(role string) []domain.Action {
	var out []domain.Action
	for _, a := range authz.Actions() {
		if authz.RoleAllows(role, a.Action) {
			out = append(out, a.Action)
		}
	}
	return nonNil(out)
}

// builtinRoles lists the built-in roles, most privileged first.
var builtinRoles = func() []struct{ name, description string } {
	roles := authz.BuiltinRoles()
	out := make([]struct{ name, description string }, 0, len(roles))
	for _, r := range roles {
		out = append(out, struct{ name, description string }{r.Name, r.Description})
	}
	return out
}()

// isBuiltinRole reports whether name is a built-in role.
func isBuiltinRole(name string) bool { return authz.IsBuiltinRole(name) }

// knownAction reports whether act is a concrete catalog action.
func knownAction(act string) bool {
	for _, ai := range iamCatalog.Actions() {
		if string(ai.Action) == act {
			return true
		}
	}
	return false
}

// knownArea reports whether area is the area of some catalog action.
func knownArea(area string) bool {
	for _, ai := range iamCatalog.Actions() {
		if ai.Area == area {
			return true
		}
	}
	return false
}

// checkActionPattern validates a concrete action, "<area>:*" or (when
// allowAll) "*". It returns a client-facing message or "".
func checkActionPattern(s string, allowAll bool) string {
	switch {
	case s == "*":
		if allowAll {
			return ""
		}
		return `"*" is not allowed here; list actions or "<area>:*"`
	case strings.HasSuffix(s, ":*"):
		if area := strings.TrimSuffix(s, ":*"); knownArea(area) {
			return ""
		}
		return "unknown action area in " + strconv.Quote(s)
	case knownAction(s):
		return ""
	}
	return "unknown action " + strconv.Quote(s)
}

// scopeCovers reports whether the scope pattern have permits every action
// matched by the scope pattern want.
func scopeCovers(have, want string) bool {
	switch {
	case have == "*" || have == want:
		return true
	case want == "*":
		return false
	case strings.HasSuffix(have, ":*"):
		area := strings.TrimSuffix(have, "*")
		return strings.HasPrefix(want, area)
	}
	return false
}

// validateScopes normalizes token scopes: each must be an action pattern
// ("tree:read", "tree:*" or "*"); duplicates are removed. When the caller
// is itself restricted by scopes, every requested scope must be covered by
// one of them so that a scoped token cannot mint a broader one.
func validateScopes(scopes []string, caller *domain.Principal) ([]string, error) {
	if len(scopes) > 64 {
		return nil, badRequestf("at most 64 scopes are allowed")
	}
	out := make([]string, 0, len(scopes))
	for _, s := range scopes {
		s = strings.TrimSpace(s)
		if msg := checkActionPattern(s, true); msg != "" {
			return nil, badRequestf("scopes: %s", msg)
		}
		if !slices.Contains(out, s) {
			out = append(out, s)
		}
	}
	if caller != nil && len(caller.Scopes) > 0 {
		if len(out) == 0 {
			return nil, forbiddenf("a scoped token can only create tokens with a subset of its scopes")
		}
		for _, s := range out {
			if !slices.ContainsFunc(caller.Scopes, func(h string) bool { return scopeCovers(h, s) }) {
				return nil, forbiddenf("scope %q exceeds the scopes of the calling token", s)
			}
		}
	}
	return out, nil
}
