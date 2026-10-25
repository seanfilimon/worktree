package authz

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// fixture: tenant acme with tree "repo" (private) and "other" (private).
type acmeFixture struct {
	*env
	acme        *domain.Tenant
	repo, other *domain.Tree
}

func newAcme(t *testing.T) *acmeFixture {
	e := newEnv(t)
	f := &acmeFixture{env: e, acme: e.tenant("t-acme", "acme")}
	f.repo = tree("tree-repo", f.acme.ID, "repo", domain.VisibilityPrivate)
	f.other = tree("tree-other", f.acme.ID, "other", domain.VisibilityPrivate)
	return f
}

func (f *acmeFixture) allowed(p *domain.Principal, act domain.Action, tr *domain.Tree) bool {
	f.t.Helper()
	d, err := f.az.Evaluate(context.Background(), p, act, domain.Resource{TenantID: f.acme.ID, Tree: tr}, domain.RequestEnv{})
	if err != nil {
		f.t.Fatalf("Evaluate(%s): %v", act, err)
	}
	return d.Allowed
}

func TestCustomRoles(t *testing.T) {
	f := newAcme(t)
	f.role(f.acme.ID, "deployer", "tree:*", domain.ActMRMerge)
	f.role(f.acme.ID, "auditor", domain.ActIAMRead, domain.ActTenantRead)
	f.role("t-elsewhere", "ghostly", "tree:*") // other tenant: must not apply
	dep, aud, ghost := f.user("dep"), f.user("aud"), f.user("ghost")
	f.member(f.acme.ID, dep, "deployer")
	f.member(f.acme.ID, aud, "Auditor")
	f.member(f.acme.ID, ghost, "ghostly")

	tests := []struct {
		p    *domain.Principal
		act  domain.Action
		tr   *domain.Tree
		want bool
	}{
		{dep, domain.ActTreeRead, f.repo, true},
		{dep, domain.ActTreeWrite, f.repo, true},
		{dep, domain.ActTreeAdmin, f.repo, true},
		{dep, domain.ActTreeCreate, nil, true},
		{dep, domain.ActBranchPush, f.repo, true}, // via tree:write
		{dep, domain.ActMRMerge, f.repo, true},
		{dep, domain.ActMRApprove, f.repo, false},
		{dep, domain.ActTenantRead, nil, false},
		{dep, domain.ActServerAdmin, nil, false},
		{aud, domain.ActIAMRead, nil, true},
		{aud, domain.ActIAMAdmin, nil, false},
		{aud, domain.ActTreeRead, f.repo, false},
		{ghost, domain.ActTreeRead, f.repo, false},
	}
	for _, tt := range tests {
		if got := f.allowed(tt.p, tt.act, tt.tr); got != tt.want {
			t.Errorf("%s %s = %v, want %v", tt.p.Username, tt.act, got, tt.want)
		}
	}
}

func TestCustomRoleCannotGrantServerAdmin(t *testing.T) {
	f := newAcme(t)
	f.role(f.acme.ID, "sneaky", "*", "server:*", domain.ActServerAdmin)
	p := f.user("sneak")
	f.member(f.acme.ID, p, "sneaky")
	if f.allowed(p, domain.ActServerAdmin, nil) {
		t.Fatal("custom role granted server:admin")
	}
	if !f.allowed(p, domain.ActTenantAdmin, nil) {
		t.Fatal("\"*\" in a stored custom role should still match tenant actions")
	}
}

func TestTeamGrants(t *testing.T) {
	f := newAcme(t)
	alice, bob := f.user("alice"), f.user("bob")
	f.team("team-be", f.acme.ID, "backend", alice)
	f.team("team-x", "t-other", "elsewhere", bob)
	f.grant(f.repo, domain.SubjectTeam, "team-be", "backend", domain.RoleMaintainer)
	f.grant(f.repo, domain.SubjectTeam, "team-x", "elsewhere", domain.RoleAdmin)

	tests := []struct {
		p    *domain.Principal
		act  domain.Action
		tr   *domain.Tree
		want bool
	}{
		{alice, domain.ActTreeRead, f.repo, true},
		{alice, domain.ActMRMerge, f.repo, true},
		{alice, domain.ActBranchPush, f.repo, true},
		{alice, domain.ActTreeAdmin, f.repo, false},
		{alice, domain.ActTreeRead, f.other, false}, // grant is per tree
		{alice, domain.ActTenantRead, f.repo, false},
		{alice, domain.ActTreeCreate, f.repo, false}, // tree grants never cover tenant actions
		{alice, domain.ActIAMRead, f.repo, false},
		{bob, domain.ActTreeRead, f.repo, true}, // bob's team id matches the grant (store keyed by id)
	}
	for _, tt := range tests {
		if got := f.allowed(tt.p, tt.act, tt.tr); got != tt.want {
			t.Errorf("%s %s on %v = %v, want %v", tt.p.Username, tt.act, tt.tr != nil, got, tt.want)
		}
	}
}

