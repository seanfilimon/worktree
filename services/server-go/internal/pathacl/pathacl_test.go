package pathacl

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

var t0 = time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)

// world is a tenant "acme" with tree "repo", owner alice and developer bob.
type world struct {
	store *meta.Memory
	az    *authz.Authorizer
	tree  *domain.Tree
	alice *domain.Principal
	bob   *domain.Principal
}

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func newWorld(t *testing.T) *world {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	w := &world{store: store, az: authz.New(store, authz.Options{})}
	must(t, store.CreateTenant(ctx, &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}))
	for _, u := range []struct{ name, role string }{{"alice", domain.RoleOwner}, {"bob", domain.RoleDeveloper}} {
		must(t, store.CreateTenant(ctx, &domain.Tenant{ID: "t-" + u.name, Slug: u.name, Name: u.name, Kind: domain.TenantUser, CreatedAt: t0}))
		must(t, store.CreateAccount(ctx, &domain.Account{ID: "a-" + u.name, Username: u.name, Email: u.name + "@example.com", TenantID: "t-" + u.name, CreatedAt: t0}))
		must(t, store.PutMembership(ctx, &domain.Membership{TenantID: "t-acme", AccountID: "a-" + u.name, Role: u.role, CreatedAt: t0}))
	}
	w.alice = &domain.Principal{AccountID: "a-alice", Username: "alice", TenantID: "t-alice"}
	w.bob = &domain.Principal{AccountID: "a-bob", Username: "bob", TenantID: "t-bob"}
	w.tree = &domain.Tree{ID: "tree-repo", TenantID: "t-acme", Name: "repo", DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0, UpdatedAt: t0}
	must(t, store.CreateTree(ctx, w.tree))
	return w
}

func (w *world) policy(t *testing.T, name string, enabled bool, st ...domain.PolicyStatement) {
	t.Helper()
	must(t, w.store.CreatePolicy(context.Background(), &domain.Policy{
		TenantID: "t-acme", Name: name, Enabled: enabled, Statements: st, Version: 1, CreatedAt: t0, UpdatedAt: t0,
	}))
}

func deny(action, resource, principal string) domain.PolicyStatement {
	return domain.PolicyStatement{Effect: domain.EffectDeny, Actions: []string{action}, Resources: []string{resource}, Principals: []string{principal}}
}

func plan(paths ...string) *domain.PushPlan {
	return &domain.PushPlan{Branch: "main", NewTip: "tip", ChangedPaths: paths}
}

func TestCheckPushPlan(t *testing.T) {
	w := newWorld(t)
	w.policy(t, "freeze-infra", true, deny("tree:write", "wt:path/acme/repo/infra/**", "user:bob"))
	w.policy(t, "disabled", false, deny("tree:write", "wt:path/acme/repo/**", "*"))
	c := New(Deps{Store: w.store, Authz: w.az})
	ctx := context.Background()

	cases := []struct {
		name  string
		p     *domain.Principal
		paths []string
		path  string // denied path; "" = allowed
	}{
		{"unrelated paths", w.bob, []string{"README.md", "src/main.go"}, ""},
		{"denied path", w.bob, []string{"README.md", "infra/prod.tf", "infra/x.tf"}, "infra/prod.tf"},
		{"deeply nested", w.bob, []string{"infra/a/b/c.yaml"}, "infra/a/b/c.yaml"},
		{"prefix is not a directory", w.bob, []string{"infrastructure.md"}, ""},
		{"other principal", w.alice, []string{"infra/prod.tf"}, ""},
		{"no changes", w.bob, nil, ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			err := c.CheckPushPlan(ctx, tc.p, w.tree, plan(tc.paths...))
			if tc.path == "" {
				if err != nil {
					t.Fatalf("CheckPushPlan = %v, want nil", err)
				}
				return
			}
			var d *domain.PushDenied
			if !errors.As(err, &d) {
				t.Fatalf("CheckPushPlan = %v, want *PushDenied", err)
			}
			want := "writing " + tc.path + " is denied by policy freeze-infra"
			if d.Kind != "access_denied" || d.Path != tc.path || d.Reason != want {
				t.Fatalf("denial = %+v, want access_denied %q", d, want)
			}
		})
	}
}

func TestCheckPushPlanReadDenyDoesNotBlockWrites(t *testing.T) {
	w := newWorld(t)
	w.policy(t, "hide", true, deny("tree:read", "wt:path/acme/repo/secret/**", "user:bob"))
	c := New(Deps{Store: w.store, Authz: w.az})
	if err := c.CheckPushPlan(context.Background(), w.bob, w.tree, plan("secret/a.txt")); err != nil {
		t.Fatalf("a tree:read path deny blocked a push: %v", err)
	}
}

func TestCheckPushPlanWithoutEvaluator(t *testing.T) {
	w := newWorld(t)
	w.policy(t, "freeze", true, deny("tree:write", "wt:path/acme/repo/**", "*"))
	c := New(Deps{Store: w.store, Authz: plainAuthorizer{}})
	if err := c.CheckPushPlan(context.Background(), w.bob, w.tree, plan("a.txt")); err != nil {
		t.Fatalf("CheckPushPlan without an AccessEvaluator = %v, want nil", err)
	}
	if err := New(Deps{Store: w.store, Authz: w.az}).CheckPushPlan(context.Background(), w.bob, w.tree, nil); err != nil {
		t.Fatalf("nil plan = %v", err)
	}
}

