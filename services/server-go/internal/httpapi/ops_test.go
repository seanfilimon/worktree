package httpapi

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/gc"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
)

// Tests of the §13.3 operations endpoints and middleware.

var quietLog = slog.New(slog.DiscardHandler)

// nonDenialActions returns the recorded audit actions except access.denied
// (which every 403 records, §13.3).
func nonDenialActions(f *fixture) []string {
	var out []string
	for _, a := range f.audit.actions() {
		if a != audit.ActionAccessDenied {
			out = append(out, a)
		}
	}
	return out
}

// opsStore adds UpdateTenant to the fake store; DocStore and audit methods
// fall through to a real memory store.
type opsStore struct{ *fakeStore }

func (s opsStore) UpdateTenant(_ context.Context, t *domain.Tenant) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.tenants[t.ID]; !ok {
		return nf("tenant")
	}
	s.tenants[t.ID] = *t
	return nil
}

type opsFixture struct {
	*fixture
	ops     opsStore
	limiter *limits.Limiter
	gcStore domain.MetaStore
	gc      *gc.Collector
}

// newOpsFixture wires a Limiter (rate limit rpm) and a Collector over a
// separate memory store and disk object store.
func newOpsFixture(t *testing.T, rate int, mutate ...func(*Deps)) *opsFixture {
	t.Helper()
	of := &opsFixture{gcStore: meta.NewMemory()}
	t.Cleanup(func() { _ = of.gcStore.Close() })
	of.gc = gc.New(gc.Deps{Store: of.gcStore, Objects: objstore.New(t.TempDir()), Logger: quietLog,
		Interval: time.Hour, Grace: 2 * time.Hour, StagedRetention: 3 * time.Hour})
	wire := func(d *Deps) {
		fs := d.Store.(*fakeStore)
		fs.MetaStore = meta.NewMemory()
		of.ops = opsStore{fs}
		d.Store = of.ops
		of.limiter = limits.New(limits.Deps{Store: of.ops, Logger: quietLog, RateLimit: rate})
		d.Limits = of.limiter
		d.GC = of.gc
	}
	of.fixture = newFixture(t, append([]func(*Deps){wire}, mutate...)...)
	return of
}

func (f *opsFixture) setLimits(t *testing.T, l domain.TenantLimits) {
	t.Helper()
	tn := f.acme
	tn.Limits = &l
	must(t, f.ops.UpdateTenant(context.Background(), &tn))
}

func TestHealthzDegraded(t *testing.T) {
	f := newFixture(t)
	rec := f.do(t, "GET", "/healthz", "", nil)
	h := decode[healthResponse](t, rec)
	if rec.Code != 200 || h.Status != "ok" || h.Checks["meta"] != "ok" || h.Checks["objects"] != "ok" {
		t.Fatalf("healthy: %d %+v", rec.Code, h)
	}

	broken := newFixture(t, func(d *Deps) { d.Store = brokenStore{d.Store.(*fakeStore)} })
	rec = broken.do(t, "GET", "/healthz", "", nil)
	h = decode[healthResponse](t, rec)
	if rec.Code != http.StatusServiceUnavailable || h.Status != "degraded" || h.Checks["meta"] != "error" || h.Checks["objects"] != "ok" {
		t.Fatalf("degraded: %d %+v", rec.Code, h)
	}

	// A disk store whose root disappeared fails the objects probe.
	dir := t.TempDir() + "/gone"
	gone := newFixture(t, func(d *Deps) { d.Objects = objstore.New(dir) })
	rec = gone.do(t, "GET", "/healthz", "", nil)
	if h := decode[healthResponse](t, rec); rec.Code != 503 || h.Checks["objects"] != "error" {
		t.Fatalf("missing object root: %d %+v", rec.Code, h)
	}
}

type brokenStore struct{ *fakeStore }

func (brokenStore) GetTenant(context.Context, string) (*domain.Tenant, error) {
	return nil, errors.New("database is locked")
}

