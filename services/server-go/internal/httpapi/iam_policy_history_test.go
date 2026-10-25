package httpapi

import (
	"context"
	"net/http/httptest"
	"strconv"
	"strings"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/policyhist"
)

// ---- DocStore for the IAM fakes ----
//
// The policy handlers record versions in a DocStore collection. The shared
// fakes have none, so iamStore gets one here, backed by a meta.Memory per
// store (kept outside the struct so iam_fakes_test.go stays untouched).

var iamDocStores sync.Map // *iamStore -> domain.DocStore

func (s *iamStore) docs() domain.DocStore {
	if v, ok := iamDocStores.Load(s); ok {
		return v.(domain.DocStore)
	}
	v, _ := iamDocStores.LoadOrStore(s, meta.NewMemory())
	return v.(domain.DocStore)
}

func (s *iamStore) PutDoc(ctx context.Context, coll, key string, v any) error {
	return s.docs().PutDoc(ctx, coll, key, v)
}
func (s *iamStore) CreateDoc(ctx context.Context, coll, key string, v any) error {
	return s.docs().CreateDoc(ctx, coll, key, v)
}
func (s *iamStore) GetDoc(ctx context.Context, coll, key string, v any) error {
	return s.docs().GetDoc(ctx, coll, key, v)
}
func (s *iamStore) ListDocs(ctx context.Context, coll, prefix string) ([][]byte, error) {
	return s.docs().ListDocs(ctx, coll, prefix)
}
func (s *iamStore) DeleteDoc(ctx context.Context, coll, key string) error {
	return s.docs().DeleteDoc(ctx, coll, key)
}
func (s *iamStore) DeleteDocs(ctx context.Context, coll, prefix string) (int, error) {
	return s.docs().DeleteDocs(ctx, coll, prefix)
}
func (s *iamStore) UpdateDoc(ctx context.Context, coll, key string, fn func([]byte) ([]byte, error)) error {
	return s.docs().UpdateDoc(ctx, coll, key, fn)
}
func (s *iamStore) NextSeq(ctx context.Context, name string) (int64, error) {
	return s.docs().NextSeq(ctx, name)
}

// ---- real-stack tests ----

// newHistoryStack is newAccessStack with an event bus and an audit log.
func newHistoryStack(t *testing.T) (*accessStack, *fakeBus, *fakeAudit) {
	t.Helper()
	s := newAccessStack(t)
	sec := iam.New(s.store, iam.Options{})
	az := authz.New(s.store, authz.Options{})
	bus, aud := newFakeBus(), &fakeAudit{}
	s.h = New(Deps{
		Store: s.store, Auth: sec, Authz: az, Access: az, Security: sec, Bus: bus, Audit: aud,
		AccessCfg: accesscfg.New(accesscfg.Deps{Store: s.store, Authz: az, Bus: bus, Audit: aud}),
	})
	return s, bus, aud
}

const polBase = "/api/v1/tenants/acme/policies"

func stmt(sid, effect, action string) map[string]any {
	return map[string]any{"sid": sid, "effect": effect, "actions": []string{action}, "resources": []string{"wt:tree/acme/web"}, "principals": []string{"role:developer"}}
}

func versionsOf(t *testing.T, s *accessStack, id string) []domain.PolicyVersion {
	t.Helper()
	rec := s.do("GET", polBase+"/"+id+"/versions", "adam", nil)
	s.expect(rec, 200, "")
	return decode[[]domain.PolicyVersion](t, rec)
}

func changesOf(vs []domain.PolicyVersion) string {
	parts := make([]string, len(vs))
	for i, v := range vs {
		parts[i] = v.Change + "@" + strconv.Itoa(v.Version)
	}
	return strings.Join(parts, ",")
}

func drain(ch <-chan domain.Event) []domain.Event {
	var out []domain.Event
	for {
		select {
		case e := <-ch:
			out = append(out, e)
		default:
			return out
		}
	}
}