func TestDirectGrantsRaiseTenantRole(t *testing.T) {
	f := newAcme(t)
	viewer, outsider := f.user("viewer"), f.user("outsider")
	f.member(f.acme.ID, viewer, domain.RoleViewer)
	f.grant(f.repo, domain.SubjectUser, viewer.AccountID, "viewer", domain.RoleAdmin)
	f.role(f.acme.ID, "pusher", domain.ActTreeRead, domain.ActTreeWrite)
	f.grant(f.repo, domain.SubjectUser, outsider.AccountID, "outsider", "pusher")

	if !f.allowed(viewer, domain.ActTreeAdmin, f.repo) {
		t.Error("grant admin should raise viewer on repo")
	}
	if f.allowed(viewer, domain.ActTreeAdmin, f.other) {
		t.Error("grant must not apply to other trees")
	}
	if f.allowed(viewer, domain.ActTenantAdmin, f.repo) {
		t.Error("tree grant must not grant tenant:admin")
	}
	if !f.allowed(outsider, domain.ActTreeWrite, f.repo) {
		t.Error("custom-role grant should let a non-member write")
	}
	if f.allowed(outsider, domain.ActTenantRead, nil) {
		t.Error("a tree grant does not make a tenant member")
	}
}

func TestServiceAccounts(t *testing.T) {
	f := newAcme(t)
	f.tenant("t-beta", "beta")
	betaTree := tree("tree-beta", "t-beta", "svc", domain.VisibilityPrivate)
	betaInternal := tree("tree-beta-int", "t-beta", "int", domain.VisibilityInternal)
	ci := f.serviceAccount("sa-ci", f.acme.ID, "ci", domain.RoleDeveloper)
	f.grant(betaTree, domain.SubjectServiceAccount, "sa-ci", "ci", domain.RoleViewer)
	off := f.serviceAccount("sa-off", f.acme.ID, "off", domain.RoleOwner)
	f.store.sas["sa-off"] = domain.ServiceAccount{ID: "sa-off", TenantID: f.acme.ID, Name: "off", Role: domain.RoleOwner, Disabled: true}

	ctx := context.Background()
	eval := func(p *domain.Principal, act domain.Action, tenant string, tr *domain.Tree) domain.Decision {
		t.Helper()
		d, err := f.az.Evaluate(ctx, p, act, domain.Resource{TenantID: tenant, Tree: tr}, domain.RequestEnv{})
		if err != nil {
			t.Fatalf("Evaluate: %v", err)
		}
		return d
	}
	if !eval(ci, domain.ActTreeWrite, f.acme.ID, f.repo).Allowed {
		t.Error("ci developer should write in its tenant")
	}
	if eval(ci, domain.ActTreeAdmin, f.acme.ID, f.repo).Allowed {
		t.Error("ci developer must not admin")
	}
	if d := eval(ci, domain.ActTreeRead, "t-beta", betaTree); !d.Allowed || !hasReason(d, SourceTreeGrant, effectAllow) {
		t.Errorf("ci should read beta via grant: %+v", d)
	}
	if eval(ci, domain.ActTreeWrite, "t-beta", betaTree).Allowed {
		t.Error("ci's own role must not apply in another tenant")
	}
	if !eval(ci, domain.ActTreeRead, "t-beta", betaInternal).Allowed {
		t.Error("service accounts are authenticated: internal trees are readable")
	}
	d := eval(off, domain.ActTreeRead, f.acme.ID, f.repo)
	if d.Allowed || d.Effect != effectDeny || !hasReason(d, SourceDisabled, effectDeny) {
		t.Errorf("disabled service account: %+v", d)
	}
	// IsAdmin on a service-account principal is never trusted.
	sneaky := *ci
	sneaky.IsAdmin = true
	if eval(&sneaky, domain.ActServerAdmin, "", nil).Allowed {
		t.Error("service account claimed server admin")
	}

	// Unknown / malformed service-account principals are unauthenticated.
	for _, p := range []*domain.Principal{
		{Kind: domain.PrincipalServiceAccount, ServiceAccountID: "sa-gone"},
		{Kind: domain.PrincipalServiceAccount},
	} {
		err := f.az.Authorize(ctx, p, domain.ActTreeRead, f.acme.ID, f.repo)
		if !errors.Is(err, domain.ErrUnauthenticated) {
			t.Errorf("Authorize(%+v) = %v, want ErrUnauthenticated", p, err)
		}
	}
	if err := f.az.Authorize(ctx, off, domain.ActTreeRead, f.acme.ID, f.repo); !errors.Is(err, domain.ErrForbidden) {
		t.Errorf("disabled sa Authorize = %v", err)
	}
}

