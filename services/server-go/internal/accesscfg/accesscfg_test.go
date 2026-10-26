package accesscfg

import (
	"context"
	"encoding/json"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

type recAudit struct {
	mu     sync.Mutex
	events []domain.AuditEvent
}

func (a *recAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, e)
}

func (a *recAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}

type env struct {
	t     *testing.T
	ctx   context.Context
	store *meta.Memory
	svc   *Service
	audit *recAudit
	bus   *events.Bus
	acme  *domain.Tenant
	web   *domain.Tree
	users map[string]*domain.Principal
}

func newEnv(t *testing.T) *env {
	t.Helper()
	ctx := context.Background()
	st := meta.NewMemory()
	e := &env{t: t, ctx: ctx, store: st, audit: &recAudit{}, bus: events.NewBus(), users: map[string]*domain.Principal{}}
	e.svc = New(Deps{Store: st, Authz: authz.New(st, authz.Options{}), Audit: e.audit, Bus: e.bus})
	e.acme = &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg}
	e.must(st.CreateTenant(ctx, e.acme))
	e.must(st.CreateTenant(ctx, &domain.Tenant{ID: "t-partner", Slug: "partner", Name: "Partner", Kind: domain.TenantOrg}))
	e.web = &domain.Tree{ID: "tree-web", TenantID: e.acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	e.must(st.CreateTree(ctx, e.web))
	for name, role := range map[string]string{"olga": domain.RoleOwner, "adam": domain.RoleAdmin, "dev": domain.RoleDeveloper, "root": ""} {
		acct := &domain.Account{ID: "acct-" + name, Username: name, Email: name + "@example.com", TenantID: "t-acme", IsAdmin: name == "root"}
		e.must(st.CreateAccount(ctx, acct))
		if role != "" {
			e.must(st.PutMembership(ctx, &domain.Membership{TenantID: e.acme.ID, AccountID: acct.ID, Role: role}))
		}
		e.users[name] = &domain.Principal{AccountID: acct.ID, Username: name, IsAdmin: acct.IsAdmin}
	}
	e.must(st.CreateTeam(ctx, &domain.Team{ID: "team-core", TenantID: e.acme.ID, Slug: "core", Name: "Core"}))
	return e
}

func (e *env) must(err error) {
	e.t.Helper()
	if err != nil {
		e.t.Fatal(err)
	}
}

func (e *env) apply(as string, tree *domain.Tree, doc any, dry bool) *Result {
	e.t.Helper()
	res, err := e.applyErr(as, tree, doc, dry)
	if err != nil {
		e.t.Fatalf("Apply: %v", err)
	}
	return res
}

func (e *env) applyErr(as string, tree *domain.Tree, doc any, dry bool) (*Result, error) {
	e.t.Helper()
	var raw []byte
	switch d := doc.(type) {
	case string:
		raw = []byte(d)
	default:
		var err error
		if raw, err = json.Marshal(d); err != nil {
			e.t.Fatal(err)
		}
	}
	return e.svc.Apply(e.ctx, e.users[as], e.acme, tree, raw, dry)
}

func (e *env) policies() map[string]domain.Policy {
	e.t.Helper()
	ps, err := e.store.ListPolicies(e.ctx, e.acme.ID)
	e.must(err)
	out := map[string]domain.Policy{}
	for _, p := range ps {
		out[p.Name] = p
	}
	return out
}

func (e *env) role(name string) *domain.CustomRole {
	r, err := e.store.GetRole(e.ctx, e.acme.ID, name)
	if errors.Is(err, domain.ErrNotFound) {
		return nil
	}
	e.must(err)
	return r
}

func deny(actions, resources, principals []string) domain.PolicyStatement {
	return domain.PolicyStatement{Effect: "deny", Actions: actions, Resources: resources, Principals: principals}
}

func allow(actions, resources, principals []string) domain.PolicyStatement {
	return domain.PolicyStatement{Effect: "allow", Actions: actions, Resources: resources, Principals: principals}
}

func l(s ...string) []string { return s }

func baseDoc() Document {
	return Document{
		Roles: []domain.CustomRole{{Name: "release-manager", Description: "Cut releases", Permissions: []domain.Action{"tree:read", "release:*", "mr:merge"}}},
		Policies: []domain.Policy{{
			Name: "protect-main", Description: "Only maintainers push to main", Enabled: true,
			Statements: []domain.PolicyStatement{
				{Sid: "no-direct-main", Effect: "deny", Actions: l("branch:push"), Resources: l("wt:branch/acme/web/main"),
					Principals: l("role:developer", "team:core", "user:dev", "tenant:partner"), Conditions: domain.PolicyConditions{RequireMFA: true}},
			},
		}},
	}
}

