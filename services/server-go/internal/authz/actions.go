package authz

import (
	"fmt"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Action areas: the part of an action before the colon.
const (
	AreaTenant  = "tenant"
	AreaTree    = "tree"
	AreaBranch  = "branch"
	AreaMR      = "mr"
	AreaIAM     = "iam"
	AreaRelease = "release" // v0.5
	AreaCI      = "ci"      // v0.5
	AreaServer  = "server"
)

// ActionInfo describes one authorization verb (GET /iam/actions).
type ActionInfo struct {
	Action      domain.Action `json:"action"`
	Area        string        `json:"area"`
	Description string        `json:"description"`
}

// catalog lists every action in display order (grouped by area).
var catalog = []ActionInfo{
	{domain.ActTenantRead, AreaTenant, "View the tenant, its members and its trees"},
	{domain.ActTenantAdmin, AreaTenant, "Manage tenant members and settings"},
	{domain.ActTreeRead, AreaTree, "Read a tree: browse, clone, pull and download"},
	{domain.ActTreeWrite, AreaTree, "Write to a tree: push, stage and tag"},
	{domain.ActTreeCreate, AreaTree, "Create trees in the tenant"},
	{domain.ActTreeAdmin, AreaTree, "Administer a tree: settings, protections, licenses, deletion"},
	{domain.ActTreeFork, AreaTree, "Fork a tree into a tenant where the caller may create trees (v0.8)"},
	{domain.ActBranchCreate, AreaBranch, "Create branches"},
	{domain.ActBranchDelete, AreaBranch, "Delete branches"},
	{domain.ActBranchPush, AreaBranch, "Push to a specific branch (requires tree:write; policies may deny per branch)"},
	{domain.ActBranchForcePush, AreaBranch, "Force-push (rewind) a branch; never on no_force_push protections (v0.8)"},
	{domain.ActProtectionBypass, AreaBranch, "Bypass branch protection rules when pushing or merging (v0.8, audited)"},
	{domain.ActMRCreate, AreaMR, "Open merge requests"},
	{domain.ActMRApprove, AreaMR, "Approve merge requests"},
	{domain.ActMRMerge, AreaMR, "Merge and close merge requests"},
	{domain.ActReleaseCreate, AreaRelease, "Create and edit releases and upload release assets"},
	{domain.ActReleaseDelete, AreaRelease, "Delete releases and release assets"},
	{domain.ActCIReport, AreaCI, "Report CI check results for snapshots"},
	{domain.ActIAMRead, AreaIAM, "View teams, roles, policies, grants and service accounts"},
	{domain.ActIAMAdmin, AreaIAM, "Manage teams, roles, policies, grants, service accounts and invitations"},
	{domain.ActServerAdmin, AreaServer, "Administer the server (server admins only)"},
}

var (
	actionSet = func() map[domain.Action]ActionInfo {
		m := make(map[domain.Action]ActionInfo, len(catalog))
		for _, a := range catalog {
			m[a.Action] = a
		}
		return m
	}()
	areaSet = func() map[string]bool {
		m := map[string]bool{}
		for _, a := range catalog {
			m[a.Area] = true
		}
		return m
	}()
)

// Actions returns the action catalog in display order. The slice is a copy.
func Actions() []ActionInfo {
	return append([]ActionInfo(nil), catalog...)
}

// IsAction reports whether act is a concrete action of the catalog.
func IsAction(act domain.Action) bool {
	_, ok := actionSet[act]
	return ok
}

// areaOf returns the area of an action ("tree" for "tree:read").
func areaOf(act string) string {
	area, _, _ := strings.Cut(act, ":")
	return area
}

// treeScoped reports whether act operates on a single tree. Only these
// actions can be granted by tree grants (Grant / team grants): a grant
// raises access to one tree, never to the tenant.
func treeScoped(act domain.Action) bool {
	switch act {
	case domain.ActTreeRead, domain.ActTreeWrite, domain.ActTreeAdmin,
		domain.ActBranchCreate, domain.ActBranchDelete, domain.ActBranchPush,
		domain.ActMRCreate, domain.ActMRApprove, domain.ActMRMerge,
		domain.ActReleaseCreate, domain.ActReleaseDelete, domain.ActCIReport,
		domain.ActTreeFork, domain.ActBranchForcePush, domain.ActProtectionBypass:
		return true
	}
	return false
}

// actionPatternMatches reports whether an action pattern ("tree:read",
// "tree:*" or "*") covers act.
func actionPatternMatches(pattern string, act domain.Action) bool {
	pattern = strings.TrimSpace(pattern)
	switch {
	case pattern == "*":
		return true
	case strings.HasSuffix(pattern, ":*"):
		return strings.TrimSuffix(pattern, ":*") == areaOf(string(act))
	default:
		return pattern == string(act)
	}
}

// validateActionPattern checks a policy / scope action pattern: a catalog
// action, "<area>:*" for a known area, or "*".
func validateActionPattern(pattern string) error {
	switch {
	case pattern == "":
		return fmt.Errorf("action is empty")
	case pattern == "*":
		return nil
	case strings.HasSuffix(pattern, ":*"):
		if area := strings.TrimSuffix(pattern, ":*"); !areaSet[area] {
			return fmt.Errorf("unknown action area %q", area)
		}
		return nil
	case !IsAction(domain.Action(pattern)):
		return fmt.Errorf("unknown action %q", pattern)
	}
	return nil
}

// ValidatePermissions checks the permissions of a custom role: each must be
// a catalog action or "<area>:*". server:admin (and "server:*") cannot be
// granted by a role. It returns the first problem found.
func ValidatePermissions(perms []domain.Action) error {
	for i, p := range perms {
		s := string(p)
		if s == "*" {
			return fmt.Errorf("permission %d: \"*\" is not allowed in a role; list areas (\"tree:*\") instead", i)
		}
		if err := validateActionPattern(s); err != nil {
			return fmt.Errorf("permission %d: %v", i, err)
		}
		if areaOf(s) == AreaServer {
			return fmt.Errorf("permission %d: server permissions cannot be granted by a role", i)
		}
	}
	return nil
}

// ---- built-in roles ----

// roleRank orders the built-in roles; higher ranks include all permissions
// of lower ones. Unknown roles rank 0.
var roleRank = map[string]int{
	domain.RoleViewer:     1,
	domain.RoleDeveloper:  2,
	domain.RoleMaintainer: 3,
	domain.RoleAdmin:      4,
	domain.RoleOwner:      5,
}

// builtinOrder lists the built-in roles from most to least privileged.
var builtinOrder = []string{domain.RoleOwner, domain.RoleAdmin, domain.RoleMaintainer, domain.RoleDeveloper, domain.RoleViewer}

var builtinDescriptions = map[string]string{
	domain.RoleOwner:      "Full control of the tenant (same actions as admin; owner-only operations are enforced separately)",
	domain.RoleAdmin:      "Manage the tenant, its members, IAM and every tree",
	domain.RoleMaintainer: "Create trees, merge and approve, delete branches, view IAM",
	domain.RoleDeveloper:  "Push, stage, tag, create branches and open merge requests",
	domain.RoleViewer:     "Read the tenant and its trees",
}

// actionMinRole is the least privileged built-in role granting each action
// (the §7 matrix of internal/iam extended for v0.4: branch:push with
// tree:write, iam:read for maintainer+, iam:admin for admin+). Actions
// absent from the map (server:admin) are never granted by a role.
var actionMinRole = map[domain.Action]string{
	domain.ActTenantRead:       domain.RoleViewer,
	domain.ActTreeRead:         domain.RoleViewer,
	domain.ActTreeWrite:        domain.RoleDeveloper,
	domain.ActBranchCreate:     domain.RoleDeveloper,
	domain.ActBranchPush:       domain.RoleDeveloper,
	domain.ActMRCreate:         domain.RoleDeveloper,
	domain.ActReleaseCreate:    domain.RoleDeveloper,
	domain.ActCIReport:         domain.RoleDeveloper,
	domain.ActReleaseDelete:    domain.RoleMaintainer,
	domain.ActBranchDelete:     domain.RoleMaintainer,
	domain.ActMRApprove:        domain.RoleMaintainer,
	domain.ActMRMerge:          domain.RoleMaintainer,
	domain.ActTreeCreate:       domain.RoleMaintainer,
	domain.ActIAMRead:          domain.RoleMaintainer,
	domain.ActTreeAdmin:        domain.RoleAdmin,
	domain.ActTenantAdmin:      domain.RoleAdmin,
	domain.ActIAMAdmin:         domain.RoleAdmin,
	domain.ActTreeFork:         domain.RoleViewer,
	domain.ActBranchForcePush:  domain.RoleAdmin,
	domain.ActProtectionBypass: domain.RoleOwner,
}

// IsBuiltinRole reports whether name is a built-in role (case-insensitive).
func IsBuiltinRole(name string) bool {
	return roleRank[strings.ToLower(name)] > 0
}

// RoleAllows reports whether the built-in role grants act. It ignores
// server-admin status, tree visibility and policies; custom roles and
// unknown names grant nothing.
func RoleAllows(role string, act domain.Action) bool {
	minRole, ok := actionMinRole[act]
	if !ok {
		return false
	}
	r := roleRank[strings.ToLower(role)]
	return r > 0 && r >= roleRank[minRole]
}

// RoleInfo describes a built-in role (GET /tenants/{t}/roles).
type RoleInfo struct {
	Name        string          `json:"name"`
	Description string          `json:"description"`
	Permissions []domain.Action `json:"permissions"`
	Builtin     bool            `json:"builtin"`
}

// BuiltinRoles returns the built-in roles, most privileged first, with
// their permissions in catalog order.
func BuiltinRoles() []RoleInfo {
	out := make([]RoleInfo, 0, len(builtinOrder))
	for _, name := range builtinOrder {
		out = append(out, RoleInfo{
			Name:        name,
			Description: builtinDescriptions[name],
			Permissions: builtinPermissions(name),
			Builtin:     true,
		})
	}
	return out
}

func builtinPermissions(role string) []domain.Action {
	perms := make([]domain.Action, 0, len(catalog))
	for _, a := range catalog {
		if RoleAllows(role, a.Action) {
			perms = append(perms, a.Action)
		}
	}
	return perms
}

// customRoleGrant returns the permission of a custom role that grants act
// ("" when none does). server:admin is never granted.
func customRoleGrant(perms []domain.Action, act domain.Action) string {
	if areaOf(string(act)) == AreaServer {
		return ""
	}
	for _, p := range perms {
		if actionPatternMatches(string(p), act) {
			return string(p)
		}
	}
	return ""
}
