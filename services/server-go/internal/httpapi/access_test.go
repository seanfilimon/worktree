package httpapi

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// accessStack is the real IAM stack (memory store, iam, authz, accesscfg)
// behind the HTTP API: tenant acme with olga owner, adam admin, dev
// developer; root is a server admin; pat is a member of tenant partner.
type accessStack struct {
	t      *testing.T
	store  *meta.Memory
	h      http.Handler
	tokens map[string]string
	acme   *domain.Tenant
	web    *domain.Tree
}

func newAccessStack(t *testing.T) *accessStack {
	t.Helper()
	ctx := context.Background()
	st := meta.NewMemory()
	sec := iam.New(st, iam.Options{})
	az := authz.New(st, authz.Options{})
	s := &accessStack{t: t, store: st, tokens: map[string]string{}}
	s.acme = &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg}
	must(t, st.CreateTenant(ctx, s.acme))
	must(t, st.CreateTenant(ctx, &domain.Tenant{ID: "t-partner", Slug: "partner", Name: "Partner", Kind: domain.TenantOrg}))
	s.web = &domain.Tree{ID: "tree-web", TenantID: s.acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	must(t, st.CreateTree(ctx, s.web))
	for _, u := range []struct{ name, tenant, role string }{
		{"olga", "t-acme", domain.RoleOwner}, {"adam", "t-acme", domain.RoleAdmin}, {"dev", "t-acme", domain.RoleDeveloper},
		{"root", "", ""}, {"pat", "t-partner", domain.RoleViewer},
	} {
		acct := &domain.Account{ID: "acct-" + u.name, Username: u.name, Email: u.name + "@example.com", TenantID: "t-acme", IsAdmin: u.name == "root"}
		must(t, st.CreateAccount(ctx, acct))
		if u.role != "" {
			must(t, st.PutMembership(ctx, &domain.Membership{TenantID: u.tenant, AccountID: acct.ID, Role: u.role}))
		}
		tok, _, err := sec.CreateToken(ctx, acct.ID, "test", time.Hour)
		must(t, err)
		s.tokens[u.name] = tok
	}
	s.h = New(Deps{
		Store: st, Auth: sec, Authz: az, Access: az, Security: sec,
		AccessCfg: accesscfg.New(accesscfg.Deps{Store: st, Authz: az}),
	})
	return s
}

func (s *accessStack) do(method, path, as string, body any) *httptest.ResponseRecorder {
	s.t.Helper()
	var rd *bytes.Reader
	switch b := body.(type) {
	case nil:
		rd = bytes.NewReader(nil)
	case string:
		rd = bytes.NewReader([]byte(b))
	default:
		raw, err := json.Marshal(b)
		must(s.t, err)
		rd = bytes.NewReader(raw)
	}
	req := httptest.NewRequest(method, path, rd)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if as != "" {
		req.Header.Set("Authorization", "Bearer "+s.tokens[as])
	}
	rec := httptest.NewRecorder()
	s.h.ServeHTTP(rec, req)
	return rec
}

func (s *accessStack) expect(rec *httptest.ResponseRecorder, status int, code string) {
	s.t.Helper()
	if rec.Code != status || (code != "" && errCode(rec) != code) {
		s.t.Fatalf("status %d code %q, want %d %q: %s", rec.Code, errCode(rec), status, code, rec.Body.String())
	}
}

type applyResponse struct {
	Accepted bool     `json:"accepted"`
	Errors   []string `json:"errors"`
	Changes  []string `json:"changes"`
}

const accessDoc = `{"content": {
  "roles": [{"name": "release-manager", "description": "Cut releases", "permissions": ["tree:read", "release:*", "mr:merge"]}],
  "policies": [{"name": "protect-main", "description": "", "enabled": true, "statements": [
    {"sid": "no-direct-main", "effect": "deny", "actions": ["branch:push"], "resources": ["wt:branch/acme/web/main"],
     "principals": ["role:developer"], "conditions": {}}]}]}}`