func TestRateLimiting(t *testing.T) {
	f := newOpsFixture(t, 2)
	before := opsmetrics.RateLimited.Get("")
	for i := 0; i < 2; i++ {
		rec := f.do(t, "GET", "/api/v1/tenants", "alice", nil)
		if rec.Code != 200 || rec.Header().Get("X-RateLimit-Limit") != "2" || rec.Header().Get("X-RateLimit-Remaining") != fmt.Sprint(1-i) {
			t.Fatalf("request %d: %d headers %v", i, rec.Code, rec.Header())
		}
	}
	rec := f.do(t, "GET", "/api/v1/tenants", "alice", nil)
	if rec.Code != http.StatusTooManyRequests || errCode(rec) != "rate_limited" || rec.Header().Get("Retry-After") == "" ||
		rec.Header().Get("X-RateLimit-Remaining") != "0" {
		t.Fatalf("over budget: %d %s %v", rec.Code, rec.Body.String(), rec.Header())
	}
	if opsmetrics.RateLimited.Get("") <= before {
		t.Fatal("worktree_rate_limited_total not incremented")
	}
	// Other principals and exempt endpoints are unaffected.
	if rec := f.do(t, "GET", "/api/v1/tenants", "bob", nil); rec.Code != 200 {
		t.Fatalf("bob: %d", rec.Code)
	}
	for i := 0; i < 5; i++ {
		if rec := f.do(t, "GET", "/healthz", "alice", nil); rec.Code != 200 {
			t.Fatalf("healthz rate limited: %d", rec.Code)
		}
		if rec := f.do(t, "GET", "/metrics", "alice", nil); rec.Code != 200 {
			t.Fatalf("metrics rate limited: %d", rec.Code)
		}
	}
	// A tenant override raises alice's personal-tenant budget.
	pt, err := f.store.GetTenant(context.Background(), "tenant-alice")
	must(t, err)
	pt.Limits = &domain.TenantLimits{RequestsPerMinute: 100}
	must(t, f.ops.UpdateTenant(context.Background(), pt))
	f.limiter.ForgetTenant(pt.ID)
	if rec := f.do(t, "GET", "/api/v1/tenants", "alice", nil); rec.Code != 200 || rec.Header().Get("X-RateLimit-Limit") != "100" {
		t.Fatalf("tenant override: %d %v", rec.Code, rec.Header())
	}
}

func TestGCEndpoints(t *testing.T) {
	f := newOpsFixture(t, 0)
	runsBefore := opsmetrics.GCRuns.Get("")
	cases := []struct {
		method, path, as string
		status           int
		code             string
	}{
		{"POST", "/api/v1/admin/gc", "", 401, "unauthenticated"},
		{"POST", "/api/v1/admin/gc", "alice", 403, "forbidden"},
		{"POST", "/api/v1/admin/gc?dry_run=maybe", "admin", 400, "bad_request"},
		{"GET", "/api/v1/admin/gc", "bob", 403, "forbidden"},
	}
	for _, c := range cases {
		if rec := f.do(t, c.method, c.path, c.as, nil); rec.Code != c.status || errCode(rec) != c.code {
			t.Fatalf("%s %s as %q: %d %s", c.method, c.path, c.as, rec.Code, rec.Body.String())
		}
	}
	rec := f.do(t, "POST", "/api/v1/admin/gc?dry_run=true", "admin", nil)
	if rep := decode[domain.GCReport](t, rec); rec.Code != 200 || !rep.DryRun || rep.StartedAt.IsZero() {
		t.Fatalf("dry run: %d %s", rec.Code, rec.Body.String())
	}
	rec = f.do(t, "POST", "/api/v1/admin/gc", "admin", nil)
	if rep := decode[domain.GCReport](t, rec); rec.Code != 200 || rep.DryRun {
		t.Fatalf("live run: %d %s", rec.Code, rec.Body.String())
	}
	if opsmetrics.GCRuns.Get("") < runsBefore+2 {
		t.Fatal("worktree_gc_runs_total not incremented")
	}

	rec = f.do(t, "GET", "/api/v1/admin/gc", "admin", nil)
	st := decode[gcStatusView](t, rec)
	if rec.Code != 200 || st.Config.Interval != "1h0m0s" || st.Config.IntervalSeconds != 3600 || st.Config.GraceSeconds != 7200 ||
		st.Config.StagedRetentionSeconds != 3*3600 || st.Config.AuditRetention != "0s" || st.Running || len(st.Runs) != 2 || st.Runs[0].DryRun {
		t.Fatalf("status: %d %s", rec.Code, rec.Body.String())
	}
	var raw map[string]any
	must(t, json.Unmarshal(rec.Body.Bytes(), &raw))
	if _, ok := raw["next_run_at"]; ok {
		t.Fatal("next_run_at reported although the ticker is not running")
	}

	// Without a collector the endpoints answer 501.
	plain := newFixture(t)
	if rec := plain.do(t, "POST", "/api/v1/admin/gc", "admin", nil); rec.Code != 501 || errCode(rec) != "not_implemented" {
		t.Fatalf("no gc: %d %s", rec.Code, rec.Body.String())
	}
}

