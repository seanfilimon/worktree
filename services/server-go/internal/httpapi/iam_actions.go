package httpapi

import (
	"slices"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
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
//
// INTEGRATION: the authoritative catalog is authz.Actions() (iam-authz
// workstream). Until it is wired in, iamCatalog is the local copy below;
// replace it with an adapter over authz.Actions() / iam.RoleAllows at
// integration time (the interface is deliberately small).
type ActionCatalog interface {
	// Actions returns every concrete action, grouped by area.
	Actions() []ActionInfo
	// BuiltinPermissions returns the actions a built-in role grants.
	BuiltinPermissions(role string) []domain.Action
}

// iamCatalog is the catalog used by the IAM endpoints.
var iamCatalog ActionCatalog = localCatalog{}

// localActions mirrors ENGINEERING_PLAN §7 / §12 (and authz.Actions()).
var localActions = []ActionInfo{
	{domain.ActTenantRead, "View the tenant, its members and its trees", "tenant"},
	{domain.ActTenantAdmin, "Manage tenant members and settings", "tenant"},
	{domain.ActTreeRead, "Read and clone a tree", "tree"},
	{domain.ActTreeWrite, "Push snapshots, stage work and create tags", "tree"},
	{domain.ActTreeCreate, "Create trees in the tenant", "tree"},
	{domain.ActTreeAdmin, "Manage tree settings, protections and licenses, delete the tree", "tree"},
	{domain.ActBranchCreate, "Create branches", "branch"},
	{domain.ActBranchDelete, "Delete branches", "branch"},
	{domain.ActBranchPush, "Push to a specific branch (requires tree:write)", "branch"},
	{domain.ActMRCreate, "Open and close merge requests", "mr"},
	{domain.ActMRApprove, "Approve merge requests", "mr"},
	{domain.ActMRMerge, "Merge merge requests", "mr"},
	{domain.ActIAMRead, "View teams, roles, policies, grants, service accounts and invitations", "iam"},
	{domain.ActIAMAdmin, "Manage teams, roles, policies, grants, service accounts and invitations", "iam"},
	{domain.ActServerAdmin, "Administer the whole server (server admins only)", "server"},
}

// builtinRoles lists the built-in roles, most privileged first.
var builtinRoles = []struct{ name, description string }{
	{domain.RoleOwner, "Full control of the tenant, including ownership changes"},
	{domain.RoleAdmin, "Manage members, trees, settings and access"},
	{domain.RoleMaintainer, "Merge, manage branches and create trees; view access configuration"},
	{domain.RoleDeveloper, "Push, branch and open merge requests"},
	{domain.RoleViewer, "Read-only access"},
}

// v04MinRole covers the §12 actions that predate iam.RoleAllows support.
var v04MinRole = map[domain.Action]string{
	domain.ActBranchPush: domain.RoleDeveloper,
	domain.ActIAMRead:    domain.RoleMaintainer,
	domain.ActIAMAdmin:   domain.RoleAdmin,
}

type localCatalog struct{}

func (localCatalog) Actions() []ActionInfo { return slices.Clone(localActions) }

func (localCatalog) BuiltinPermissions(role string) []domain.Action {
	var out []domain.Action
	for _, ai := range localActions {
		if iam.RoleAllows(role, ai.Action) {
			out = append(out, ai.Action)
			continue
		}
		if least, ok := v04MinRole[ai.Action]; ok && iam.RoleAtLeast(role, least) {
			out = append(out, ai.Action)
		}
	}
	return nonNil(out)
}

// isBuiltinRole reports whether name is a built-in role.
func isBuiltinRole(name string) bool {
	for _, r := range builtinRoles {
		if r.name == name {
			return true
		}
	}
	return false
}

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
