package authz

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

func TestPolicyDenyOverridesAllow(t *testing.T) {
	f := newAcme(t)
	owner := f.user("owner")
	f.member(f.acme.ID, owner, domain.RoleOwner)
	pub := tree("tree-pub", f.acme.ID, "pub", domain.VisibilityPublic)
	f.policy(f.acme.ID, "lockdown", true,
		domain.PolicyStatement{Sid: "no-admin", Effect: domain.EffectDeny, Actions: strs("tree:admin"), Resources: strs("wt:tree/acme/*"), Principals: strs("*")},
		domain.PolicyStatement{Sid: "allow-admin", Effect: domain.EffectAllow, Actions: strs("tree:admin"), Resources: strs("*"), Principals: strs("user:owner")},
		domain.PolicyStatement{Sid: "no-pub-read", Effect: domain.EffectDeny, Actions: strs("tree:read"), Resources: strs("wt:tree/acme/pub"), Principals: strs("anonymous")},
	)
	ctx := context.Background()
	d, err := f.az.Evaluate(ctx, owner, domain.ActTreeAdmin, domain.Resource{Tree: f.repo}, domain.RequestEnv{})
	if err != nil {
		t.Fatal(err)
	}
	if d.Allowed || d.Effect != effectDeny {
		t.Fatalf("deny must override role and policy allows: %+v", d)
	}
	if !hasReason(d, SourceTenantRole, effectAllow) || !hasReason(d, SourcePolicy, effectAllow) || !hasReason(d, SourcePolicy, effectDeny) {
		t.Fatalf("reasons must list every source: %+v", d.Reasons)
	}
	err = f.az.Authorize(ctx, owner, domain.ActTreeAdmin, "", f.repo)
	if !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), `denied by policy "lockdown"`) {
		t.Fatalf("Authorize = %v", err)
	}
	// Deny overrides public visibility for anonymous callers only.
	if err := f.az.Authorize(ctx, nil, domain.ActTreeRead, "", pub); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous public read = %v, want denied", err)
	}
	if err := f.az.Authorize(ctx, owner, domain.ActTreeRead, "", pub); err != nil {
		t.Fatalf("owner public read = %v", err)
	}
	// Server admins bypass policies.
	admin := f.user("root")
	f.updateAccount(admin, func(a *domain.Account) { a.IsAdmin = true })
	if err := f.az.Authorize(ctx, admin, domain.ActTreeAdmin, "", f.repo); err != nil {
		t.Fatalf("admin = %v", err)
	}
}

func TestPolicyAllowWithoutRole(t *testing.T) {
	f := newAcme(t)
	bob := f.user("bob")
	f.policy(f.acme.ID, "share", true,
		stmt(domain.EffectAllow, strs("tree:read", "mr:*"), strs("wt:tree/acme/repo"), strs("user:bob")),
		stmt(domain.EffectAllow, strs("tree:read"), strs("wt:tree/acme/docs"), strs("anonymous")),
		stmt(domain.EffectAllow, strs("server:admin", "*"), strs("*"), strs("user:bob")),
	)
	f.policy(f.acme.ID, "disabled", false, stmt(domain.EffectDeny, strs("*"), strs("*"), strs("*")))
	docs := tree("tree-docs", f.acme.ID, "docs", domain.VisibilityPrivate)
	ctx := context.Background()
	if err := f.az.Authorize(ctx, bob, domain.ActMRApprove, "", f.repo); err != nil {
		t.Fatalf("policy allow: %v", err)
	}
	if err := f.az.Authorize(ctx, nil, domain.ActTreeRead, "", docs); err != nil {
		t.Fatalf("anonymous policy allow: %v", err)
	}
	if err := f.az.Authorize(ctx, nil, domain.ActTreeRead, "", f.repo); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous repo read = %v", err)
	}
	if err := f.az.Authorize(ctx, bob, domain.ActServerAdmin, "", nil); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("policy must not grant server:admin: %v", err)
	}
	if err := f.az.Authorize(ctx, bob, domain.ActServerAdmin, f.acme.ID, nil); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("policy must not grant server:admin in a tenant: %v", err)
	}
	// Policies of another tenant never apply.
	f.tenant("t-beta", "beta")
	beta := tree("tree-beta", "t-beta", "repo", domain.VisibilityPrivate)
	if err := f.az.Authorize(ctx, bob, domain.ActTreeRead, "", beta); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("cross-tenant policy applied: %v", err)
	}
}