func TestLimitsAndUsageEndpoints(t *testing.T) {
	f := newOpsFixture(t, 1200)
	const lp = "/api/v1/admin/tenants/acme/limits"
	if rec := f.do(t, "GET", lp, "alice", nil); rec.Code != 403 {
		t.Fatalf("owner reading admin limits: %d", rec.Code)
	}
	if rec := f.do(t, "GET", lp, "admin", nil); rec.Code != 200 || strings.TrimSpace(rec.Body.String()) != "{}" {
		t.Fatalf("empty limits: %d %s", rec.Code, rec.Body.String())
	}
	if rec := f.do(t, "PUT", lp, "admin", map[string]any{"max_trees": -1}); rec.Code != 400 {
		t.Fatalf("negative: %d", rec.Code)
	}
	if rec := f.do(t, "PUT", lp, "admin", map[string]any{"bogus": 1}); rec.Code != 400 {
		t.Fatalf("unknown field: %d", rec.Code)
	}
	rec := f.do(t, "PUT", lp, "admin", domain.TenantLimits{MaxTrees: 5, MaxStorageBytes: 1 << 20, RequestsPerMinute: 60})
	if l := decode[domain.TenantLimits](t, rec); rec.Code != 200 || l.MaxTrees != 5 {
		t.Fatalf("put: %d %s", rec.Code, rec.Body.String())
	}
	hasAudit(t, f.fixture, audit.ActionLimitsPut)
	if rec := f.do(t, "GET", lp, "admin", nil); decode[domain.TenantLimits](t, rec).MaxStorageBytes != 1<<20 {
		t.Fatalf("get after put: %s", rec.Body.String())
	}
	if rec := f.do(t, "GET", "/api/v1/admin/tenants/nope/limits", "admin", nil); rec.Code != 404 {
		t.Fatalf("unknown tenant: %d", rec.Code)
	}

	must(t, limits.SetStorageBytes(context.Background(), f.ops, f.acme.ID, 1234, time.Now()))
	rec = f.do(t, "GET", "/api/v1/tenants/acme/usage", "carol", nil)
	u := decode[limits.Usage](t, rec)
	if rec.Code != 200 || u.Trees != 2 || u.Branches != 2 || u.Members != 4 || u.StorageBytes != 1234 ||
		u.Limits.MaxTrees != 5 || u.Effective.RequestsPerMinute != 60 || u.Effective.MaxStorageBytes != 1<<20 {
		t.Fatalf("usage: %d %s", rec.Code, rec.Body.String())
	}
	var keys map[string]json.RawMessage
	must(t, json.Unmarshal(rec.Body.Bytes(), &keys))
	for _, k := range []string{"trees", "branches", "members", "storage_bytes", "limits", "effective"} {
		if _, ok := keys[k]; !ok {
			t.Fatalf("usage lacks %q: %s", k, rec.Body.String())
		}
	}
	if rec := f.do(t, "GET", "/api/v1/tenants/acme/usage", "dave", nil); rec.Code != 403 {
		t.Fatalf("non-member usage: %d", rec.Code)
	}

	// {} clears the limits.
	if rec := f.do(t, "PUT", lp, "admin", map[string]any{}); rec.Code != 200 {
		t.Fatalf("clear: %d", rec.Code)
	}
	if tn, _ := f.store.GetTenant(context.Background(), f.acme.ID); tn.Limits != nil {
		t.Fatalf("limits not cleared: %+v", tn.Limits)
	}
}