func TestCheckPushPlanEvaluationError(t *testing.T) {
	w := newWorld(t)
	w.policy(t, "freeze", true, deny("tree:write", "wt:path/acme/repo/**", "*"))
	boom := errors.New("boom")
	c := New(Deps{Store: w.store, Authz: failingEvaluator{err: boom}})
	err := c.CheckPushPlan(context.Background(), w.bob, w.tree, plan("a.txt"))
	var d *domain.PushDenied
	if !errors.Is(err, boom) || errors.As(err, &d) {
		t.Fatalf("CheckPushPlan = %v, want wrapped evaluation error", err)
	}
}

func TestDeniedPaths(t *testing.T) {
	w := newWorld(t)
	ctx := context.Background()
	paths := []string{"README.md", "secret/a.txt", "secret/b/c.txt", "docs/x.md", "secret/a.txt"}

	// No path policies at all: the evaluator is never consulted.
	counting := &countingEvaluator{AccessEvaluator: w.az}
	got, err := DeniedPaths(ctx, w.store, counting, w.bob, w.tree, domain.ActTreeRead, paths)
	if err != nil || got != nil || counting.n != 0 {
		t.Fatalf("no policies: got %v, %v after %d evaluations", got, err, counting.n)
	}

	// A tree-level deny and a path allow do not enable per-path evaluation.
	w.policy(t, "a-other", true,
		domain.PolicyStatement{Effect: domain.EffectAllow, Actions: []string{"tree:read"}, Resources: []string{"wt:path/acme/repo/**"}, Principals: []string{"*"}},
		deny("tree:admin", "wt:tree/acme/repo", "user:bob"))
	if got, err = DeniedPaths(ctx, w.store, counting, w.bob, w.tree, domain.ActTreeRead, paths); err != nil || got != nil || counting.n != 0 {
		t.Fatalf("no path denies: got %v, %v after %d evaluations", got, err, counting.n)
	}

	w.policy(t, "b-hide-secrets", true, deny("tree:read", "wt:path/acme/repo/secret/**", "user:bob"))
	got, err = DeniedPaths(ctx, w.store, counting, w.bob, w.tree, domain.ActTreeRead, paths)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 2 || got["secret/a.txt"] != "b-hide-secrets" || got["secret/b/c.txt"] != "b-hide-secrets" {
		t.Fatalf("denied = %v", got)
	}
	if counting.n != 4 { // duplicates are evaluated once
		t.Fatalf("evaluations = %d, want 4", counting.n)
	}

	// Alice is not covered; writes are not covered by a read deny.
	if got, err = DeniedPaths(ctx, w.store, w.az, w.alice, w.tree, domain.ActTreeRead, paths); err != nil || len(got) != 0 {
		t.Fatalf("alice: %v, %v", got, err)
	}
	if got, err = DeniedPaths(ctx, w.store, w.az, w.bob, w.tree, domain.ActTreeWrite, paths); err != nil || len(got) != 0 {
		t.Fatalf("bob write: %v, %v", got, err)
	}

	// Without a store every path is evaluated (no fast path).
	if got, err = DeniedPaths(ctx, nil, w.az, w.bob, w.tree, domain.ActTreeRead, []string{"secret/z"}); err != nil || got["secret/z"] != "b-hide-secrets" {
		t.Fatalf("no store: %v, %v", got, err)
	}
	// No evaluator: nothing is denied.
	if got, err = DeniedPaths(ctx, w.store, nil, w.bob, w.tree, domain.ActTreeRead, paths); err != nil || got != nil {
		t.Fatalf("nil evaluator: %v, %v", got, err)
	}
}

func TestDeniedPathsConditions(t *testing.T) {
	w := newWorld(t)
	past := t0.Add(-time.Hour)
	st := deny("tree:read", "wt:path/acme/repo/secret/**", "*")
	st.Conditions.NotAfter = &past // the deny lapsed
	w.policy(t, "lapsed", true, st)
	got, err := DeniedPaths(context.Background(), w.store, w.az, w.bob, w.tree, domain.ActTreeRead, []string{"secret/a"})
	if err != nil || len(got) != 0 {
		t.Fatalf("lapsed deny applied: %v, %v", got, err)
	}
}

func TestDenyingPolicy(t *testing.T) {
	cases := []struct {
		detail string
		want   string
	}{
		{`policy "no-secrets" statement "s1" denies tree:read`, "no-secrets"},
		{`policy "a \"quoted\" name" statement #2 denies tree:write`, `a "quoted" name`},
		{`something else`, ""},
	}
	for _, tc := range cases {
		d := domain.Decision{Effect: domain.EffectDeny, Reasons: []domain.DecisionReason{
			{Source: authz.SourcePolicy, Effect: "info", Detail: `policy "ignored" statement #1 does not apply`},
			{Source: authz.SourcePolicy, Effect: domain.EffectDeny, Detail: tc.detail},
		}}
		if got := DenyingPolicy(d); got != tc.want {
			t.Errorf("DenyingPolicy(%q) = %q, want %q", tc.detail, got, tc.want)
		}
	}
}

// ---- fakes ----

type plainAuthorizer struct{ domain.Authorizer }

type failingEvaluator struct {
	domain.Authorizer
	err error
}

func (f failingEvaluator) Evaluate(context.Context, *domain.Principal, domain.Action, domain.Resource, domain.RequestEnv) (domain.Decision, error) {
	return domain.Decision{}, f.err
}

func (failingEvaluator) EffectivePermissions(context.Context, *domain.Principal, string, *domain.Tree) ([]domain.Action, error) {
	return nil, nil
}

type countingEvaluator struct {
	domain.AccessEvaluator
	n int
}

func (c *countingEvaluator) Evaluate(ctx context.Context, p *domain.Principal, act domain.Action, res domain.Resource, env domain.RequestEnv) (domain.Decision, error) {
	c.n++
	return c.AccessEvaluator.Evaluate(ctx, p, act, res, env)
}