func TestAccessConfigRoutes(t *testing.T) {
	s := newAccessStack(t)
	const base = "/api/v1/tenants/acme/access-config"

	s.expect(s.do("GET", base, "olga", nil), 404, "not_found")
	s.expect(s.do("POST", base+"/plan", "", accessDoc), 401, "unauthenticated")
	s.expect(s.do("POST", base+"/plan", "dev", accessDoc), 403, "forbidden")
	s.expect(s.do("POST", base+"/plan", "olga", `{"content": null}`), 400, "bad_request")

	rec := s.do("POST", base+"/plan", "adam", accessDoc)
	s.expect(rec, 200, "")
	plan := decode[applyResponse](t, rec)
	if !plan.Accepted || len(plan.Changes) != 2 || plan.Errors == nil {
		t.Fatalf("plan = %+v", plan)
	}
	s.expect(s.do("GET", base, "olga", nil), 404, "not_found") // plan stores nothing

	rec = s.do("POST", base+"/apply", "adam", accessDoc)
	s.expect(rec, 200, "")
	if res := decode[applyResponse](t, rec); !res.Accepted || res.Changes[0] != "role release-manager: create (3 permissions)" {
		t.Fatalf("apply = %+v", res)
	}
	rec = s.do("GET", base, "adam", nil)
	s.expect(rec, 200, "")
	doc := decode[domain.AccessConfigDoc](t, rec)
	if doc.Kind != "combined" || !bytes.Contains(doc.Content, []byte("release-manager")) {
		t.Fatalf("doc = %+v", doc)
	}

	// Invalid documents come back as accepted=false.
	rec = s.do("POST", base+"/apply", "olga", `{"content": {"policies": [{"name": "x", "enabled": true, "statements": [{"effect": "maybe", "actions": ["tree:read"], "resources": ["*"], "principals": ["*"]}]}]}}`)
	s.expect(rec, 200, "")
	if res := decode[applyResponse](t, rec); res.Accepted || !strings.Contains(strings.Join(res.Errors, "|"), "effect") {
		t.Fatalf("invalid apply = %+v", res)
	}
	// A JSON string holding the document is accepted too.
	rec = s.do("POST", base+"/plan", "olga", `{"content": "{\"roles\": [], \"policies\": []}"}`)
	if res := decode[applyResponse](t, rec); !res.Accepted || len(res.Changes) != 2 {
		t.Fatalf("string content plan = %+v", res)
	}

	// Tree documents: deny only.
	tree := "/api/v1/trees/acme/web/access-config"
	rec = s.do("POST", tree+"/apply", "adam", `{"content": {"policies": [{"name": "freeze", "enabled": true, "statements": [{"effect": "deny", "actions": ["tree:write"], "resources": ["wt:tree/acme/web"], "principals": ["*"]}]}]}}`)
	if res := decode[applyResponse](t, rec); !res.Accepted || res.Changes[0] != "policy freeze: create (1 statement)" {
		t.Fatalf("tree apply = %+v", res)
	}
	s.expect(s.do("GET", tree, "dev", nil), 200, "")
	s.expect(s.do("GET", "/api/v1/trees/acme/nope/access-config", "olga", nil), 404, "not_found")
}