func TestQuotaEnforcementInCreateHandlers(t *testing.T) {
	f := newOpsFixture(t, 0)
	f.setLimits(t, domain.TenantLimits{MaxTrees: 2, MaxMembers: 4})
	rec := f.do(t, "POST", "/api/v1/tenants/acme/trees", "alice", map[string]string{"name": "third"})
	if rec.Code != 409 || errCode(rec) != "quota_exceeded" || !strings.Contains(rec.Body.String(), "max_trees") {
		t.Fatalf("tree quota: %d %s", rec.Code, rec.Body.String())
	}
	// acme has 4 members: adding dave exceeds, changing bob's role does not.
	rec = f.do(t, "PUT", "/api/v1/tenants/acme/members/dave", "alice", map[string]string{"role": "viewer"})
	if rec.Code != 409 || errCode(rec) != "quota_exceeded" {
		t.Fatalf("member quota: %d %s", rec.Code, rec.Body.String())
	}
	if rec := f.do(t, "PUT", "/api/v1/tenants/acme/members/bob", "alice", map[string]string{"role": "maintainer"}); rec.Code != 200 {
		t.Fatalf("role change under quota: %d %s", rec.Code, rec.Body.String())
	}
	f.setLimits(t, domain.TenantLimits{MaxTrees: 3, MaxMembers: 5})
	if rec := f.do(t, "POST", "/api/v1/tenants/acme/trees", "alice", map[string]string{"name": "third"}); rec.Code != 201 {
		t.Fatalf("tree under quota: %d %s", rec.Code, rec.Body.String())
	}
	if rec := f.do(t, "PUT", "/api/v1/tenants/acme/members/dave", "alice", map[string]string{"role": "viewer"}); rec.Code != 200 {
		t.Fatalf("member under quota: %d %s", rec.Code, rec.Body.String())
	}
}

// auditFixture uses a real audit log over a memory store.
func auditFixture(t *testing.T) (*fixture, domain.MetaStore) {
	t.Helper()
	mem := meta.NewMemory()
	t.Cleanup(func() { _ = mem.Close() })
	f := newFixture(t, func(d *Deps) { d.Audit = audit.New(mem, quietLog) })
	return f, mem
}