func TestPolicyPrincipals(t *testing.T) {
	f := newAcme(t)
	alice, bob := f.user("alice"), f.user("bob")
	f.member(f.acme.ID, alice, domain.RoleDeveloper)
	f.team("tm-core", f.acme.ID, "core", alice)
	f.team("tm-foreign", "t-other", "core-foreign", bob)
	ci := f.serviceAccount("sa-ci", f.acme.ID, "ci", domain.RoleViewer)
	f.tenant("t-beta", "beta")
	bot := f.serviceAccount("sa-bot", "t-beta", "bot", domain.RoleViewer)

	tests := []struct {
		principal string
		p         *domain.Principal
		want      bool
	}{
		{"*", nil, true},
		{"*", alice, true},
		{"anonymous", nil, true},
		{"anonymous", alice, false},
		{"authenticated", nil, false},
		{"authenticated", alice, true},
		{"authenticated", ci, true},
		{"user:alice", alice, true},
		{"user:al*", alice, true},
		{"user:alice", bob, false},
		{"user:ci", ci, false},
		{"team:core", alice, true},
		{"team:co*", alice, true},
		{"team:core", bob, false},
		{"team:core-foreign", bob, false}, // team of another tenant
		{"role:developer", alice, true},
		{"role:Developer", alice, true},
		{"role:viewer", alice, false},
		{"role:viewer", ci, true},
		{"sa:ci", ci, true},
		{"sa:acme/ci", ci, true},
		{"sa:*/ci", ci, true},
		{"sa:beta/ci", ci, false},
		{"sa:bot", bot, false}, // bare name only matches the policy tenant's accounts
		{"sa:beta/bot", bot, true},
		{"sa:ci", alice, false},
	}
	for _, tt := range tests {
		f.store.policies = map[string][]domain.Policy{}
		f.policy(f.acme.ID, "p", true, stmt(domain.EffectAllow, strs("tree:admin"), strs("*"), strs(tt.principal)))
		who := "anonymous"
		if tt.p != nil {
			who = tt.p.Username
		}
		if got := f.allowed(tt.p, domain.ActTreeAdmin, f.repo); got != tt.want {
			t.Errorf("principal %q vs %s: allowed = %v, want %v", tt.principal, who, got, tt.want)
		}
	}
}

func TestPolicyResourcesAndActions(t *testing.T) {
	f := newAcme(t)
	bob := f.user("bob")
	ctx := context.Background()
	tests := []struct {
		resource string
		action   string
		act      domain.Action
		res      domain.Resource
		want     bool
	}{
		{"*", "*", domain.ActTreeRead, domain.Resource{Tree: f.repo}, true},
		{"wt:tenant/acme", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo}, true}, // ancestor
		{"wt:tenant/acme", "tenant:read", domain.ActTenantRead, domain.Resource{TenantID: f.acme.ID}, true},
		{"wt:tenant/other", "*", domain.ActTenantRead, domain.Resource{TenantID: f.acme.ID}, false},
		{"wt:tenant/*", "tenant:*", domain.ActTenantAdmin, domain.Resource{TenantID: f.acme.ID}, true},
		{"wt:tree/acme/repo", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo}, true},
		{"wt:tree/acme/rep*", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo}, true},
		{"wt:tree/acme/other", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo}, false},
		{"wt:tree/acme/repo", "tree:read", domain.ActTenantRead, domain.Resource{TenantID: f.acme.ID}, false},
		{"wt:tree/acme/repo", "tree:write", domain.ActTreeRead, domain.Resource{Tree: f.repo}, false},
		{"wt:tree/**", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo}, true},
		{"wt:tree/acme/repo", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo, Path: "src/a.go"}, true},
		{"wt:path/acme/repo/src/**", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo, Path: "src/x/y/a.go"}, true},
		{"wt:path/acme/repo/src/*", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo, Path: "src/x/a.go"}, false},
		{"wt:path/acme/repo/src/*.go", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo, Path: "/src/a.go"}, true},
		{"wt:path/acme/repo/**/*.md", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo, Path: "README.md"}, true},
		{"wt:path/acme/repo/src/**", "tree:read", domain.ActTreeRead, domain.Resource{Tree: f.repo}, false}, // descendants never cover the tree
		{"wt:branch/acme/repo/release/*", "branch:delete", domain.ActBranchDelete, domain.Resource{Tree: f.repo, Branch: "release/1.0"}, true},
		{"wt:branch/acme/repo/release/*", "branch:delete", domain.ActBranchDelete, domain.Resource{Tree: f.repo, Branch: "main"}, false},
		{"wt:branch/acme/repo/**", "branch:*", domain.ActBranchCreate, domain.Resource{Tree: f.repo, Branch: "a/b/c"}, true},
	}
	for _, tt := range tests {
		f.store.policies = map[string][]domain.Policy{}
		f.policy(f.acme.ID, "p", true, stmt(domain.EffectAllow, strs(tt.action), strs(tt.resource), strs("user:bob")))
		d, err := f.az.Evaluate(ctx, bob, tt.act, tt.res, domain.RequestEnv{})
		if err != nil {
			t.Fatal(err)
		}
		if d.Allowed != tt.want {
			t.Errorf("resource %q action %q vs %s %+v: allowed = %v, want %v", tt.resource, tt.action, tt.act, tt.res, d.Allowed, tt.want)
		}
	}
}