func TestApplyCreatesUpdatesDeletes(t *testing.T) {
	e := newEnv(t)

	// Plan (dry run) changes nothing.
	res := e.apply("olga", nil, baseDoc(), true)
	if !res.Accepted || len(res.Errors) != 0 {
		t.Fatalf("plan rejected: %+v", res)
	}
	want := []string{"role release-manager: create (3 permissions)", "policy protect-main: create (1 statement)"}
	if strings.Join(res.Changes, "|") != strings.Join(want, "|") {
		t.Fatalf("plan = %q, want %q", res.Changes, want)
	}
	if e.role("release-manager") != nil || len(e.policies()) != 0 || res.Doc != nil {
		t.Fatal("dry run wrote something")
	}
	if _, err := e.svc.Get(e.ctx, e.users["olga"], e.acme, nil); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("Get before apply = %v", err)
	}

	// Apply.
	res = e.apply("olga", nil, baseDoc(), false)
	if !res.Accepted || len(res.Changes) != 2 || res.Doc == nil {
		t.Fatalf("apply = %+v", res)
	}
	r := e.role("release-manager")
	if r == nil || r.ManagedBy != domain.ManagedByConfig || len(r.Permissions) != 3 || r.Permissions[0] != "mr:merge" {
		t.Fatalf("role = %+v", r)
	}
	p, ok := e.policies()["protect-main"]
	if !ok || p.ManagedBy != domain.ManagedByConfig || p.TreeID != "" || !p.Enabled || p.Version != 1 || p.CreatedBy != "olga" {
		t.Fatalf("policy = %+v", p)
	}
	doc, err := e.svc.Get(e.ctx, e.users["adam"], e.acme, nil)
	if err != nil || doc.Kind != KindCombined || doc.UpdatedBy != "olga" || len(doc.Hash) != 64 {
		t.Fatalf("Get = %+v, %v", doc, err)
	}
	if len(e.audit.events) != 1 || e.audit.events[0].Action != domain.EventAccessApplied ||
		!strings.Contains(e.audit.events[0].Details["changes"], "policy protect-main: create") {
		t.Fatalf("audit = %+v", e.audit.events)
	}

	// Re-applying the same document is a no-op.
	res = e.apply("olga", nil, baseDoc(), false)
	if !res.Accepted || len(res.Changes) != 0 {
		t.Fatalf("idempotent apply = %+v", res.Changes)
	}
	if p2 := e.policies()["protect-main"]; p2.Version != 1 {
		t.Fatalf("no-op apply bumped the version: %d", p2.Version)
	}

	// Update and delete.
	d := baseDoc()
	d.Roles[0].Permissions = []domain.Action{"tree:read"}
	d.Policies[0].Enabled = false
	d.Policies = append(d.Policies, domain.Policy{Name: "readers", Enabled: true,
		Statements: []domain.PolicyStatement{allow(l("tree:read"), l("wt:tree/acme/web"), l("tenant:partner"))}})
	res = e.apply("olga", nil, d, false)
	want = []string{"role release-manager: update (1 permission)", "policy protect-main: update (1 statement, disabled)", "policy readers: create (1 statement)"}
	if strings.Join(res.Changes, "|") != strings.Join(want, "|") {
		t.Fatalf("changes = %q", res.Changes)
	}
	if p := e.policies()["protect-main"]; p.Enabled || p.Version != 2 || p.ID == "" {
		t.Fatalf("updated policy = %+v", p)
	}
	res = e.apply("olga", nil, Document{}, false)
	want = []string{"policy protect-main: delete", "policy readers: delete", "role release-manager: delete"}
	if strings.Join(sortedCopy(res.Changes), "|") != strings.Join(want, "|") {
		t.Fatalf("delete changes = %q", res.Changes)
	}
	if e.role("release-manager") != nil || len(e.policies()) != 0 {
		t.Fatal("managed items survived an empty document")
	}
}

func sortedCopy(s []string) []string {
	out := append([]string(nil), s...)
	for i := range out {
		for j := i + 1; j < len(out); j++ {
			if out[j] < out[i] {
				out[i], out[j] = out[j], out[i]
			}
		}
	}
	return out
}