func TestPolicyHistoryRESTWritePaths(t *testing.T) {
	s, bus, aud := newHistoryStack(t)
	events, cancel := bus.Subscribe(domain.EventFilter{Types: []string{domain.EventPolicyChanged}})
	defer cancel()

	rec := s.do("POST", polBase, "adam", map[string]any{"name": "guard", "statements": []any{stmt("a", "deny", "tree:write")}})
	s.expect(rec, 201, "")
	id := decode[domain.Policy](t, rec).ID
	put := map[string]any{"name": "guard", "enabled": false, "statements": []any{stmt("a", "deny", "tree:write"), stmt("b", "deny", "branch:push")}}
	s.expect(s.do("PUT", polBase+"/"+id, "olga", put), 200, "")
	s.expect(s.do("DELETE", polBase+"/"+id, "adam", nil), 204, "")

	vs := versionsOf(t, s, id) // still readable after the delete
	if got := changesOf(vs); got != "create@1,update@2,delete@3" {
		t.Fatalf("versions = %s", got)
	}
	if vs[0].ChangedBy != "adam" || vs[1].ChangedBy != "olga" || vs[1].Policy.Version != 2 || len(vs[1].Policy.Statements) != 2 ||
		vs[2].Policy.Name != "guard" || vs[2].TenantID != s.acme.ID || vs[2].PolicyID != id {
		t.Fatalf("versions = %+v", vs)
	}
	if e := lastAudit(t, aud, "policy.update"); e.Details["history_version"] != "2" {
		t.Fatalf("audit = %+v", e)
	}

	got := drain(events)
	if len(got) != 3 {
		t.Fatalf("policy.changed events = %+v", got)
	}
	p := got[1].Payload.(map[string]any)
	if got[1].TenantID != s.acme.ID || got[1].Actor != "olga" || p["policy_id"] != id || p["version"] != 2 || p["change"] != "update" || p["name"] != "guard" {
		t.Fatalf("event = %+v", got[1])
	}

	// One version by number; unknown ones are 404, bad numbers 400.
	rec = s.do("GET", polBase+"/"+id+"/versions/2", "olga", nil)
	s.expect(rec, 200, "")
	if v := decode[domain.PolicyVersion](t, rec); v.Version != 2 || v.Policy.Enabled {
		t.Fatalf("version 2 = %+v", v)
	}
	s.expect(s.do("GET", polBase+"/"+id+"/versions/9", "olga", nil), 404, "not_found")
	s.expect(s.do("GET", polBase+"/"+id+"/versions/x", "olga", nil), 400, "bad_request")
	s.expect(s.do("GET", polBase+"/nope/versions", "olga", nil), 404, "not_found")

	// Deleted policies stay listable.
	rec = s.do("GET", polBase+"/deleted", "adam", nil)
	s.expect(rec, 200, "")
	if del := decode[[]domain.PolicyVersion](t, rec); len(del) != 1 || del[0].PolicyID != id || del[0].Change != "delete" || del[0].Policy.Name != "guard" {
		t.Fatalf("deleted = %+v", del)
	}
}

func lastAudit(t *testing.T, aud *fakeAudit, action string) domain.AuditEvent {
	t.Helper()
	aud.mu.Lock()
	defer aud.mu.Unlock()
	for i := len(aud.events) - 1; i >= 0; i-- {
		if aud.events[i].Action == action {
			return aud.events[i]
		}
	}
	t.Fatalf("audit %q not recorded", action)
	return domain.AuditEvent{}
}