func TestPolicyConditions(t *testing.T) {
	f := newAcme(t)
	bob := f.user("bob")
	mfaBob := *bob
	mfaBob.MFA = true
	ny, _ := time.LoadLocation("America/New_York")
	// Wednesday 2026-10-21 10:30 in New York.
	wedMorningNY := time.Date(2026, 10, 21, 10, 30, 0, 0, ny)
	past := time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)
	future := time.Date(2030, 1, 1, 0, 0, 0, 0, time.UTC)
	now := time.Date(2026, 10, 21, 12, 0, 0, 0, time.UTC)

	tests := []struct {
		name string
		cond domain.PolicyConditions
		p    *domain.Principal
		env  domain.RequestEnv
		want bool
	}{
		{"empty", domain.PolicyConditions{}, bob, domain.RequestEnv{}, true},
		{"ip in v4 range", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8")}, bob, domain.RequestEnv{RemoteIP: "10.1.2.3"}, true},
		{"ip with port", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8")}, bob, domain.RequestEnv{RemoteIP: "10.1.2.3:5555"}, true},
		{"ip outside", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8", "192.168.0.0/16")}, bob, domain.RequestEnv{RemoteIP: "8.8.8.8"}, false},
		{"second range", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8", "192.168.0.0/16")}, bob, domain.RequestEnv{RemoteIP: "192.168.4.4"}, true},
		{"single address", domain.PolicyConditions{IPRanges: strs("203.0.113.7")}, bob, domain.RequestEnv{RemoteIP: "203.0.113.7"}, true},
		{"ipv6", domain.PolicyConditions{IPRanges: strs("2001:db8::/32")}, bob, domain.RequestEnv{RemoteIP: "[2001:db8::1]:443"}, true},
		{"v4-mapped v6 client", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8")}, bob, domain.RequestEnv{RemoteIP: "::ffff:10.0.0.1"}, true},
		{"unknown ip", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8")}, bob, domain.RequestEnv{}, false},
		{"garbage ip", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8")}, bob, domain.RequestEnv{RemoteIP: "nope"}, false},
		{"mfa required, absent", domain.PolicyConditions{RequireMFA: true}, bob, domain.RequestEnv{}, false},
		{"mfa required, present", domain.PolicyConditions{RequireMFA: true}, &mfaBob, domain.RequestEnv{}, true},
		{"not before future", domain.PolicyConditions{NotBefore: &future}, bob, domain.RequestEnv{Now: now}, false},
		{"not before past", domain.PolicyConditions{NotBefore: &past}, bob, domain.RequestEnv{Now: now}, true},
		{"not after past", domain.PolicyConditions{NotAfter: &past}, bob, domain.RequestEnv{Now: now}, false},
		{"between", domain.PolicyConditions{NotBefore: &past, NotAfter: &future}, bob, domain.RequestEnv{Now: now}, true},
		{"window inside tz", domain.PolicyConditions{TimeWindow: &domain.TimeWindow{Days: strs("mon", "tue", "wed", "thu", "fri"), Start: "09:00", End: "18:00", Timezone: "America/New_York"}},
			bob, domain.RequestEnv{Now: wedMorningNY.UTC()}, true},
		{"window wrong day", domain.PolicyConditions{TimeWindow: &domain.TimeWindow{Days: strs("sat", "sun"), Start: "09:00", End: "18:00", Timezone: "America/New_York"}},
			bob, domain.RequestEnv{Now: wedMorningNY}, false},
		{"window before start (UTC vs NY)", domain.PolicyConditions{TimeWindow: &domain.TimeWindow{Start: "11:00", End: "18:00", Timezone: "America/New_York"}},
			bob, domain.RequestEnv{Now: wedMorningNY.UTC()}, false},
		{"window default UTC", domain.PolicyConditions{TimeWindow: &domain.TimeWindow{Start: "14:00", End: "15:00"}},
			bob, domain.RequestEnv{Now: wedMorningNY}, true}, // 10:30 NY = 14:30 UTC
		{"window end exclusive", domain.PolicyConditions{TimeWindow: &domain.TimeWindow{Start: "09:00", End: "10:30", Timezone: "America/New_York"}},
			bob, domain.RequestEnv{Now: wedMorningNY}, false},
		{"all conditions", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8"), RequireMFA: true, NotBefore: &past, NotAfter: &future,
			TimeWindow: &domain.TimeWindow{Start: "00:00", End: "24:00"}}, &mfaBob, domain.RequestEnv{RemoteIP: "10.0.0.1", Now: now}, true},
		{"all but mfa", domain.PolicyConditions{IPRanges: strs("10.0.0.0/8"), RequireMFA: true, NotBefore: &past}, bob, domain.RequestEnv{RemoteIP: "10.0.0.1", Now: now}, false},
		{"broken condition never allows", domain.PolicyConditions{IPRanges: strs("not-a-cidr")}, bob, domain.RequestEnv{RemoteIP: "10.0.0.1"}, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			f.store.policies = map[string][]domain.Policy{}
			st := stmt(domain.EffectAllow, strs("tree:read"), strs("*"), strs("user:bob"))
			st.Conditions = tt.cond
			f.policy(f.acme.ID, "p", true, st)
			d, err := f.az.Evaluate(context.Background(), tt.p, domain.ActTreeRead, domain.Resource{Tree: f.repo}, tt.env)
			if err != nil {
				t.Fatal(err)
			}
			if d.Allowed != tt.want {
				t.Fatalf("allowed = %v, want %v; reasons %+v", d.Allowed, tt.want, d.Reasons)
			}
			if !tt.want && !hasReason(d, SourcePolicy, effectInfo) {
				t.Fatalf("a statement whose conditions fail must be explained: %+v", d.Reasons)
			}
		})
	}
}

func TestTimeWindowWrapsMidnight(t *testing.T) {
	w, err := compileWindow(domain.TimeWindow{Days: strs("fri"), Start: "22:00", End: "02:00", Timezone: "Europe/Berlin"})
	if err != nil {
		t.Fatal(err)
	}
	berlin, _ := time.LoadLocation("Europe/Berlin")
	at := func(day, h, m int) time.Time { return time.Date(2026, 10, day, h, m, 0, 0, berlin) } // 2026-10-23 is a Friday
	tests := []struct {
		t    time.Time
		want bool
	}{
		{at(23, 21, 59), false},
		{at(23, 22, 0), true},
		{at(23, 23, 59), true},
		{at(24, 1, 59), true}, // Saturday early morning belongs to Friday's window
		{at(24, 2, 0), false},
		{at(23, 1, 0), false}, // Friday early morning belongs to Thursday
		{at(24, 22, 30), false},
	}
	for _, tt := range tests {
		if got := w.contains(tt.t.UTC()); got != tt.want {
			t.Errorf("contains(%s) = %v, want %v", tt.t, got, tt.want)
		}
	}
	if s := w.String(); !strings.Contains(s, "fri 22:00-02:00 Europe/Berlin") {
		t.Errorf("String() = %q", s)
	}
}

func TestDenyConditionsFailClosed(t *testing.T) {
	f := newAcme(t)
	owner := f.user("owner")
	f.member(f.acme.ID, owner, domain.RoleOwner)
	deny := func(c domain.PolicyConditions) {
		f.store.policies = map[string][]domain.Policy{}
		st := stmt(domain.EffectDeny, strs("tree:write"), strs("*"), strs("*"))
		st.Conditions = c
		f.policy(f.acme.ID, "p", true, st)
	}
	check := func(name string, env domain.RequestEnv, want bool) {
		t.Helper()
		d, err := f.az.Evaluate(context.Background(), owner, domain.ActTreeWrite, domain.Resource{Tree: f.repo}, env)
		if err != nil {
			t.Fatal(err)
		}
		if d.Allowed != want {
			t.Errorf("%s: allowed = %v, want %v (%+v)", name, d.Allowed, want, d.Reasons)
		}
	}
	deny(domain.PolicyConditions{IPRanges: strs("198.51.100.0/24")})
	check("deny ip outside", domain.RequestEnv{RemoteIP: "10.0.0.1"}, true)
	check("deny ip inside", domain.RequestEnv{RemoteIP: "198.51.100.9"}, false)
	check("deny ip unknown", domain.RequestEnv{}, false)
	deny(domain.PolicyConditions{TimeWindow: &domain.TimeWindow{Start: "09:00", End: "10:00", Timezone: "Mars/Olympus"}})
	check("deny broken tz", domain.RequestEnv{}, false)
	deny(domain.PolicyConditions{RequireMFA: true})
	check("deny when mfa (no mfa)", domain.RequestEnv{}, true)
}

func TestInvalidStoredStatementsIgnored(t *testing.T) {
	f := newAcme(t)
	bob := f.user("bob")
	f.policy(f.acme.ID, "corrupt", true,
		stmt("maybe", strs("*"), strs("*"), strs("*")),
		stmt(domain.EffectAllow, strs("*"), strs("bogus-resource"), strs("*")),
	)
	if f.allowed(bob, domain.ActTreeRead, f.repo) {
		t.Fatal("malformed statements must not grant")
	}
}

func TestBranchPushDeny(t *testing.T) {
	f := newAcme(t)
	dev, lead := f.user("dev"), f.user("lead")
	f.member(f.acme.ID, dev, domain.RoleDeveloper)
	f.member(f.acme.ID, lead, domain.RoleMaintainer)
	viewer := f.user("viewer")
	f.member(f.acme.ID, viewer, domain.RoleViewer)
	f.policy(f.acme.ID, "protect-release", true,
		domain.PolicyStatement{Sid: "no-release", Effect: domain.EffectDeny, Actions: strs("branch:push"),
			Resources: strs("wt:branch/acme/repo/release/**"), Principals: strs("role:developer")},
		domain.PolicyStatement{Sid: "no-main", Effect: domain.EffectDeny, Actions: strs("tree:write"),
			Resources: strs("wt:branch/acme/*/main"), Principals: strs("user:dev")},
		domain.PolicyStatement{Sid: "freeze-other", Effect: domain.EffectDeny, Actions: strs("branch:push"),
			Resources: strs("wt:tree/acme/other"), Principals: strs("*")},
		domain.PolicyStatement{Sid: "push-allow", Effect: domain.EffectAllow, Actions: strs("branch:push"),
			Resources: strs("*"), Principals: strs("user:viewer")},
	)
	ctx := context.Background()
	push := func(p *domain.Principal, tr *domain.Tree, branch string) domain.Decision {
		t.Helper()
		d, err := f.az.Evaluate(ctx, p, domain.ActBranchPush, domain.Resource{TenantID: f.acme.ID, Tree: tr, Branch: branch}, domain.RequestEnv{})
		if err != nil {
			t.Fatal(err)
		}
		return d
	}
	tests := []struct {
		name   string
		p      *domain.Principal
		tr     *domain.Tree
		branch string
		want   bool
	}{
		{"dev feature", dev, f.repo, "feature/x", true},
		{"dev release", dev, f.repo, "release/1.0", false},
		{"dev deep release", dev, f.repo, "release/1/hotfix", false},
		{"dev main via tree:write deny on branch", dev, f.repo, "main", false},
		{"lead release", lead, f.repo, "release/1.0", true},
		{"lead main", lead, f.repo, "main", true},
		{"tree-level branch:push deny", lead, f.other, "anything", false},
		{"policy allow cannot grant push without tree:write", viewer, f.repo, "feature/x", false},
		{"no branch given = tree:write", dev, f.repo, "", true},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			d := push(tt.p, tt.tr, tt.branch)
			if d.Allowed != tt.want {
				t.Fatalf("allowed = %v, want %v; reasons %+v", d.Allowed, tt.want, d.Reasons)
			}
			if !tt.want && d.Effect == effectDeny && !hasReason(d, SourcePolicy, effectDeny) {
				t.Fatalf("explicit deny without policy reason: %+v", d.Reasons)
			}
		})
	}
	// A branch deny never affects the tree-level tree:write check.
	if err := f.az.Authorize(ctx, dev, domain.ActTreeWrite, f.acme.ID, f.repo); err != nil {
		t.Fatalf("tree:write = %v", err)
	}
}

func TestSimulatorReasons(t *testing.T) {
	f := newAcme(t)
	alice := f.user("alice")
	f.member(f.acme.ID, alice, "release-manager")
	f.role(f.acme.ID, "release-manager", "mr:*", domain.ActTreeRead)
	f.team("tm", f.acme.ID, "core", alice)
	f.grant(f.repo, domain.SubjectTeam, "tm", "core", domain.RoleDeveloper)
	f.grant(f.repo, domain.SubjectUser, alice.AccountID, "alice", domain.RoleViewer)
	office := domain.PolicyStatement{Sid: "office", Effect: domain.EffectAllow, Actions: strs("tree:admin"), Resources: strs("*"),
		Principals: strs("team:core"), Conditions: domain.PolicyConditions{IPRanges: strs("10.0.0.0/8")}}
	f.policy(f.acme.ID, "office-admin", true, office)

	scoped := *alice
	scoped.Scopes = []string{"tree:*"}
	d, err := f.az.Evaluate(context.Background(), &scoped, domain.ActTreeAdmin, domain.Resource{Tree: f.repo}, domain.RequestEnv{RemoteIP: "10.9.9.9"})
	if err != nil {
		t.Fatal(err)
	}
	if !d.Allowed || d.Effect != effectAllow {
		t.Fatalf("decision = %+v", d)
	}
	for _, want := range []struct{ source, effect, detail string }{
		{SourceTokenScope, effectInfo, `token scope "tree:*" permits tree:admin`},
		{SourceTenantRole, effectInfo, "member of tenant acme: custom role release-manager"},
		{SourceCustomRole, effectInfo, "custom role release-manager does not grant tree:admin"},
		{SourceTeamRole, effectInfo, `team core grant on tree "repo": role developer does not grant tree:admin`},
		{SourceTreeGrant, effectInfo, `grant on tree "repo": role viewer does not grant tree:admin`},
		{SourceVisibility, "", ""},
		{SourcePolicy, effectAllow, `policy "office-admin" statement "office" allows tree:admin`},
	} {
		found := false
		for _, r := range d.Reasons {
			if r.Source == want.source && (want.effect == "" || r.Effect == want.effect) && strings.Contains(r.Detail, want.detail) {
				found = true
			}
		}
		if (want.source == SourceVisibility) == found {
			t.Errorf("reason %+v present=%v; reasons:\n%s", want, found, formatReasons(d.Reasons))
		}
	}

	// Outside the office the same request is an implicit deny that says why.
	d, _ = f.az.Evaluate(context.Background(), alice, domain.ActTreeAdmin, domain.Resource{Tree: f.repo}, domain.RequestEnv{RemoteIP: "8.8.8.8"})
	if d.Allowed || d.Effect != effectImplicitDeny {
		t.Fatalf("decision = %+v", d)
	}
	if !hasDetail(d, SourcePolicy, "client IP 8.8.8.8 is outside 10.0.0.0/8") || !hasReason(d, SourceDefault, effectDeny) {
		t.Fatalf("reasons:\n%s", formatReasons(d.Reasons))
	}
	if d.Reasons == nil {
		t.Fatal("reasons must be non-nil")
	}
	err = f.az.Authorize(context.Background(), alice, domain.ActTreeAdmin, "", f.repo)
	if !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), "does not permit it") {
		t.Fatalf("Authorize = %v", err)
	}
}

