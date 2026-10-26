package authz

// Benchmarks of authorization evaluation (hardening,
// docs/ENGINEERING_PLAN.md §15.10) for a tenant with a few hundred
// policies and tree grants. Run with
//
//	go test ./internal/authz -run '^$' -bench . -benchmem

import (
	"context"
	"fmt"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

const (
	benchPolicies = 300
	benchGrants   = 200
	benchTeams    = 50
)

type authzBench struct {
	e    *env
	dev  *domain.Principal
	tree *domain.Tree
}

// newAuthzBench builds tenant acme with benchPolicies policies of two
// statements each (a mix of user, team and tenant principals, a tenth with
// IP or time-window conditions), benchGrants grants on tree acme/repo and
// a developer who is in a few teams.
func newAuthzBench() *authzBench {
	e := &env{store: newFakeStore()}
	e.az = New(e.store, Options{})
	acme := e.tenant("t-acme", "acme")
	e.tenant("t-partner", "partner")
	dev := e.user("dev")
	e.member(acme.ID, dev, domain.RoleDeveloper)
	repo := tree("tree-repo", acme.ID, "repo", domain.VisibilityPrivate)

	others := make([]*domain.Principal, 0, benchGrants)
	for i := 0; i < benchGrants; i++ {
		u := e.user(fmt.Sprintf("user%03d", i))
		e.member(acme.ID, u, domain.RoleViewer)
		others = append(others, u)
	}
	for i := 0; i < benchTeams; i++ {
		members := []*domain.Principal{others[i], others[(i+1)%len(others)]}
		if i%10 == 0 {
			members = append(members, dev)
		}
		e.team(fmt.Sprintf("team-%02d", i), acme.ID, fmt.Sprintf("team%02d", i), members...)
	}
	for i := 0; i < benchGrants; i++ {
		switch i % 4 {
		case 0, 1:
			e.grant(repo, domain.SubjectUser, others[i].AccountID, others[i].Username, domain.RoleDeveloper)
		case 2:
			team := fmt.Sprintf("team-%02d", i%benchTeams)
			e.grant(repo, domain.SubjectTeam, team, team, domain.RoleMaintainer)
		default:
			e.grant(repo, domain.SubjectTenant, "t-partner", "partner", domain.RoleViewer)
		}
	}
	for i := 0; i < benchPolicies; i++ {
		var principal string
		switch i % 3 {
		case 0:
			principal = fmt.Sprintf("user:user%03d", i%benchGrants)
		case 1:
			principal = fmt.Sprintf("team:team%02d", i%benchTeams)
		default:
			principal = "tenant:partner"
		}
		allow := stmt(domain.EffectAllow, strs("tree:read", "mr:*"), strs(fmt.Sprintf("wt:tree/acme/repo%03d", i)), strs(principal))
		deny := stmt(domain.EffectDeny, strs("branch:push"), strs(fmt.Sprintf("wt:branch/acme/repo/release-%03d", i)), strs("*"))
		switch i % 20 {
		case 0:
			deny.Conditions.IPRanges = []string{"10.0.0.0/8", "192.168.0.0/16"}
		case 10:
			allow.Conditions.TimeWindow = &domain.TimeWindow{Days: []string{"mon", "tue", "wed", "thu", "fri"}, Start: "09:00", End: "18:00", Timezone: "Europe/Berlin"}
		}
		e.policy(acme.ID, fmt.Sprintf("p%03d", i), true, allow, deny)
	}
	return &authzBench{e: e, dev: dev, tree: repo}
}

func BenchmarkAuthorize(b *testing.B) {
	ab := newAuthzBench()
	ctx := context.Background()
	for _, act := range []domain.Action{domain.ActTreeRead, domain.ActTreeWrite, domain.ActMRApprove} {
		b.Run(string(act), func(b *testing.B) {
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				_ = ab.e.az.Authorize(ctx, ab.dev, act, ab.tree.TenantID, ab.tree)
			}
		})
	}
}

// BenchmarkEvaluateBranchPush is the per-push evaluation of Push
// (tree:write then branch:push) sharing one request cache, as syncsvc does.
func BenchmarkEvaluateBranchPush(b *testing.B) {
	ab := newAuthzBench()
	res := domain.Resource{TenantID: ab.tree.TenantID, Tree: ab.tree, Branch: "release-007"}
	env := domain.RequestEnv{RemoteIP: "203.0.113.9"}
	b.ReportAllocs()
	for i := 0; i < b.N; i++ {
		ctx := WithRequestCache(context.Background())
		if err := ab.e.az.Authorize(ctx, ab.dev, domain.ActTreeWrite, ab.tree.TenantID, ab.tree); err != nil {
			b.Fatal(err)
		}
		d, err := ab.e.az.Evaluate(ctx, ab.dev, domain.ActBranchPush, res, env)
		if err != nil {
			b.Fatal(err)
		}
		if d.Allowed {
			b.Fatal("push to release-007 should be denied")
		}
	}
}

func BenchmarkEffectivePermissions(b *testing.B) {
	ab := newAuthzBench()
	ctx := context.Background()
	b.ReportAllocs()
	for i := 0; i < b.N; i++ {
		if _, err := ab.e.az.EffectivePermissions(ctx, ab.dev, ab.tree.TenantID, ab.tree); err != nil {
			b.Fatal(err)
		}
	}
}

func BenchmarkCompilePolicies(b *testing.B) {
	ab := newAuthzBench()
	pols := ab.e.store.policies["t-acme"]
	b.ReportAllocs()
	for i := 0; i < b.N; i++ {
		if got := compilePolicies(pols); len(got) != benchPolicies {
			b.Fatalf("%d policies", len(got))
		}
	}
}