func TestAuditFiltersAndNDJSON(t *testing.T) {
	f, mem := auditFixture(t)
	ctx := context.Background()
	base := time.Date(2026, 5, 1, 0, 0, 0, 0, time.UTC)
	const n = 1205 // more than one page and more than maxAuditLimit
	for i := 0; i < n; i++ {
		e := &domain.AuditEvent{ID: fmt.Sprintf("ev-%04d", i), TenantID: f.acme.ID, Action: "tree.create",
			ResourceType: "tree", ResourceID: "tree-web", At: base.Add(time.Duration(i/3) * time.Second)}
		switch i % 4 {
		case 1:
			e.Action, e.ResourceType, e.ResourceID = "iam.role.put", "role", "dev"
		case 2:
			e.Action, e.ResourceType = "iam.grant.put", "grant"
		case 3:
			e.TenantID = "tenant-other"
		}
		must(t, mem.AppendAudit(ctx, e))
	}
	type result struct {
		path  string
		as    string
		count int
		check func(e domain.AuditEvent) bool
	}
	until := base.Add(100 * time.Second).Format(time.RFC3339)
	cases := []result{
		{"/api/v1/audit?tenant=acme&action=iam.*&limit=1000", "alice", 0, func(e domain.AuditEvent) bool { return strings.HasPrefix(e.Action, "iam.") }},
		{"/api/v1/audit?tenant=acme&resource_type=role&limit=1000", "alice", 0, func(e domain.AuditEvent) bool { return e.ResourceType == "role" }},
		{"/api/v1/audit?tenant=acme&resource_id=dev&limit=1000", "alice", 0, func(e domain.AuditEvent) bool { return e.ResourceID == "dev" }},
		{"/api/v1/audit?tenant=acme&until=" + until + "&limit=1000", "alice", 225, func(e domain.AuditEvent) bool { return e.At.Before(base.Add(100 * time.Second)) }},
	}
	for _, c := range cases {
		rec := f.do(t, "GET", c.path, c.as, nil)
		evs := decode[[]domain.AuditEvent](t, rec)
		if rec.Code != 200 || len(evs) == 0 {
			t.Fatalf("%s: %d %d events", c.path, rec.Code, len(evs))
		}
		if c.count > 0 && len(evs) != c.count {
			t.Fatalf("%s: %d events, want %d", c.path, len(evs), c.count)
		}
		for _, e := range evs {
			if !c.check(e) || e.TenantID != f.acme.ID {
				t.Fatalf("%s leaked %+v", c.path, e)
			}
		}
	}

	ndjson := func(path, as string) []domain.AuditEvent {
		t.Helper()
		rec := f.do(t, "GET", path, as, nil)
		if rec.Code != 200 || rec.Header().Get("Content-Type") != "application/x-ndjson" {
			t.Fatalf("%s: %d %q %s", path, rec.Code, rec.Header().Get("Content-Type"), rec.Body.String())
		}
		var out []domain.AuditEvent
		sc := bufio.NewScanner(rec.Body)
		for sc.Scan() {
			var e domain.AuditEvent
			must(t, json.Unmarshal(sc.Bytes(), &e))
			out = append(out, e)
		}
		return out
	}
	// Server admin export: every event, no limit, newest first.
	all := ndjson("/api/v1/audit?format=ndjson&limit=5", "admin")
	if len(all) != n {
		t.Fatalf("admin export = %d events, want %d", len(all), n)
	}
	for i := 1; i < len(all); i++ {
		if all[i].At.After(all[i-1].At) {
			t.Fatal("export not newest first")
		}
	}
	// Tenant export with a prefix filter.
	acmeIAM := ndjson("/api/v1/audit?tenant=acme&action=iam.*&format=ndjson", "alice")
	want := 0
	for i := 0; i < n; i++ {
		if i%4 == 1 || i%4 == 2 {
			want++
		}
	}
	if len(acmeIAM) != want {
		t.Fatalf("acme iam export = %d, want %d", len(acmeIAM), want)
	}
	// Unscoped export by a tenant admin covers only the tenants they administer.
	for _, e := range ndjson("/api/v1/audit?format=ndjson", "mallory") {
		if e.TenantID != f.acme.ID {
			t.Fatalf("scoped export leaked %+v", e)
		}
	}
	if evs := ndjson("/api/v1/audit?format=ndjson", "bob"); len(evs) != 0 {
		t.Fatalf("developer export = %d events", len(evs))
	}

	for _, bad := range []string{"format=csv", "until=yesterday", "action=.*"} {
		if rec := f.do(t, "GET", "/api/v1/audit?"+bad, "admin", nil); rec.Code != 400 {
			t.Fatalf("%s: %d", bad, rec.Code)
		}
	}
}

