package authz

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// v03Actions are the actions of the v0.3 iam matrix.
var v03Actions = []domain.Action{
	domain.ActTreeRead, domain.ActTreeWrite, domain.ActTreeCreate, domain.ActTreeAdmin,
	domain.ActBranchCreate, domain.ActBranchDelete,
	domain.ActMRCreate, domain.ActMRApprove, domain.ActMRMerge,
	domain.ActTenantRead, domain.ActTenantAdmin, domain.ActServerAdmin,
}

// expectedGrants is the full v0.4 built-in matrix, written out explicitly.
var expectedGrants = map[string]map[domain.Action]bool{
	domain.RoleViewer: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
	},
	domain.RoleDeveloper: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActBranchPush: true, domain.ActMRCreate: true,
		domain.ActReleaseCreate: true, domain.ActCIReport: true,
	},
	domain.RoleMaintainer: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActBranchPush: true, domain.ActMRCreate: true,
		domain.ActReleaseCreate: true, domain.ActCIReport: true,
		domain.ActBranchDelete: true, domain.ActMRApprove: true, domain.ActMRMerge: true, domain.ActTreeCreate: true,
		domain.ActReleaseDelete: true,
		domain.ActIAMRead:       true,
	},
	domain.RoleAdmin: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActBranchPush: true, domain.ActMRCreate: true,
		domain.ActReleaseCreate: true, domain.ActCIReport: true,
		domain.ActBranchDelete: true, domain.ActMRApprove: true, domain.ActMRMerge: true, domain.ActTreeCreate: true,
		domain.ActReleaseDelete: true,
		domain.ActIAMRead:       true, domain.ActTreeAdmin: true, domain.ActTenantAdmin: true, domain.ActIAMAdmin: true,
	},
	domain.RoleOwner: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActBranchPush: true, domain.ActMRCreate: true,
		domain.ActReleaseCreate: true, domain.ActCIReport: true,
		domain.ActBranchDelete: true, domain.ActMRApprove: true, domain.ActMRMerge: true, domain.ActTreeCreate: true,
		domain.ActReleaseDelete: true,
		domain.ActIAMRead:       true, domain.ActTreeAdmin: true, domain.ActTenantAdmin: true, domain.ActIAMAdmin: true,
	},
	"":            {}, // non-member
	"custom-role": {}, // undefined custom role
}

func TestRoleAllowsMatchesIAMForV03Actions(t *testing.T) {
	for role := range expectedGrants {
		for _, act := range v03Actions {
			if got, want := RoleAllows(role, act), iam.RoleAllows(role, act); got != want {
				t.Errorf("RoleAllows(%q, %s) = %v, iam says %v", role, act, got, want)
			}
		}
	}
	for role, grants := range expectedGrants {
		for _, info := range Actions() {
			if got := RoleAllows(role, info.Action); got != grants[info.Action] {
				t.Errorf("RoleAllows(%q, %s) = %v, want %v", role, info.Action, got, grants[info.Action])
			}
		}
	}
	if !RoleAllows("Maintainer", domain.ActIAMRead) {
		t.Error("role names must be case-insensitive")
	}
}

func TestBuiltinRolesSuperset(t *testing.T) {
	roles := BuiltinRoles()
	if len(roles) != 5 || roles[0].Name != domain.RoleOwner || roles[4].Name != domain.RoleViewer {
		t.Fatalf("BuiltinRoles order = %+v", roles)
	}
	for i := 0; i+1 < len(roles); i++ {
		hi, lo := map[domain.Action]bool{}, roles[i+1].Permissions
		for _, a := range roles[i].Permissions {
			hi[a] = true
		}
		for _, a := range lo {
			if !hi[a] {
				t.Errorf("%s lacks %s granted to %s", roles[i].Name, a, roles[i+1].Name)
			}
		}
	}
	for _, r := range roles {
		if !r.Builtin || r.Description == "" {
			t.Errorf("role %+v: want builtin with description", r)
		}
		for _, a := range r.Permissions {
			if a == domain.ActServerAdmin {
				t.Errorf("role %s grants server:admin", r.Name)
			}
		}
	}
}

// TestAuthorizeMatrix is iam's TestAuthorizeMatrix ported to authz, run
// over every catalog action.
func TestAuthorizeMatrix(t *testing.T) {
	ctx := context.Background()
	e := newEnv(t)
	tenant := e.tenant("t-org", "org")
	principals := map[string]*domain.Principal{}
	for role := range expectedGrants {
		name := "user-" + role
		if role == "" {
			name = "outsider"
		}
		p := e.user(name)
		if role != "" {
			e.member(tenant.ID, p, role)
		}
		principals[role] = p
	}
	admin := e.user("server-admin")
	e.updateAccount(admin, func(a *domain.Account) { a.IsAdmin = true })

	visibilities := []string{domain.VisibilityPrivate, domain.VisibilityInternal, domain.VisibilityPublic, "", "bogus"}
	for _, vis := range visibilities {
		tr := tree("tree-"+vis, tenant.ID, "repo", vis)
		for _, withTree := range []bool{true, false} {
			var trp *domain.Tree
			if withTree {
				trp = tr
			}
			for _, info := range Actions() {
				act := info.Action
				if err := e.az.Authorize(ctx, admin, act, tenant.ID, trp); err != nil {
					t.Errorf("admin %s vis=%q tree=%v: %v", act, vis, withTree, err)
				}

				err := e.az.Authorize(ctx, nil, act, tenant.ID, trp)
				anonOK := act == domain.ActTreeRead && withTree && vis == domain.VisibilityPublic
				checkDecision(t, "anonymous", act, vis, withTree, err, anonOK, domain.ErrUnauthenticated)

				for role, grants := range expectedGrants {
					want := grants[act]
					if act == domain.ActTreeRead && withTree &&
						(vis == domain.VisibilityPublic || vis == domain.VisibilityInternal) {
						want = true
					}
					err := e.az.Authorize(ctx, principals[role], act, tenant.ID, trp)
					checkDecision(t, "role "+role, act, vis, withTree, err, want, domain.ErrForbidden)
				}
			}
		}
	}
}