func TestPolicyHistoryAccessApply(t *testing.T) {
	s, bus, _ := newHistoryStack(t)
	events, cancel := bus.Subscribe(domain.EventFilter{Types: []string{domain.EventPolicyChanged}})
	defer cancel()
	const apply = "/api/v1/tenants/acme/access-config/apply"
	ctx := context.Background()

	s.expect(s.do("POST", apply, "olga", accessDoc), 200, "")
	pols, err := s.store.ListPolicies(ctx, s.acme.ID)
	must(t, err)
	if len(pols) != 1 {
		t.Fatalf("policies = %+v", pols)
	}
	id := pols[0].ID
	// Re-applying the same document changes nothing and records nothing.
	s.expect(s.do("POST", apply, "olga", accessDoc), 200, "")
	changed := strings.Replace(accessDoc, `"description": ""`, `"description": "now documented"`, 1)
	s.expect(s.do("POST", apply, "olga", changed), 200, "")
	// A REST-managed policy is not touched by applies.
	s.expect(s.do("POST", polBase, "adam", map[string]any{"name": "manual", "statements": []any{}}), 201, "")
	s.expect(s.do("POST", apply, "olga", `{"content": {"roles": [{"name": "release-manager", "description": "Cut releases", "permissions": ["tree:read", "release:*", "mr:merge"]}], "policies": []}}`), 200, "")

	vs := versionsOf(t, s, id)
	if got := changesOf(vs); got != "create@1,update@2,delete@3" {
		t.Fatalf("versions = %s", got)
	}
	if vs[0].ChangedBy != "olga" || !strings.Contains(vs[0].Comment, "access document of tenant acme") || vs[1].Policy.Description != "now documented" {
		t.Fatalf("versions = %+v", vs)
	}
	// create + update + delete of protect-main, plus the REST create.
	if got := drain(events); len(got) != 4 {
		t.Fatalf("policy.changed events = %d: %+v", len(got), got)
	}

	// Forced REST edits of config-managed policies are recorded as well.
	s.expect(s.do("POST", apply, "olga", accessDoc), 200, "")
	pols, err = s.store.ListPolicies(ctx, s.acme.ID)
	must(t, err)
	var managed domain.Policy
	for _, p := range pols {
		if p.ManagedBy == domain.ManagedByConfig {
			managed = p
		}
	}
	body := map[string]any{"name": "protect-main", "enabled": false, "statements": managed.Statements}
	s.expect(s.do("PUT", polBase+"/"+managed.ID+"?force=true", "olga", body), 200, "")
	if got := changesOf(versionsOf(t, s, managed.ID)); got != "create@1,update@2" {
		t.Fatalf("versions = %s", got)
	}
}

func TestPolicyHistorySyntheticFirstVersion(t *testing.T) {
	s, _, _ := newHistoryStack(t)
	ctx := context.Background()
	old := &domain.Policy{ID: "pol-old", TenantID: s.acme.ID, Name: "legacy", Enabled: true, CreatedBy: "olga",
		Statements: []domain.PolicyStatement{{Sid: "x", Effect: "deny", Actions: []string{"tree:write"}, Resources: []string{"*"}, Principals: []string{"*"}}}}
	must(t, s.store.CreatePolicy(ctx, old))

	vs := versionsOf(t, s, "pol-old")
	if len(vs) != 1 || vs[0].Version != 1 || vs[0].Change != "create" || vs[0].Comment != policyhist.SyntheticComment ||
		vs[0].ChangedBy != "olga" || vs[0].Policy.Name != "legacy" {
		t.Fatalf("synthetic = %+v", vs)
	}
	// Stored, not recomputed: reading again returns the same single version.
	if again := versionsOf(t, s, "pol-old"); len(again) != 1 {
		t.Fatalf("versions = %+v", again)
	}

	// A policy without history that is changed first gets the synthetic
	// version from its state before the change.
	other := &domain.Policy{ID: "pol-other", TenantID: s.acme.ID, Name: "other", Enabled: true, CreatedBy: "adam", Statements: []domain.PolicyStatement{}}
	must(t, s.store.CreatePolicy(ctx, other))
	s.expect(s.do("PUT", polBase+"/pol-other", "adam", map[string]any{"name": "other", "enabled": false, "statements": []any{}}), 200, "")
	vs = versionsOf(t, s, "pol-other")
	if got := changesOf(vs); got != "create@1,update@2" || !vs[0].Policy.Enabled || vs[1].Policy.Enabled || vs[1].Policy.Version != 2 {
		t.Fatalf("versions = %s %+v", got, vs)
	}
}