func TestApplyLeavesRESTManagedAloneAndAdopts(t *testing.T) {
	e := newEnv(t)
	e.must(e.store.PutRole(e.ctx, &domain.CustomRole{TenantID: e.acme.ID, Name: "manual", Permissions: []domain.Action{"tree:read"}}))
	e.must(e.store.PutRole(e.ctx, &domain.CustomRole{TenantID: e.acme.ID, Name: "release-manager", Permissions: []domain.Action{"tree:read"}}))
	e.must(e.store.CreatePolicy(e.ctx, &domain.Policy{TenantID: e.acme.ID, Name: "hand-made", Enabled: true,
		Statements: []domain.PolicyStatement{deny(l("tree:admin"), l("*"), l("user:dev"))}}))

	res := e.apply("olga", nil, baseDoc(), false)
	if !res.Accepted {
		t.Fatalf("%+v", res)
	}
	if !strings.Contains(strings.Join(res.Changes, "|"), "role release-manager: update (3 permissions, now managed by config)") {
		t.Fatalf("adoption not reported: %q", res.Changes)
	}
	if r := e.role("release-manager"); r.ManagedBy != domain.ManagedByConfig {
		t.Fatal("role not adopted")
	}
	res = e.apply("olga", nil, Document{}, false)
	if e.role("manual") == nil || e.policies()["hand-made"].Name == "" {
		t.Fatalf("REST-managed items were deleted: %q", res.Changes)
	}
}

func TestApplyRefusesDeletingAssignedRole(t *testing.T) {
	e := newEnv(t)
	e.apply("olga", nil, baseDoc(), false)
	e.must(e.store.PutMembership(e.ctx, &domain.Membership{TenantID: e.acme.ID, AccountID: "acct-dev", Role: "release-manager"}))
	res := e.apply("olga", nil, Document{Policies: baseDoc().Policies}, false)
	if res.Accepted || !strings.Contains(strings.Join(res.Errors, "|"), "role release-manager: cannot be deleted while it is assigned (tenant membership)") {
		t.Fatalf("res = %+v", res)
	}
	if e.role("release-manager") == nil {
		t.Fatal("assigned role deleted")
	}
}

func TestApplyValidation(t *testing.T) {
	e := newEnv(t)
	st := func(s domain.PolicyStatement) Document {
		return Document{Policies: []domain.Policy{{Name: "p", Enabled: true, Statements: []domain.PolicyStatement{s}}}}
	}
	cases := []struct {
		name string
		tree *domain.Tree
		doc  any
		want string
	}{
		{"empty", nil, "", "content is empty"},
		{"not json", nil, "roles = []", "not a valid access document"},
		{"unknown field", nil, `{"rolez": []}`, "unknown field"},
		{"trailing", nil, `{} {}`, "trailing data"},
		{"unknown action", nil, st(deny(l("tree:fly"), l("*"), l("*"))), `unknown action "tree:fly"`},
		{"bad principal", nil, st(deny(l("tree:read"), l("*"), l("group:x"))), "unknown principal"},
		{"bad resource", nil, st(deny(l("tree:read"), l("acme/web"), l("*"))), "must be"},
		{"missing user", nil, st(deny(l("tree:read"), l("*"), l("user:ghost"))), "principals[0]: user:ghost does not exist"},
		{"missing team", nil, st(deny(l("tree:read"), l("*"), l("team:ghosts"))), "team:ghosts does not exist"},
		{"missing tenant", nil, st(deny(l("tree:read"), l("*"), l("tenant:nowhere"))), "tenant:nowhere does not exist"},
		{"missing sa", nil, st(deny(l("tree:read"), l("*"), l("sa:bot"))), "sa:bot does not exist"},
		{"missing role", nil, st(deny(l("tree:read"), l("*"), l("role:ghost"))), "role:ghost does not exist"},
		{"duplicate policy", nil, Document{Policies: []domain.Policy{{Name: "p", Enabled: true}, {Name: "P", Enabled: true}}}, "duplicate policy name"},
		{"duplicate role", nil, Document{Roles: []domain.CustomRole{{Name: "r1"}, {Name: "r1"}}}, "duplicate role name"},
		{"builtin role", nil, Document{Roles: []domain.CustomRole{{Name: "admin"}}}, "is a built-in role"},
		{"bad role name", nil, Document{Roles: []domain.CustomRole{{Name: "Bad Name"}}}, "name must be 2-40 characters"},
		{"role server perm", nil, Document{Roles: []domain.CustomRole{{Name: "r1", Permissions: []domain.Action{"server:admin"}}}}, "server permissions cannot be granted"},
		{"role unknown perm", nil, Document{Roles: []domain.CustomRole{{Name: "r1", Permissions: []domain.Action{"x:y"}}}}, `unknown action "x:y"`},
		{"policy no name", nil, Document{Policies: []domain.Policy{{Name: " "}}}, "name is required"},
		{"tree allow", e.web, st(allow(l("tree:read"), l("wt:tree/acme/web"), l("*"))), "may only contain deny statements"},
		{"tree outside", e.web, st(deny(l("tree:read"), l("wt:tree/acme/docs"), l("*"))), "is outside tree acme/web"},
		{"tree star", e.web, st(deny(l("tree:read"), l("*"), l("*"))), "is outside tree"},
		{"tree roles", e.web, Document{Roles: []domain.CustomRole{{Name: "r1"}}}, "cannot define roles"},
	}
	for _, c := range cases {
		res := e.apply("olga", c.tree, c.doc, false)
		if res.Accepted || !strings.Contains(strings.Join(res.Errors, "|"), c.want) {
			t.Errorf("%s: res = %+v, want error containing %q", c.name, res, c.want)
		}
	}
	if len(e.policies()) != 0 || len(e.audit.events) != 0 {
		t.Fatal("a rejected document changed something")
	}
	// Roles defined in the same document may be referenced.
	ok := Document{Roles: []domain.CustomRole{{Name: "auditor", Permissions: []domain.Action{"iam:read"}}},
		Policies: []domain.Policy{{Name: "p", Enabled: true, Statements: []domain.PolicyStatement{deny(l("tree:admin"), l("*"), l("role:auditor", "user:d*"))}}}}
	if res := e.apply("olga", nil, ok, true); !res.Accepted {
		t.Fatalf("valid document rejected: %+v", res.Errors)
	}
}