func checkDecision(t *testing.T, who string, act domain.Action, vis string, withTree bool, err error, want bool, denyErr error) {
	t.Helper()
	if want {
		if err != nil {
			t.Errorf("%s %s vis=%q tree=%v: unexpected deny: %v", who, act, vis, withTree, err)
		}
		return
	}
	if !errors.Is(err, denyErr) {
		t.Errorf("%s %s vis=%q tree=%v: got %v, want %v", who, act, vis, withTree, err, denyErr)
	}
}

// TestAuthorizeEdgeCases is iam's TestAuthorizeEdgeCases ported to authz.
func TestAuthorizeEdgeCases(t *testing.T) {
	ctx := context.Background()
	e := newEnv(t)
	alice := e.user("alice")
	bob := e.user("bob")
	aliceTree := tree("t1", alice.TenantID, "a", domain.VisibilityPrivate)
	publicTree := tree("t2", alice.TenantID, "b", domain.VisibilityPublic)

	disabled := e.user("dora")
	e.updateAccount(disabled, func(a *domain.Account) { a.Disabled, a.IsAdmin = true, true })

	carol := e.user("carol")
	e.member(alice.TenantID, carol, domain.RoleViewer)

	ghost := &domain.Principal{AccountID: "ghost", Username: "ghost"}
	// A stale principal claiming admin is not trusted: the store decides.
	fakeAdmin := *bob
	fakeAdmin.IsAdmin = true

	tests := []struct {
		name    string
		p       *domain.Principal
		act     domain.Action
		tenant  string
		tree    *domain.Tree
		want    error
		wantMsg string
	}{
		{"owner of personal tenant", alice, domain.ActTreeAdmin, alice.TenantID, nil, nil, ""},
		{"tenant derived from tree", alice, domain.ActTreeWrite, "", aliceTree, nil, ""},
		{"tree from another tenant", alice, domain.ActTreeRead, bob.TenantID, aliceTree, domain.ErrForbidden, "tree does not belong to tenant"},
		{"public tree in mismatched tenant", nil, domain.ActTreeRead, bob.TenantID, publicTree, domain.ErrUnauthenticated, "tree does not belong to tenant"},
		{"non-member private read", bob, domain.ActTreeRead, "", aliceTree, domain.ErrForbidden, "not a member of the tenant"},
		{"non-member public write", bob, domain.ActTreeWrite, "", publicTree, domain.ErrForbidden, "not a member of the tenant"},
		{"no tenant", alice, domain.ActTreeCreate, "", nil, domain.ErrForbidden, "no tenant given"},
		{"disabled admin", disabled, domain.ActTreeRead, "", publicTree, domain.ErrForbidden, "account is disabled"},
		{"disabled admin server", disabled, domain.ActServerAdmin, "", nil, domain.ErrForbidden, "account is disabled"},
		{"unknown account", ghost, domain.ActTreeRead, "", publicTree, domain.ErrUnauthenticated, "unknown account"},
		{"stale admin claim", &fakeAdmin, domain.ActServerAdmin, "", nil, domain.ErrForbidden, "server admin required"},
		{"anonymous server admin", nil, domain.ActServerAdmin, "", nil, domain.ErrUnauthenticated, "authentication required"},
		{"anonymous private read", nil, domain.ActTreeRead, "", aliceTree, domain.ErrUnauthenticated, "authentication required"},
		{"unknown action", alice, domain.Action("tree:explode"), alice.TenantID, nil, domain.ErrForbidden, "unknown action"},
		{"own tenant write", bob, domain.ActTreeWrite, bob.TenantID, nil, nil, ""},
		{"viewer cannot write", carol, domain.ActTreeWrite, "", aliceTree, domain.ErrForbidden, "role viewer does not permit it"},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := e.az.Authorize(ctx, tt.p, tt.act, tt.tenant, tt.tree)
			if tt.want == nil {
				if err != nil {
					t.Fatalf("Authorize = %v, want nil", err)
				}
				return
			}
			if !errors.Is(err, tt.want) {
				t.Fatalf("Authorize = %v, want %v", err, tt.want)
			}
			if !strings.Contains(err.Error(), tt.wantMsg) {
				t.Fatalf("Authorize = %q, want message containing %q", err, tt.wantMsg)
			}
		})
	}
}

func TestRoleOf(t *testing.T) {
	ctx := context.Background()
	e := newEnv(t)
	a := e.user("alice")
	e.member("org", a, "Maintainer")
	tests := []struct{ tenant, want string }{
		{a.TenantID, domain.RoleOwner},
		{"org", domain.RoleMaintainer},
		{"elsewhere", ""},
	}
	for _, tt := range tests {
		got, err := e.az.RoleOf(ctx, a.AccountID, tt.tenant)
		if err != nil || got != tt.want {
			t.Errorf("RoleOf(%q) = %q, %v; want %q", tt.tenant, got, err, tt.want)
		}
	}
	e.store.fail["GetMembership"] = errors.New("disk on fire")
	if _, err := e.az.RoleOf(ctx, a.AccountID, "org"); err == nil {
		t.Error("RoleOf must surface store errors")
	}
}
