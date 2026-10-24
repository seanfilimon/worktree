package iam

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

var allActions = []domain.Action{
	domain.ActTreeRead, domain.ActTreeWrite, domain.ActTreeCreate, domain.ActTreeAdmin,
	domain.ActBranchCreate, domain.ActBranchDelete,
	domain.ActMRCreate, domain.ActMRApprove, domain.ActMRMerge,
	domain.ActTenantRead, domain.ActTenantAdmin, domain.ActServerAdmin,
}

// expectedGrants is the role matrix from doc.go, written out explicitly.
var expectedGrants = map[string]map[domain.Action]bool{
	domain.RoleViewer: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
	},
	domain.RoleDeveloper: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActMRCreate: true,
	},
	domain.RoleMaintainer: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActMRCreate: true,
		domain.ActBranchDelete: true, domain.ActMRApprove: true, domain.ActMRMerge: true, domain.ActTreeCreate: true,
	},
	domain.RoleAdmin: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActMRCreate: true,
		domain.ActBranchDelete: true, domain.ActMRApprove: true, domain.ActMRMerge: true, domain.ActTreeCreate: true,
		domain.ActTreeAdmin: true, domain.ActTenantAdmin: true,
	},
	domain.RoleOwner: {
		domain.ActTenantRead: true, domain.ActTreeRead: true,
		domain.ActTreeWrite: true, domain.ActBranchCreate: true, domain.ActMRCreate: true,
		domain.ActBranchDelete: true, domain.ActMRApprove: true, domain.ActMRMerge: true, domain.ActTreeCreate: true,
		domain.ActTreeAdmin: true, domain.ActTenantAdmin: true,
	},
	"":            {}, // non-member
	"custom-role": {},
}