func TestManagedByConfigIsReadOnly(t *testing.T) {
	s := newAccessStack(t)
	s.expect(s.do("POST", "/api/v1/tenants/acme/access-config/apply", "olga", accessDoc), 200, "")
	pols, err := s.store.ListPolicies(context.Background(), s.acme.ID)
	must(t, err)
	id := pols[0].ID
	pol := "/api/v1/tenants/acme/policies/" + id
	body := map[string]any{"name": "protect-main", "enabled": false, "statements": pols[0].Statements}

	s.expect(s.do("PUT", pol, "adam", body), 409, "managed_by_config")
	s.expect(s.do("DELETE", pol, "adam", nil), 409, "managed_by_config")
	s.expect(s.do("PATCH", "/api/v1/tenants/acme/roles/release-manager", "adam", map[string]string{"description": "x"}), 409, "managed_by_config")
	s.expect(s.do("DELETE", "/api/v1/tenants/acme/roles/release-manager", "olga", nil), 409, "managed_by_config")
	// force needs an owner (or server admin).
	s.expect(s.do("PUT", pol+"?force=true", "adam", body), 403, "forbidden")
	rec := s.do("PUT", pol+"?force=true", "olga", body)
	s.expect(rec, 200, "")
	if p := decode[domain.Policy](t, rec); p.Enabled || p.ManagedBy != domain.ManagedByConfig {
		t.Fatalf("forced update = %+v", p)
	}
	s.expect(s.do("PATCH", "/api/v1/tenants/acme/roles/release-manager?force=true", "root", map[string]string{"description": "x"}), 200, "")
	// A fetched policy can be sent back (managed_by / tree_id are accepted).
	rec = s.do("GET", pol, "olga", nil)
	s.expect(s.do("PUT", pol+"?force=true", "olga", rec.Body.String()), 200, "")
	// REST-managed policies are unaffected.
	rec = s.do("POST", "/api/v1/tenants/acme/policies", "adam", map[string]any{"name": "manual", "statements": []any{}})
	s.expect(rec, 201, "")
	s.expect(s.do("DELETE", "/api/v1/tenants/acme/policies/"+decode[domain.Policy](t, rec).ID, "adam", nil), 204, "")
}

func TestTenantStatusAdmin(t *testing.T) {
	s := newAccessStack(t)
	path := "/api/v1/admin/tenants/acme"
	s.expect(s.do("PATCH", path, "olga", map[string]string{"status": "suspended"}), 403, "forbidden")
	s.expect(s.do("PATCH", path, "root", map[string]string{"status": "frozen"}), 400, "bad_request")
	s.expect(s.do("PATCH", path, "root", map[string]string{}), 400, "bad_request")
	s.expect(s.do("PATCH", "/api/v1/admin/tenants/nope", "root", map[string]string{"status": "suspended"}), 404, "not_found")

	rec := s.do("PATCH", path, "root", map[string]string{"status": "suspended", "status_reason": "unpaid invoice"})
	s.expect(rec, 200, "")
	if tn := decode[domain.Tenant](t, rec); tn.Status != "suspended" || tn.StatusReason != "unpaid invoice" {
		t.Fatalf("tenant = %+v", tn)
	}
	// Reads work, writes are refused.
	s.expect(s.do("GET", "/api/v1/tenants/acme", "dev", nil), 200, "")
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "dev", nil), 200, "")
	rec = s.do("POST", "/api/v1/tenants/acme/teams", "olga", map[string]string{"slug": "core", "name": "Core"})
	s.expect(rec, 403, "forbidden")
	if !strings.Contains(rec.Body.String(), "suspended") {
		t.Fatalf("refusal does not explain the suspension: %s", rec.Body.String())
	}
	// Deactivated: everything is refused except for server admins.
	s.expect(s.do("PATCH", path, "root", map[string]string{"status": "deactivated"}), 200, "")
	s.expect(s.do("GET", "/api/v1/tenants/acme", "olga", nil), 403, "forbidden")
	s.expect(s.do("GET", "/api/v1/tenants/acme", "root", nil), 200, "")
	// Resume: nothing was revoked.
	rec = s.do("PATCH", path, "root", map[string]string{"status": "active"})
	s.expect(rec, 200, "")
	if tn := decode[domain.Tenant](t, rec); tn.Status != "" || tn.StatusReason != "" {
		t.Fatalf("resumed tenant = %+v", tn)
	}
	s.expect(s.do("POST", "/api/v1/tenants/acme/teams", "olga", map[string]string{"slug": "core", "name": "Core"}), 201, "")
}