func TestAccessDeniedAudited(t *testing.T) {
	f := newFixture(t)
	if rec := f.do(t, "GET", "/api/v1/admin/stats", "alice", nil); rec.Code != 403 {
		t.Fatalf("stats as alice: %d", rec.Code)
	}
	e := auditDetail(t, f, audit.ActionAccessDenied)
	if e.Actor != "alice" || e.ResourceType != "route" || e.ResourceID != "GET /api/v1/admin/stats" ||
		e.Details["method"] != "GET" || e.Details["code"] != "forbidden" || e.TenantID != "" {
		t.Fatalf("access.denied = %+v", e)
	}

	f = newFixture(t)
	if rec := f.do(t, "PUT", "/api/v1/tenants/acme/members/dave", "bob", map[string]string{"role": "viewer"}); rec.Code != 403 {
		t.Fatalf("member put as developer: %d", rec.Code)
	}
	if e := auditDetail(t, f, audit.ActionAccessDenied); e.TenantID != f.acme.ID || e.Details["route"] != "PUT /api/v1/tenants/{tenant}/members/{username}" {
		t.Fatalf("tenant access.denied = %+v", e)
	}

	// 401/404 are not access denials.
	f = newFixture(t)
	f.do(t, "GET", "/api/v1/admin/stats", "", nil)
	f.do(t, "GET", "/api/v1/tenants/nope", "alice", nil)
	if got := f.audit.actions(); len(got) != 0 {
		t.Fatalf("unexpected audit %v", got)
	}
}

func TestDeniedAction(t *testing.T) {
	cases := map[string]string{
		"authz: tree:write: no role grants it: forbidden": "tree:write",
		"server admin required: forbidden":                "",
		`tenant "acme": not a member`:                     "",
		"path a/b:c denied":                               "",
	}
	for msg, want := range cases {
		if got := deniedAction(msg); got != want {
			t.Errorf("deniedAction(%q) = %q, want %q", msg, got, want)
		}
	}
}

func TestOpsMetricsExposed(t *testing.T) {
	opsmetrics.SyncPushes.Inc("accepted")
	opsmetrics.AuthzDecisions.Inc("deny")
	f := newFixture(t)
	rec := f.do(t, "GET", "/metrics", "", nil)
	body := rec.Body.String()
	for _, want := range []string{
		"# TYPE worktree_sync_pushes_total counter",
		`worktree_sync_pushes_total{result="accepted"} `,
		`worktree_sync_pushes_total{result="rejected"} `,
		`worktree_authz_decisions_total{decision="allow"} `,
		`worktree_authz_decisions_total{decision="deny"} `,
		"worktree_sync_pulls_total ",
		"worktree_mr_merges_total ",
		"worktree_gc_runs_total ",
		"worktree_gc_deleted_objects_total ",
		"worktree_rate_limited_total ",
	} {
		if !strings.Contains(body, want) {
			t.Fatalf("metrics lack %q:\n%s", want, body)
		}
	}
}

func TestBranchQuotaInCreateHandler(t *testing.T) {
	f := newOpsFixture(t, 0)
	const path = "/api/v1/trees/acme/web/branches"
	rec := f.do(t, "GET", path, "alice", nil)
	var have []map[string]any
	if err := json.Unmarshal(rec.Body.Bytes(), &have); err != nil {
		var wrapped struct {
			Branches []map[string]any `json:"branches"`
		}
		must(t, json.Unmarshal(rec.Body.Bytes(), &wrapped))
		have = wrapped.Branches
	}
	f.setLimits(t, domain.TenantLimits{MaxBranchesPerTree: len(have)})
	rec = f.do(t, "POST", path, "alice", map[string]string{"name": "one-too-many"})
	if rec.Code != 409 || errCode(rec) != "quota_exceeded" || !strings.Contains(rec.Body.String(), "max_branches_per_tree") {
		t.Fatalf("branch quota: %d %s", rec.Code, rec.Body.String())
	}
	f.setLimits(t, domain.TenantLimits{MaxBranchesPerTree: len(have) + 1})
	// Under the quota the request gets past the check (this fixture's fake
	// store may still refuse the write for other reasons).
	if rec := f.do(t, "POST", path, "alice", map[string]string{"name": "one-too-many"}); errCode(rec) == "quota_exceeded" {
		t.Fatalf("branch under quota refused: %s", rec.Body.String())
	}
}