type diffResponse struct {
	From    int                          `json:"from"`
	To      int                          `json:"to"`
	Changes []policyhist.StatementChange `json:"changes"`
	Fields  []policyhist.FieldChange     `json:"fields"`
}

func TestPolicyHistoryDiff(t *testing.T) {
	s, _, _ := newHistoryStack(t)
	anon := map[string]any{"effect": "deny", "actions": []string{"tree:admin"}, "resources": []string{"*"}, "principals": []string{"*"}}
	rec := s.do("POST", polBase, "adam", map[string]any{"name": "d", "statements": []any{stmt("keep", "deny", "tree:write"), stmt("gone", "deny", "branch:push"), anon}})
	s.expect(rec, 201, "")
	id := decode[domain.Policy](t, rec).ID
	anon2 := map[string]any{"effect": "deny", "actions": []string{"tree:admin", "iam:admin"}, "resources": []string{"*"}, "principals": []string{"*"}}
	s.expect(s.do("PUT", polBase+"/"+id, "adam", map[string]any{"name": "d", "enabled": false,
		"statements": []any{stmt("new", "deny", "mr:merge"), stmt("keep", "deny", "tree:write"), anon2}}), 200, "")

	diff := func(query string) diffResponse {
		t.Helper()
		rec := s.do("GET", polBase+"/"+id+"/diff"+query, "adam", nil)
		s.expect(rec, 200, "")
		return decode[diffResponse](t, rec)
	}
	d := diff("")
	if d.From != 1 || d.To != 2 || len(d.Changes) != 3 {
		t.Fatalf("diff = %+v", d)
	}
	add, change, remove := d.Changes[0], d.Changes[1], d.Changes[2]
	if add.Op != "add" || add.Sid != "new" || add.Index != 0 || add.Statement == nil ||
		change.Op != "change" || change.Sid != "" || change.Index != 2 || len(change.Before.Actions) != 1 || len(change.After.Actions) != 2 ||
		remove.Op != "remove" || remove.Sid != "gone" || remove.Index != 1 || remove.Statement.Actions[0] != "branch:push" {
		t.Fatalf("changes = %+v", d.Changes)
	}
	if len(d.Fields) != 1 || d.Fields[0].Field != "enabled" || d.Fields[0].Before != true || d.Fields[0].After != false {
		t.Fatalf("fields = %+v", d.Fields)
	}
	// Reverse direction and from the empty policy.
	if r := diff("?from=2&to=1"); len(r.Changes) != 3 || r.Changes[2].Op != "remove" || r.Changes[2].Sid != "new" {
		t.Fatalf("reverse = %+v", r)
	}
	if z := diff("?from=0&to=1"); len(z.Changes) != 3 || z.Changes[0].Op != "add" || len(z.Fields) != 3 {
		t.Fatalf("from empty = %+v", z)
	}
	if same := diff("?from=2&to=2"); len(same.Changes) != 0 || same.Changes == nil || same.Fields == nil {
		t.Fatalf("same = %s", s.do("GET", polBase+"/"+id+"/diff?from=2&to=2", "adam", nil).Body.String())
	}
	// A delete version stands for the empty policy.
	s.expect(s.do("DELETE", polBase+"/"+id, "adam", nil), 204, "")
	if del := diff(""); del.From != 2 || del.To != 3 || len(del.Changes) != 3 || del.Changes[0].Op != "remove" {
		t.Fatalf("delete diff = %+v", del)
	}
	s.expect(s.do("GET", polBase+"/"+id+"/diff?from=-1", "adam", nil), 400, "bad_request")
	s.expect(s.do("GET", polBase+"/"+id+"/diff?to=0", "adam", nil), 400, "bad_request")
	s.expect(s.do("GET", polBase+"/"+id+"/diff?to=7", "adam", nil), 404, "not_found")
}