func TestAuthorizeMatrix(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	tenant := &domain.Tenant{ID: "t-org", Slug: "org", Kind: domain.TenantOrg}
	if err := e.store.CreateTenant(ctx, tenant); err != nil {
		t.Fatal(err)
	}
	principals := map[string]*domain.Principal{}
	for role := range expectedGrants {
		name := "user-" + role
		if role == "" {
			name = "outsider"
		}
		a := e.register(t, name)
		if role != "" {
			if err := e.store.PutMembership(ctx, &domain.Membership{TenantID: tenant.ID, AccountID: a.ID, Role: role}); err != nil {
				t.Fatal(err)
			}
		}
		principals[role] = principalOf(a)
	}
	admin := e.register(t, "server-admin")
	admin.IsAdmin = true
	_ = e.store.UpdateAccount(ctx, admin)

	visibilities := []string{domain.VisibilityPrivate, domain.VisibilityInternal, domain.VisibilityPublic, "", "bogus"}
	for _, vis := range visibilities {
		tree := &domain.Tree{ID: "tree-" + vis, TenantID: tenant.ID, Name: "repo", Visibility: vis}
		for _, withTree := range []bool{true, false} {
			var tr *domain.Tree
			if withTree {
				tr = tree
			}
			for _, act := range allActions {
				// Server admin: everything.
				if err := e.svc.Authorize(ctx, principalOf(admin), act, tenant.ID, tr); err != nil {
					t.Errorf("admin %s vis=%q tree=%v: %v", act, vis, withTree, err)
				}

				// Anonymous: only tree:read of public trees.
				err := e.svc.Authorize(ctx, nil, act, tenant.ID, tr)
				anonOK := act == domain.ActTreeRead && withTree && vis == domain.VisibilityPublic
				checkDecision(t, "anonymous", act, vis, withTree, err, anonOK, domain.ErrUnauthenticated)

				for role, grants := range expectedGrants {
					want := grants[act]
					if act == domain.ActTreeRead && withTree &&
						(vis == domain.VisibilityPublic || vis == domain.VisibilityInternal) {
						want = true
					}
					err := e.svc.Authorize(ctx, principals[role], act, tenant.ID, tr)
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

func TestAuthorizeEdgeCases(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	bob := e.register(t, "bob")
	aliceTree := &domain.Tree{ID: "t1", TenantID: alice.TenantID, Visibility: domain.VisibilityPrivate}
	publicTree := &domain.Tree{ID: "t2", TenantID: alice.TenantID, Visibility: domain.VisibilityPublic}

	disabled := e.register(t, "dora")
	disabled.Disabled = true
	disabled.IsAdmin = true
	_ = e.store.UpdateAccount(ctx, disabled)

	ghost := &domain.Principal{AccountID: "ghost", Username: "ghost"}
	// A stale principal claiming admin is not trusted: the store decides.
	fakeAdmin := principalOf(bob)
	fakeAdmin.IsAdmin = true

	tests := []struct {
		name   string
		p      *domain.Principal
		act    domain.Action
		tenant string
		tree   *domain.Tree
		want   error
	}{
		{"owner of personal tenant", principalOf(alice), domain.ActTreeAdmin, alice.TenantID, nil, nil},
		{"tenant derived from tree", principalOf(alice), domain.ActTreeWrite, "", aliceTree, nil},
		{"tree from another tenant", principalOf(alice), domain.ActTreeRead, bob.TenantID, aliceTree, domain.ErrForbidden},
		{"public tree in mismatched tenant", nil, domain.ActTreeRead, bob.TenantID, publicTree, domain.ErrUnauthenticated},
		{"non-member private read", principalOf(bob), domain.ActTreeRead, "", aliceTree, domain.ErrForbidden},
		{"non-member public write", principalOf(bob), domain.ActTreeWrite, "", publicTree, domain.ErrForbidden},
		{"no tenant", principalOf(alice), domain.ActTreeCreate, "", nil, domain.ErrForbidden},
		{"disabled admin", principalOf(disabled), domain.ActTreeRead, "", publicTree, domain.ErrForbidden},
		{"disabled admin server", principalOf(disabled), domain.ActServerAdmin, "", nil, domain.ErrForbidden},
		{"unknown account", ghost, domain.ActTreeRead, "", publicTree, domain.ErrUnauthenticated},
		{"stale admin claim", fakeAdmin, domain.ActServerAdmin, "", nil, domain.ErrForbidden},
		{"anonymous server admin", nil, domain.ActServerAdmin, "", nil, domain.ErrUnauthenticated},
		{"unknown action", principalOf(alice), domain.Action("tree:explode"), alice.TenantID, nil, domain.ErrForbidden},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := e.svc.Authorize(ctx, tt.p, tt.act, tt.tenant, tt.tree)
			if tt.want == nil {
				if err != nil {
					t.Fatalf("Authorize = %v, want nil", err)
				}
				return
			}
			if !errors.Is(err, tt.want) {
				t.Fatalf("Authorize = %v, want %v", err, tt.want)
			}
		})
	}
}

func TestRoleOf(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	_ = e.store.PutMembership(ctx, &domain.Membership{TenantID: "org", AccountID: a.ID, Role: "Maintainer"})
	tests := []struct{ tenant, want string }{
		{a.TenantID, domain.RoleOwner},
		{"org", domain.RoleMaintainer},
		{"elsewhere", ""},
	}
	for _, tt := range tests {
		got, err := e.svc.RoleOf(ctx, a.ID, tt.tenant)
		if err != nil || got != tt.want {
			t.Errorf("RoleOf(%q) = %q, %v; want %q", tt.tenant, got, err, tt.want)
		}
	}
}

func TestRoleHelpers(t *testing.T) {
	if !RoleAtLeast(domain.RoleOwner, domain.RoleAdmin) || RoleAtLeast(domain.RoleViewer, domain.RoleDeveloper) ||
		RoleAtLeast("custom", domain.RoleViewer) || !RoleAtLeast("ADMIN", "admin") {
		t.Fatal("RoleAtLeast ordering wrong")
	}
	for _, act := range allActions {
		if RoleAllows(domain.RoleOwner, act) != (act != domain.ActServerAdmin) {
			t.Errorf("owner %s = %v", act, RoleAllows(domain.RoleOwner, act))
		}
	}
	// Superset property: each role grants everything the next lower one does.
	order := []string{domain.RoleViewer, domain.RoleDeveloper, domain.RoleMaintainer, domain.RoleAdmin, domain.RoleOwner}
	for i := 1; i < len(order); i++ {
		for _, act := range allActions {
			if RoleAllows(order[i-1], act) && !RoleAllows(order[i], act) {
				t.Errorf("%s lacks %s granted to %s", order[i], act, order[i-1])
			}
		}
	}
}