func TestTokenScopes(t *testing.T) {
	f := newAcme(t)
	owner := f.user("owner")
	f.member(f.acme.ID, owner, domain.RoleOwner)
	admin := f.user("root")
	f.updateAccount(admin, func(a *domain.Account) { a.IsAdmin = true })

	scoped := func(p *domain.Principal, scopes ...string) *domain.Principal {
		c := *p
		c.Scopes = scopes
		return &c
	}
	tests := []struct {
		name string
		p    *domain.Principal
		act  domain.Action
		want bool
	}{
		{"exact scope", scoped(owner, "tree:read"), domain.ActTreeRead, true},
		{"exact scope other action", scoped(owner, "tree:read"), domain.ActTreeWrite, false},
		{"area scope", scoped(owner, "tree:*"), domain.ActTreeAdmin, true},
		{"area scope other area", scoped(owner, "tree:*"), domain.ActMRMerge, false},
		{"star scope", scoped(owner, "*"), domain.ActTenantAdmin, true},
		{"several scopes", scoped(owner, "mr:read", "mr:*"), domain.ActMRApprove, true},
		{"branch:push needs tree:write scope", scoped(owner, "tree:write"), domain.ActBranchPush, true},
		{"branch:push scope alone", scoped(owner, "branch:push"), domain.ActBranchPush, true},
		{"branch:push scope is not tree:write", scoped(owner, "branch:push"), domain.ActTreeWrite, false},
		{"scope does not exceed role", scoped(f.user("nobody"), "*"), domain.ActTreeRead, false},
		{"admin bound by scope", scoped(admin, "tree:read"), domain.ActServerAdmin, false},
		{"admin within scope", scoped(admin, "server:*"), domain.ActServerAdmin, true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			d, err := f.az.Evaluate(context.Background(), tt.p, tt.act, domain.Resource{TenantID: f.acme.ID, Tree: f.repo}, domain.RequestEnv{})
			if err != nil {
				t.Fatal(err)
			}
			if d.Allowed != tt.want {
				t.Fatalf("allowed = %v, want %v (%+v)", d.Allowed, tt.want, d.Reasons)
			}
			if !tt.want && len(tt.p.Scopes) > 0 && tt.name != "scope does not exceed role" && !hasReason(d, SourceTokenScope, effectDeny) {
				t.Fatalf("missing token_scope deny reason: %+v", d.Reasons)
			}
		})
	}
	err := f.az.Authorize(context.Background(), scoped(owner, "tree:read"), domain.ActTreeWrite, f.acme.ID, f.repo)
	if !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("scoped Authorize = %v", err)
	}
}

func TestEvaluateInvalidResource(t *testing.T) {
	f := newAcme(t)
	p := f.user("x")
	for _, res := range []domain.Resource{
		{TenantID: f.acme.ID, Branch: "main"},
		{TenantID: f.acme.ID, Path: "a/b"},
	} {
		if _, err := f.az.Evaluate(context.Background(), p, domain.ActTreeRead, res, domain.RequestEnv{}); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("Evaluate(%+v) err = %v, want ErrInvalid", res, err)
		}
	}
}