func TestApplyAuthorizationAndEscalation(t *testing.T) {
	e := newEnv(t)
	// Developers cannot apply tenant documents at all.
	if _, err := e.applyErr("dev", nil, baseDoc(), true); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("developer apply = %v", err)
	}
	if _, err := e.svc.Apply(e.ctx, nil, e.acme, nil, []byte("{}"), true); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous apply = %v", err)
	}

	// An IAM manager without tree:admin may not create a role or allow
	// statement granting it.
	e.must(e.store.PutRole(e.ctx, &domain.CustomRole{TenantID: e.acme.ID, Name: "iam-manager", Permissions: []domain.Action{"iam:*", "tree:read", "tenant:read"}}))
	e.must(e.store.PutMembership(e.ctx, &domain.Membership{TenantID: e.acme.ID, AccountID: "acct-dev", Role: "iam-manager"}))
	esc := Document{Roles: []domain.CustomRole{{Name: "superuser", Permissions: []domain.Action{"tree:*"}}}}
	res := e.apply("dev", nil, esc, true)
	if res.Accepted || !strings.Contains(strings.Join(res.Errors, "|"), "role superuser grants tree:write, tree:create, tree:admin, tree:fork, which you do not have yourself") {
		t.Fatalf("escalating role = %+v", res)
	}
	escPol := Document{Policies: []domain.Policy{{Name: "open", Enabled: true,
		Statements: []domain.PolicyStatement{allow(l("tree:write"), l("*"), l("user:dev"))}}}}
	res = e.apply("dev", nil, escPol, true)
	if res.Accepted || !strings.Contains(strings.Join(res.Errors, "|"), "policy open statement 1 grants tree:write") {
		t.Fatalf("escalating allow = %+v", res)
	}
	// Within their own permissions it is fine; deny statements never escalate.
	fine := Document{Roles: []domain.CustomRole{{Name: "reader", Permissions: []domain.Action{"tree:read"}}},
		Policies: []domain.Policy{{Name: "d", Enabled: true, Statements: []domain.PolicyStatement{deny(l("*"), l("*"), l("user:adam"))}}}}
	if res := e.apply("dev", nil, fine, true); !res.Accepted {
		t.Fatalf("non-escalating doc = %+v", res.Errors)
	}
	// Disabled allow statements do not count.
	escPol.Policies[0].Enabled = false
	if res := e.apply("dev", nil, escPol, true); !res.Accepted {
		t.Fatalf("disabled allow counted: %+v", res.Errors)
	}
	// Owners and server admins skip the check.
	for _, who := range []string{"olga", "root"} {
		if res := e.apply(who, nil, esc, true); !res.Accepted {
			t.Fatalf("%s: %+v", who, res.Errors)
		}
	}
	// Suspended tenants refuse applying (a write).
	acme := *e.acme
	acme.Status = domain.TenantSuspended
	e.must(e.store.UpdateTenant(e.ctx, &acme))
	if _, err := e.applyErr("olga", nil, baseDoc(), true); !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), "suspended") {
		t.Fatalf("suspended apply = %v", err)
	}
}