func TestAuthorizeUsesRemoteAddr(t *testing.T) {
	f := newAcme(t)
	bob := f.user("bob")
	st := stmt(domain.EffectAllow, strs("tree:read"), strs("*"), strs("user:bob"))
	st.Conditions.IPRanges = strs("10.0.0.0/8")
	f.policy(f.acme.ID, "office", true, st)
	ctx := context.Background()
	if err := f.az.Authorize(ctx, bob, domain.ActTreeRead, "", f.repo); err == nil {
		t.Fatal("no client address: IP-conditioned allow must not apply")
	}
	if err := f.az.Authorize(iam.WithRemoteAddr(ctx, "10.1.1.1:4242"), bob, domain.ActTreeRead, "", f.repo); err != nil {
		t.Fatalf("Authorize from office = %v", err)
	}
	if got := RequestEnvFrom(iam.WithRemoteAddr(ctx, "[::1]:80")).RemoteIP; got != "::1" {
		t.Fatalf("RequestEnvFrom = %q", got)
	}
}

func hasDetail(d domain.Decision, source, substr string) bool {
	for _, r := range d.Reasons {
		if r.Source == source && strings.Contains(r.Detail, substr) {
			return true
		}
	}
	return false
}

func formatReasons(rs []domain.DecisionReason) string {
	var b strings.Builder
	for _, r := range rs {
		b.WriteString("  " + r.Source + " [" + r.Effect + "] " + r.Detail + "\n")
	}
	return b.String()
}