type rollbackResponse = domain.PolicyVersion

func TestPolicyHistoryRollback(t *testing.T) {
	s, bus, aud := newHistoryStack(t)
	rec := s.do("POST", polBase, "adam", map[string]any{"name": "r", "statements": []any{stmt("a", "deny", "tree:write")}})
	s.expect(rec, 201, "")
	id := decode[domain.Policy](t, rec).ID
	s.expect(s.do("PUT", polBase+"/"+id, "adam", map[string]any{"name": "r2", "enabled": false, "statements": []any{}}), 200, "")
	events, cancel := bus.Subscribe(domain.EventFilter{Types: []string{domain.EventPolicyChanged}})
	defer cancel()

	rb := polBase + "/" + id + "/rollback"
	s.expect(s.do("POST", rb, "adam", map[string]any{"version": 0}), 400, "bad_request")
	s.expect(s.do("POST", rb, "adam", map[string]any{"version": 9}), 404, "not_found")
	s.expect(s.do("POST", rb, "adam", map[string]any{"version": 2}), 409, "conflict") // already the content

	rec = s.do("POST", rb, "adam", map[string]any{"version": 1, "comment": "undo the rename"})
	s.expect(rec, 200, "")
	pv := decode[rollbackResponse](t, rec)
	if pv.Version != 3 || pv.Change != "rollback" || pv.Comment != "rollback to version 1: undo the rename" || pv.ChangedBy != "adam" ||
		pv.Policy.Name != "r" || !pv.Policy.Enabled || len(pv.Policy.Statements) != 1 || pv.Policy.Version != 3 {
		t.Fatalf("rollback = %+v", pv)
	}
	rec = s.do("GET", polBase+"/"+id, "adam", nil)
	if p := decode[domain.Policy](t, rec); p.Name != "r" || p.Version != 3 || len(p.Statements) != 1 {
		t.Fatalf("policy after rollback = %+v", p)
	}
	if e := lastAudit(t, aud, "policy.rollback"); e.Details["to_version"] != "1" || e.Details["history_version"] != "3" || e.Details["restored"] != "false" {
		t.Fatalf("audit = %+v", e)
	}
	if got := drain(events); len(got) != 1 || got[0].Payload.(map[string]any)["change"] != "rollback" {
		t.Fatalf("events = %+v", got)
	}

	// Rolling a deleted policy back restores it under its id.
	s.expect(s.do("DELETE", polBase+"/"+id, "adam", nil), 204, "")
	s.expect(s.do("GET", polBase+"/"+id, "adam", nil), 404, "not_found")
	rec = s.do("POST", rb, "olga", map[string]any{"version": 2})
	s.expect(rec, 200, "")
	if pv := decode[rollbackResponse](t, rec); pv.Version != 5 || pv.Policy.Name != "r2" || pv.Policy.Version != 1 {
		t.Fatalf("restore = %+v", pv)
	}
	s.expect(s.do("GET", polBase+"/"+id, "adam", nil), 200, "")
	rec = s.do("GET", polBase+"/deleted", "adam", nil)
	if del := decode[[]domain.PolicyVersion](t, rec); len(del) != 0 {
		t.Fatalf("deleted after restore = %+v", del)
	}
	if got := changesOf(versionsOf(t, s, id)); got != "create@1,update@2,rollback@3,delete@4,rollback@5" {
		t.Fatalf("versions = %s", got)
	}
	// A restore whose name is taken in the meantime is a conflict.
	s.expect(s.do("DELETE", polBase+"/"+id, "adam", nil), 204, "")
	s.expect(s.do("POST", polBase, "adam", map[string]any{"name": "r2", "statements": []any{}}), 201, "")
	s.expect(s.do("POST", rb, "adam", map[string]any{"version": 5}), 409, "conflict")
}