func TestTreeDocuments(t *testing.T) {
	e := newEnv(t)
	tdoc := Document{Policies: []domain.Policy{{Name: "freeze", Enabled: true, Statements: []domain.PolicyStatement{
		deny(l("branch:push"), l("wt:branch/acme/web/release/*"), l("*")),
		deny(l("tree:read"), l("wt:path/acme/web/secret/**"), l("role:viewer")),
	}}}}
	// Developers lack tree:admin on the tree.
	if _, err := e.applyErr("dev", e.web, tdoc, false); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("developer tree apply = %v", err)
	}
	// A tree grant of admin is enough.
	e.must(e.store.PutGrant(e.ctx, &domain.Grant{TreeID: e.web.ID, SubjectType: domain.SubjectUser, SubjectID: "acct-dev", Subject: "dev", Role: domain.RoleAdmin}))
	res := e.apply("dev", e.web, tdoc, false)
	if !res.Accepted || strings.Join(res.Changes, "|") != "policy freeze: create (2 statements)" {
		t.Fatalf("tree apply = %+v", res)
	}
	p, ok := e.policies()["web/freeze"]
	if !ok || p.TreeID != e.web.ID || p.ManagedBy != domain.ManagedByConfig {
		t.Fatalf("tree policy = %+v (%v)", p, e.policies())
	}
	// The tenant document does not touch tree policies, and vice versa.
	e.apply("olga", nil, baseDoc(), false)
	e.apply("olga", nil, Document{}, false)
	if _, ok := e.policies()["web/freeze"]; !ok {
		t.Fatal("tenant document deleted a tree policy")
	}
	// Tree documents are readable with tree:read.
	if doc, err := e.svc.Get(e.ctx, e.users["dev"], e.acme, e.web); err != nil || doc.TreeID != e.web.ID {
		t.Fatalf("Get tree doc = %+v, %v", doc, err)
	}
	// Deleting the tree removes its document and policies.
	e.must(e.svc.TreeDeleted(e.ctx, e.acme.ID, e.web.ID))
	if _, ok := e.policies()["web/freeze"]; ok {
		t.Fatal("tree policy survived tree deletion")
	}
	if _, err := e.svc.Get(e.ctx, e.users["olga"], e.acme, e.web); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("tree doc survived: %v", err)
	}
}

func TestRunDropsDeletedTrees(t *testing.T) {
	e := newEnv(t)
	e.apply("olga", e.web, Document{Policies: []domain.Policy{{Name: "f", Enabled: true,
		Statements: []domain.PolicyStatement{deny(l("tree:write"), l("wt:tree/acme/web"), l("*"))}}}}, false)
	ctx, cancel := context.WithCancel(e.ctx)
	defer cancel()
	done := make(chan struct{})
	go func() { e.svc.Run(ctx); close(done) }()
	deadline := time.Now().Add(5 * time.Second)
	for len(e.policies()) > 0 && time.Now().Before(deadline) {
		e.bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: e.acme.ID, TreeID: e.web.ID})
		time.Sleep(20 * time.Millisecond)
	}
	if len(e.policies()) != 0 {
		t.Fatal("Run did not clean up the deleted tree")
	}
	cancel()
	<-done
}

func TestEvaluationUsesAppliedPolicies(t *testing.T) {
	e := newEnv(t)
	az := authz.New(e.store, authz.Options{})
	e.apply("olga", nil, baseDoc(), false)
	res := domain.Resource{TenantID: e.acme.ID, Tree: e.web, Branch: "main"}
	dev := e.users["dev"]
	// The statement's require_mfa condition limits the deny to MFA
	// sessions.
	d, err := az.Evaluate(e.ctx, dev, domain.ActBranchPush, res, domain.RequestEnv{})
	if err != nil || !d.Allowed {
		t.Fatalf("deny applied without its condition: %+v %v", d, err)
	}
	mfa := *dev
	mfa.MFA = true
	d, _ = az.Evaluate(e.ctx, &mfa, domain.ActBranchPush, res, domain.RequestEnv{})
	if d.Allowed {
		t.Fatalf("applied deny not enforced: %+v", d)
	}
	res.Branch = "dev"
	if d, _ = az.Evaluate(e.ctx, &mfa, domain.ActBranchPush, res, domain.RequestEnv{}); !d.Allowed {
		t.Fatalf("deny leaked to another branch: %+v", d)
	}
}