func TestLastOwnerAndTenantGrants(t *testing.T) {
	s := newAccessStack(t)
	s.expect(s.do("PUT", "/api/v1/tenants/acme/members/olga", "olga", map[string]string{"role": "admin"}), 409, "last_owner")
	s.expect(s.do("DELETE", "/api/v1/tenants/acme/members/olga", "olga", nil), 409, "last_owner")

	// pat (partner) cannot read acme/web until partner gets a grant.
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "pat", nil), 403, "forbidden")
	rec := s.do("PUT", "/api/v1/trees/acme/web/grants", "adam", map[string]string{"subject_type": "tenant", "subject": "partner", "role": "viewer"})
	s.expect(rec, 200, "")
	if g := decode[domain.Grant](t, rec); g.SubjectType != "tenant" || g.Subject != "partner" || g.SubjectID != "t-partner" {
		t.Fatalf("grant = %+v", g)
	}
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "pat", nil), 200, "")
	s.expect(s.do("PUT", "/api/v1/trees/acme/web/grants", "adam", map[string]string{"subject_type": "tenant", "subject": "nowhere", "role": "viewer"}), 404, "not_found")
	rec = s.do("GET", "/api/v1/iam/effective?tenant=acme&tree=web&principal=user:pat", "adam", nil)
	s.expect(rec, 200, "")
	if eff := decode[effectiveResponse](t, rec); len(eff.Grants) != 1 || eff.Grants[0].SubjectType != "tenant" {
		t.Fatalf("effective = %+v", eff)
	}
	s.expect(s.do("DELETE", "/api/v1/trees/acme/web/grants?subject_type=tenant&subject=partner", "adam", nil), 204, "")
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "pat", nil), 403, "forbidden")
}

func TestSimulateCandidatePolicies(t *testing.T) {
	s := newAccessStack(t)
	sim := func(cands any) domain.Decision {
		t.Helper()
		body := map[string]any{"principal": map[string]string{"type": "user", "name": "dev"}, "action": "tree:write", "tenant": "acme", "tree": "web"}
		if cands != nil {
			body["candidate_policies"] = cands
		}
		rec := s.do("POST", "/api/v1/iam/simulate", "adam", body)
		s.expect(rec, 200, "")
		return decode[domain.Decision](t, rec)
	}
	if d := sim(nil); !d.Allowed {
		t.Fatalf("baseline = %+v", d)
	}
	denyAll := []map[string]any{{"name": "draft", "enabled": true, "statements": []map[string]any{
		{"effect": "deny", "actions": []string{"tree:write"}, "resources": []string{"wt:tree/acme/web"}, "principals": []string{"user:dev"}}}}}
	if d := sim(denyAll); d.Allowed || d.Effect != "deny" {
		t.Fatalf("candidate deny = %+v", d)
	}
	// Nothing was stored, and a stored deny is replaced by an empty set.
	pols, _ := s.store.ListPolicies(context.Background(), s.acme.ID)
	if len(pols) != 0 {
		t.Fatal("simulation stored policies")
	}
	s.expect(s.do("POST", "/api/v1/tenants/acme/policies", "adam", map[string]any{"name": "real", "statements": denyAll[0]["statements"]}), 201, "")
	if d := sim(nil); d.Allowed {
		t.Fatal("stored deny ignored")
	}
	if d := sim([]any{}); !d.Allowed {
		t.Fatalf("empty candidates still deny: %+v", d)
	}
	bad := []map[string]any{{"name": "bad", "enabled": true, "statements": []map[string]any{{"effect": "nope", "actions": []string{"x"}}}}}
	body := map[string]any{"action": "tree:write", "tenant": "acme", "candidate_policies": bad}
	s.expect(s.do("POST", "/api/v1/iam/simulate", "adam", body), 400, "bad_request")
}