func TestPolicyHistoryRollbackManagedByConfig(t *testing.T) {
	s, _, _ := newHistoryStack(t)
	const apply = "/api/v1/tenants/acme/access-config/apply"
	s.expect(s.do("POST", apply, "olga", accessDoc), 200, "")
	changed := strings.Replace(accessDoc, `"description": ""`, `"description": "v2"`, 1)
	s.expect(s.do("POST", apply, "olga", changed), 200, "")
	pols, err := s.store.ListPolicies(context.Background(), s.acme.ID)
	must(t, err)
	rb := polBase + "/" + pols[0].ID + "/rollback"

	s.expect(s.do("POST", rb, "adam", map[string]any{"version": 1}), 409, "managed_by_config")
	s.expect(s.do("POST", rb+"?force=true", "adam", map[string]any{"version": 1}), 403, "forbidden")
	rec := s.do("POST", rb+"?force=true", "olga", map[string]any{"version": 1})
	s.expect(rec, 200, "")
	if pv := decode[domain.PolicyVersion](t, rec); pv.Policy.Description != "" || pv.Policy.ManagedBy != domain.ManagedByConfig {
		t.Fatalf("forced rollback = %+v", pv)
	}
}

func TestPolicyHistoryPermissions(t *testing.T) {
	s, _, _ := newHistoryStack(t)
	rec := s.do("POST", polBase, "adam", map[string]any{"name": "p", "statements": []any{}})
	s.expect(rec, 201, "")
	id := decode[domain.Policy](t, rec).ID
	s.expect(s.do("PUT", polBase+"/"+id, "adam", map[string]any{"name": "p", "enabled": false, "statements": []any{}}), 200, "")
	for _, path := range []string{"/" + id + "/versions", "/" + id + "/versions/1", "/" + id + "/diff", "/deleted"} {
		s.expect(s.do("GET", polBase+path, "", nil), 401, "unauthenticated")
		s.expect(s.do("GET", polBase+path, "dev", nil), 403, "forbidden")
		s.expect(s.do("GET", polBase+path, "pat", nil), 403, "forbidden")
		s.expect(s.do("GET", polBase+path, "root", nil), 200, "")
	}
	rb := polBase + "/" + id + "/rollback"
	s.expect(s.do("POST", rb, "", map[string]any{"version": 1}), 401, "unauthenticated")
	s.expect(s.do("POST", rb, "dev", map[string]any{"version": 1}), 403, "forbidden")
	s.expect(s.do("POST", rb, "adam", `{"version": 1, "bogus": true}`), 400, "bad_request")
	s.expect(s.do("POST", rb, "adam", map[string]any{"version": 1}), 200, "")
}

// The IAM fixture's policy pol-1 predates history: its first read yields
// the synthetic version, and REST writes on the fake store are recorded.
func TestPolicyHistoryOnFakes(t *testing.T) {
	runIAMCases(t, []endpointCase{
		{name: "synthetic", method: "GET", path: polBase + "/pol-1/versions", as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				vs := decode[[]domain.PolicyVersion](t, rec)
				if len(vs) != 1 || vs[0].Comment != policyhist.SyntheticComment || vs[0].Policy.Version != 3 {
					t.Fatalf("versions = %+v", vs)
				}
			}},
		{name: "viewer", method: "GET", path: polBase + "/pol-1/versions", as: "carol", status: 403},
		{name: "maintainer cannot roll back", method: "POST", path: polBase + "/pol-1/rollback", as: "maria", body: map[string]any{"version": 1}, status: 403},
		{name: "delete records", method: "DELETE", path: polBase + "/pol-1", as: "alice", status: 204,
			check: func(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) {
				if e := auditDetail(t, f, "policy.delete"); e.Details["history_version"] != "2" {
					t.Fatalf("audit = %+v", e)
				}
			}},
	})
}
