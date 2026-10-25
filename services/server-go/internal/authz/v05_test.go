package authz

import (
	"context"
	"slices"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ListAccountMemberships backs "tenant:<slug>" principals.
func (f *fakeStore) ListAccountMemberships(_ context.Context, accountID string) ([]domain.Membership, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("ListAccountMemberships"); err != nil {
		return nil, err
	}
	out := []domain.Membership{}
	for _, m := range f.memberships {
		if m.AccountID == accountID {
			out = append(out, m)
		}
	}
	slices.SortFunc(out, func(a, b domain.Membership) int { return strings.Compare(a.TenantID, b.TenantID) })
	return out, nil
}

func (f *acmeFixture) decide(p *domain.Principal, act domain.Action, tr *domain.Tree) domain.Decision {
	f.t.Helper()
	d, err := f.az.Evaluate(context.Background(), p, act, domain.Resource{TenantID: f.acme.ID, Tree: tr}, domain.RequestEnv{})
	if err != nil {
		f.t.Fatalf("Evaluate(%s): %v", act, err)
	}
	return d
}

func hasSource(d domain.Decision, source string) bool {
	for _, r := range d.Reasons {
		if r.Source == source {
			return true
		}
	}
	return false
}

func (f *acmeFixture) setStatus(status, reason string) {
	t := f.store.tenants[f.acme.ID]
	t.Status, t.StatusReason = status, reason
	f.store.tenants[f.acme.ID] = t
}

func TestTenantStatus(t *testing.T) {
	f := newAcme(t)
	owner, dev, admin := f.user("olga"), f.user("dev"), f.user("root")
	f.member(f.acme.ID, owner, domain.RoleOwner)
	f.member(f.acme.ID, dev, domain.RoleDeveloper)
	f.updateAccount(admin, func(a *domain.Account) { a.IsAdmin = true })
	pub := tree("tree-pub", f.acme.ID, "pub", domain.VisibilityPublic)

	// Active (and "" status) tenants behave normally.
	if !f.allowed(dev, domain.ActTreeWrite, f.repo) {
		t.Fatal("developer cannot write an active tenant")
	}

	f.setStatus(domain.TenantSuspended, "unpaid")
	cases := []struct {
		p    *domain.Principal
		act  domain.Action
		tr   *domain.Tree
		want bool
	}{
		{dev, domain.ActTreeRead, f.repo, true},
		{dev, domain.ActTenantRead, nil, true},
		{owner, domain.ActIAMRead, nil, true},
		{nil, domain.ActTreeRead, pub, true},
		{dev, domain.ActTreeWrite, f.repo, false},
		{dev, domain.ActBranchPush, f.repo, false},
		{dev, domain.ActMRCreate, f.repo, false},
		{owner, domain.ActTenantAdmin, nil, false},
		{owner, domain.ActIAMAdmin, nil, false},
		{admin, domain.ActTreeWrite, f.repo, true}, // server admins are exempt
	}
	for _, c := range cases {
		d := f.decide(c.p, c.act, c.tr)
		if d.Allowed != c.want {
			t.Errorf("suspended: %v %s = %v, want %v (%+v)", c.p, c.act, d.Allowed, c.want, d.Reasons)
		}
		if !c.want && !hasSource(d, SourceTenantSuspended) {
			t.Errorf("suspended: %s denial lacks reason %s: %+v", c.act, SourceTenantSuspended, d.Reasons)
		}
	}
	d := f.decide(dev, domain.ActTreeWrite, f.repo)
	if d.Effect != "deny" || !strings.Contains(d.Reasons[len(d.Reasons)-1].Detail, "unpaid") {
		t.Errorf("suspension decision = %+v", d)
	}
	// Authorize wraps ErrForbidden with the short message.
	err := f.az.Authorize(context.Background(), dev, domain.ActTreeWrite, f.acme.ID, f.repo)
	if err == nil || !strings.Contains(err.Error(), "tenant is suspended") {
		t.Errorf("Authorize = %v", err)
	}

	f.setStatus(domain.TenantDeactivated, "")
	for _, c := range []struct {
		p    *domain.Principal
		act  domain.Action
		tr   *domain.Tree
		want bool
	}{
		{dev, domain.ActTreeRead, f.repo, false},
		{owner, domain.ActTenantRead, nil, false},
		{nil, domain.ActTreeRead, pub, false},
		{admin, domain.ActTreeAdmin, f.repo, true},
	} {
		d := f.decide(c.p, c.act, c.tr)
		if d.Allowed != c.want {
			t.Errorf("deactivated: %s = %v, want %v", c.act, d.Allowed, c.want)
		}
		if !c.want && !hasSource(d, SourceTenantDeactivated) {
			t.Errorf("deactivated: missing reason: %+v", d.Reasons)
		}
	}

	f.setStatus(domain.TenantActive, "")
	if !f.allowed(dev, domain.ActTreeWrite, f.repo) {
		t.Fatal("resuming the tenant did not restore access")
	}
}

func TestOwnerLockoutGuard(t *testing.T) {
	f := newAcme(t)
	owner, admin := f.user("olga"), f.user("adam")
	f.member(f.acme.ID, owner, domain.RoleOwner)
	f.member(f.acme.ID, admin, domain.RoleAdmin)
	f.policy(f.acme.ID, "lock-everyone-out", true,
		stmt(domain.EffectDeny, strs("*"), strs("*"), strs("*")))

	for _, act := range []domain.Action{domain.ActIAMAdmin, domain.ActIAMRead, domain.ActTenantAdmin} {
		d := f.decide(owner, act, nil)
		if !d.Allowed {
			t.Errorf("owner %s denied by policy: %+v", act, d.Reasons)
		}
		if !hasSource(d, SourceOwnerProtected) {
			t.Errorf("owner %s: missing %s reason: %+v", act, SourceOwnerProtected, d.Reasons)
		}
		if f.allowed(admin, act, nil) {
			t.Errorf("admin %s should be denied (only owners are protected)", act)
		}
	}
	// Other actions are still denied for owners.
	if f.allowed(owner, domain.ActTreeWrite, f.repo) || f.allowed(owner, domain.ActTreeAdmin, f.repo) {
		t.Error("owner protection must only cover iam:* and tenant:admin")
	}
	// An owner only through a tree grant is not a tenant owner.
	g := f.user("gus")
	f.grant(f.repo, domain.SubjectUser, g.AccountID, "gus", domain.RoleOwner)
	if d := f.decide(g, domain.ActTreeAdmin, f.repo); d.Allowed || hasSource(d, SourceOwnerProtected) {
		t.Errorf("tree-grant owner: %+v", d)
	}
}

func TestTenantGrantsAndPrincipals(t *testing.T) {
	f := newAcme(t)
	partner := f.tenant("t-partner", "partner")
	pat, out := f.user("pat"), f.user("outsider")
	f.member(partner.ID, pat, domain.RoleViewer)
	ci := f.serviceAccount("sa-ci", partner.ID, "ci", domain.RoleViewer)
	other := f.serviceAccount("sa-x", "tenant-outsider", "x", domain.RoleOwner)

	// Before the grant, partner members have nothing on acme/repo.
	if f.allowed(pat, domain.ActTreeRead, f.repo) {
		t.Fatal("partner member reads acme/repo without a grant")
	}
	f.grant(f.repo, domain.SubjectTenant, partner.ID, "partner", domain.RoleDeveloper)
	for _, p := range []*domain.Principal{pat, ci} {
		d := f.decide(p, domain.ActTreeWrite, f.repo)
		if !d.Allowed || !hasSource(d, SourceTreeGrant) {
			t.Errorf("%s: tenant grant does not apply: %+v", p.Username, d)
		}
		if f.allowed(p, domain.ActTreeWrite, f.other) {
			t.Errorf("%s: tenant grant leaked to another tree", p.Username)
		}
		if f.allowed(p, domain.ActTenantRead, nil) {
			t.Errorf("%s: tree grants must not give tenant actions", p.Username)
		}
	}
	for _, p := range []*domain.Principal{out, other} {
		if f.allowed(p, domain.ActTreeRead, f.repo) {
			t.Errorf("%s is not in partner but got the grant", p.Username)
		}
	}

	// tenant:<slug> principals in policies.
	f.policy(f.acme.ID, "partner-no-push", true,
		stmt(domain.EffectDeny, strs("branch:push"), strs("wt:branch/acme/repo/main"), strs("tenant:partner")))
	if f.allowedBranch(pat, "main") || f.allowedBranch(ci, "main") {
		t.Error("tenant:partner deny did not apply")
	}
	if !f.allowedBranch(pat, "dev") {
		t.Error("deny on main leaked to dev")
	}
	f.policy(f.acme.ID, "partner-read-other", true,
		stmt(domain.EffectAllow, strs("tree:read"), strs("wt:tree/acme/other"), strs("tenant:part*")))
	if !f.allowed(pat, domain.ActTreeRead, f.other) || !f.allowed(ci, domain.ActTreeRead, f.other) {
		t.Error("tenant:part* allow did not apply")
	}
	if f.allowed(out, domain.ActTreeRead, f.other) {
		t.Error("tenant principal matched an outsider")
	}
}

func (f *acmeFixture) allowedBranch(p *domain.Principal, branch string) bool {
	f.t.Helper()
	d, err := f.az.Evaluate(context.Background(), p, domain.ActBranchPush, domain.Resource{TenantID: f.acme.ID, Tree: f.repo, Branch: branch}, domain.RequestEnv{})
	if err != nil {
		f.t.Fatal(err)
	}
	return d.Allowed
}

func TestCandidatePolicies(t *testing.T) {
	f := newAcme(t)
	dev := f.user("dev")
	f.member(f.acme.ID, dev, domain.RoleDeveloper)
	f.policy(f.acme.ID, "deny-write", true, stmt(domain.EffectDeny, strs("tree:write"), strs("*"), strs("*")))
	ctx := context.Background()
	res := domain.Resource{TenantID: f.acme.ID, Tree: f.repo}

	d, _ := f.az.Evaluate(ctx, dev, domain.ActTreeWrite, res, domain.RequestEnv{})
	if d.Allowed {
		t.Fatal("stored deny does not apply")
	}
	// Candidates replace the stored policies.
	cctx := WithCandidatePolicies(ctx, f.acme.ID, []domain.Policy{})
	if d, _ := f.az.Evaluate(cctx, dev, domain.ActTreeWrite, res, domain.RequestEnv{}); !d.Allowed {
		t.Fatalf("empty candidate set still denies: %+v", d)
	}
	cctx = WithCandidatePolicies(ctx, f.acme.ID, []domain.Policy{
		{Name: "draft", Enabled: true, Statements: []domain.PolicyStatement{stmt(domain.EffectDeny, strs("tree:read"), strs("*"), strs("user:dev"))}},
		{Name: "disabled", Enabled: false, Statements: []domain.PolicyStatement{stmt(domain.EffectDeny, strs("tree:write"), strs("*"), strs("*"))}},
	})
	if d, _ := f.az.Evaluate(cctx, dev, domain.ActTreeRead, res, domain.RequestEnv{}); d.Allowed {
		t.Error("candidate deny did not apply")
	}
	if d, _ := f.az.Evaluate(cctx, dev, domain.ActTreeWrite, res, domain.RequestEnv{}); !d.Allowed {
		t.Error("disabled candidate applied, or the stored policy leaked")
	}
	// Other tenants keep their stored policies.
	f.policy("tenant-dev", "own", true, stmt(domain.EffectDeny, strs("tenant:read"), strs("*"), strs("*")))
	if d, _ := f.az.Evaluate(cctx, dev, domain.ActTenantRead, domain.Resource{TenantID: "tenant-dev"}, domain.RequestEnv{}); d.Allowed {
		t.Error("candidates leaked into another tenant")
	}
}

func TestExpandActions(t *testing.T) {
	got := ExpandActions([]string{"release:*", "tree:read", "tree:read", "bogus"})
	want := []domain.Action{domain.ActTreeRead, domain.ActReleaseCreate, domain.ActReleaseDelete}
	if !slices.Equal(got, want) {
		t.Errorf("ExpandActions = %v, want %v", got, want)
	}
	all := ExpandActions([]string{"*"})
	if slices.Contains(all, domain.ActServerAdmin) || len(all) != len(catalog)-1 {
		t.Errorf("ExpandActions(*) = %v", all)
	}
	if ValidatePrincipal("tenant:partner") != nil || ValidatePrincipal("tenant:") == nil || ValidatePrincipal("tenant:a/b") == nil {
		t.Error("tenant principal validation")
	}
}