func TestStoreErrorsFailClosed(t *testing.T) {
	for _, method := range []string{"GetAccount", "GetTenant", "GetMembership", "ListGrants", "ListPolicies", "GetRole", "ListAccountTeams", "GetTeam"} {
		t.Run(method, func(t *testing.T) {
			f := newAcme(t)
			p := f.user("alice")
			f.member(f.acme.ID, p, "custom")
			f.role(f.acme.ID, "custom", "tree:*")
			f.team("tm", f.acme.ID, "core", p)
			f.grant(f.repo, domain.SubjectTeam, "tm", "core", domain.RoleViewer)
			f.policy(f.acme.ID, "p", true, stmt(domain.EffectAllow, strs("mr:*"), strs("*"), strs("team:core")))
			f.store.fail[method] = errors.New("boom")
			err := f.az.Authorize(context.Background(), p, domain.ActTreeRead, f.acme.ID, f.repo)
			if err == nil || errors.Is(err, domain.ErrForbidden) || errors.Is(err, domain.ErrUnauthenticated) {
				t.Fatalf("Authorize = %v, want a store error", err)
			}
		})
	}
}

func TestEffectivePermissions(t *testing.T) {
	f := newAcme(t)
	dev := f.user("dev")
	f.member(f.acme.ID, dev, domain.RoleDeveloper)
	f.grant(f.repo, domain.SubjectUser, dev.AccountID, "dev", domain.RoleMaintainer)
	f.policy(f.acme.ID, "rules", true,
		stmt(domain.EffectDeny, strs("mr:create"), strs("wt:tree/acme/repo"), strs("user:dev")),
		stmt(domain.EffectAllow, strs("iam:read"), strs("wt:tenant/acme"), strs("user:dev")),
		// Request-dependent: ignored by EffectivePermissions.
		domain.PolicyStatement{Effect: domain.EffectDeny, Actions: strs("tree:read"), Resources: strs("*"), Principals: strs("*"),
			Conditions: domain.PolicyConditions{IPRanges: strs("10.0.0.0/8")}},
	)
	ctx := context.Background()
	got, err := f.az.EffectivePermissions(ctx, dev, f.acme.ID, f.repo)
	if err != nil {
		t.Fatal(err)
	}
	want := []domain.Action{
		domain.ActTenantRead, domain.ActTreeRead, domain.ActTreeWrite,
		domain.ActBranchCreate, domain.ActBranchDelete, domain.ActBranchPush,
		domain.ActMRApprove, domain.ActMRMerge,
		domain.ActReleaseCreate, domain.ActReleaseDelete, domain.ActCIReport, domain.ActIAMRead,
	}
	if !equalActions(got, want) {
		t.Errorf("tree permissions = %v\nwant %v", got, want)
	}

	got, err = f.az.EffectivePermissions(ctx, dev, f.acme.ID, nil)
	if err != nil {
		t.Fatal(err)
	}
	want = []domain.Action{
		domain.ActTenantRead, domain.ActTreeRead, domain.ActTreeWrite, domain.ActBranchCreate,
		domain.ActBranchPush, domain.ActMRCreate, domain.ActReleaseCreate, domain.ActCIReport, domain.ActIAMRead,
	}
	if !equalActions(got, want) {
		t.Errorf("tenant permissions = %v\nwant %v", got, want)
	}

	admin := f.user("root")
	f.updateAccount(admin, func(a *domain.Account) { a.IsAdmin = true })
	got, _ = f.az.EffectivePermissions(ctx, admin, f.acme.ID, nil)
	if len(got) != len(Actions()) {
		t.Errorf("admin permissions = %v, want all", got)
	}
	pub := tree("tree-pub", f.acme.ID, "pub", domain.VisibilityPublic)
	got, _ = f.az.EffectivePermissions(ctx, nil, f.acme.ID, pub)
	if !equalActions(got, []domain.Action{domain.ActTreeRead}) {
		t.Errorf("anonymous permissions = %v", got)
	}
	if got == nil {
		t.Error("EffectivePermissions must return a non-nil slice")
	}
}

func equalActions(a, b []domain.Action) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		if a[i] != b[i] {
			return false
		}
	}
	return true
}

func hasReason(d domain.Decision, source, effect string) bool {
	for _, r := range d.Reasons {
		if r.Source == source && r.Effect == effect {
			return true
		}
	}
	return false
}
