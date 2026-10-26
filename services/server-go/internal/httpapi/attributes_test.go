package httpapi

// Tests of the ABAC attribute endpoints and of attribute conditions
// through the real stack (v0.8, docs/ENGINEERING_PLAN.md §16.5).

import (
	"context"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func attrsOf(t *testing.T, s *accessStack, method, path, as string, body any) map[string]string {
	t.Helper()
	rec := s.do(method, path, as, body)
	s.expect(rec, 200, "")
	return decode[attributesResponse](t, rec).Attributes
}

func TestTenantAttributesEndpoints(t *testing.T) {
	s, _, aud := newHistoryStack(t)
	const path = "/api/v1/tenants/acme/attributes"
	if got := attrsOf(t, s, "GET", path, "dev", nil); got == nil || len(got) != 0 {
		t.Fatalf("initial attributes = %#v, want {}", got)
	}
	s.expect(s.do("GET", path, "pat", nil), 403, "forbidden") // not a member of acme
	s.expect(s.do("GET", path, "", nil), 401, "unauthenticated")
	body := map[string]any{"attributes": map[string]string{"region": "eu-west", "cost.center": "r-and-d"}}
	s.expect(s.do("PUT", path, "dev", body), 403, "forbidden") // tenant:admin
	got := attrsOf(t, s, "PUT", path, "adam", body)
	if got["region"] != "eu-west" || got["cost.center"] != "r-and-d" || len(got) != 2 {
		t.Fatalf("PUT = %#v", got)
	}
	e := lastAudit(t, aud, "tenant.attributes.update")
	if e.Details["set"] != "cost.center,region" || e.Details["count"] != "2" || e.Details["tenant"] != "acme" || e.ResourceType != "tenant" {
		t.Fatalf("audit = %+v", e)
	}
	for _, v := range e.Details {
		if v == "eu-west" {
			t.Fatal("audit details must not contain attribute values")
		}
	}
	if got := attrsOf(t, s, "GET", path, "dev", nil); got["region"] != "eu-west" {
		t.Fatalf("GET = %#v", got)
	}
	tn, err := s.store.GetTenant(context.Background(), s.acme.ID)
	must(t, err)
	if tn.Attributes["region"] != "eu-west" {
		t.Fatalf("stored = %#v", tn.Attributes)
	}

	for _, bad := range []any{
		map[string]any{},
		map[string]any{"attributes": nil},
		map[string]any{"attributes": map[string]string{"Region": "x"}},
		map[string]any{"attributes": map[string]string{"region": ""}},
		map[string]any{"attributes": map[string]string{"region": strings.Repeat("x", 257)}},
		`{"attributes": {"region": 1}}`,
	} {
		s.expect(s.do("PUT", path, "adam", bad), 400, "bad_request")
	}

	// Unchanged sets are not audited; {} clears.
	before := len(aud.events)
	attrsOf(t, s, "PUT", path, "adam", body)
	if len(aud.events) != before {
		t.Fatal("an unchanged PUT was audited")
	}
	if got := attrsOf(t, s, "PUT", path, "olga", map[string]any{"attributes": map[string]string{}}); len(got) != 0 {
		t.Fatalf("clear = %#v", got)
	}
	if e := lastAudit(t, aud, "tenant.attributes.update"); e.Details["removed"] != "cost.center,region" || e.Details["count"] != "0" {
		t.Fatalf("audit = %+v", e)
	}
}

func TestAccountAttributesEndpoints(t *testing.T) {
	s, _, aud := newHistoryStack(t)
	const path = "/api/v1/admin/accounts/dev/attributes"
	body := map[string]any{"attributes": map[string]string{"department": "eng", "clearance": "secret"}}
	s.expect(s.do("PUT", path, "olga", body), 403, "forbidden") // tenant owner, not a server admin
	s.expect(s.do("GET", path, "dev", nil), 403, "forbidden")
	s.expect(s.do("GET", "/api/v1/admin/accounts/nobody/attributes", "root", nil), 404, "not_found")
	if got := attrsOf(t, s, "PUT", path, "root", body); got["department"] != "eng" {
		t.Fatalf("PUT = %#v", got)
	}
	if e := lastAudit(t, aud, "account.attributes.update"); e.Details["username"] != "dev" || e.Details["set"] != "clearance,department" {
		t.Fatalf("audit = %+v", e)
	}
	if got := attrsOf(t, s, "GET", path, "root", nil); got["clearance"] != "secret" {
		t.Fatalf("GET = %#v", got)
	}
	if got := attrsOf(t, s, "GET", "/api/v1/me/attributes", "dev", nil); got["department"] != "eng" || len(got) != 2 {
		t.Fatalf("me = %#v", got)
	}
	if got := attrsOf(t, s, "GET", "/api/v1/me/attributes", "adam", nil); len(got) != 0 {
		t.Fatalf("adam = %#v", got)
	}
	// Other members never see account attributes.
	rec := s.do("GET", "/api/v1/tenants/acme/members", "adam", nil)
	s.expect(rec, 200, "")
	if strings.Contains(rec.Body.String(), "clearance") || !strings.Contains(rec.Body.String(), `"username":"dev"`) {
		t.Fatalf("members = %s", rec.Body.String())
	}
	s.expect(s.do("GET", "/api/v1/me/attributes", "", nil), 401, "unauthenticated")
	s.expect(s.do("PUT", "/api/v1/me/attributes", "dev", body), 405, "")
}

func TestTreeAttributesEndpoints(t *testing.T) {
	s, _, aud := newHistoryStack(t)
	const path = "/api/v1/trees/acme/web/attributes"
	body := map[string]any{"attributes": map[string]string{"classification": "secret"}}
	if got := attrsOf(t, s, "GET", path, "dev", nil); len(got) != 0 {
		t.Fatalf("GET = %#v", got)
	}
	s.expect(s.do("PUT", path, "dev", body), 403, "forbidden") // developer: no tree:admin
	s.expect(s.do("GET", path, "pat", nil), 403, "forbidden")
	s.expect(s.do("GET", "/api/v1/trees/acme/nope/attributes", "adam", nil), 404, "not_found")
	if got := attrsOf(t, s, "PUT", path, "adam", body); got["classification"] != "secret" {
		t.Fatalf("PUT = %#v", got)
	}
	if e := lastAudit(t, aud, "tree.attributes.update"); e.Details["tree"] != "acme/web" || e.ResourceID != s.web.ID {
		t.Fatalf("audit = %+v", e)
	}
	// The attributes show on the tree too.
	rec := s.do("GET", "/api/v1/trees/acme/web", "dev", nil)
	s.expect(rec, 200, "")
	if !strings.Contains(rec.Body.String(), `"classification":"secret"`) {
		t.Fatalf("tree = %s", rec.Body.String())
	}
}

// clearancePolicy denies tree access to developers without clearance on
// trees classified secret.
var clearancePolicy = map[string]any{
	"name": "clearance",
	"statements": []any{map[string]any{
		"sid": "need-clearance", "effect": "deny", "actions": []string{"tree:*"},
		"resources": []string{"wt:tree/acme/*"}, "principals": []string{"role:developer"},
		"conditions": map[string]any{"attributes": []any{
			map[string]any{"key": "tree.classification", "op": "equals", "values": []string{"secret"}},
			map[string]any{"key": "principal.clearance", "op": "not_in", "values": []string{"secret", "top-secret"}},
		}},
	}},
}

func TestAttributeConditionsThroughREST(t *testing.T) {
	s, _, _ := newHistoryStack(t)
	rec := s.do("POST", polBase, "adam", clearancePolicy)
	s.expect(rec, 201, "")
	pol := decode[domain.Policy](t, rec)
	if conds := pol.Statements[0].Conditions.Attributes; len(conds) != 2 || conds[1].Op != "not_in" {
		t.Fatalf("stored conditions = %+v", pol.Statements[0].Conditions)
	}
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "dev", nil), 200, "") // tree not classified

	s.expect(s.do("PUT", "/api/v1/trees/acme/web/attributes", "adam",
		map[string]any{"attributes": map[string]string{"classification": "secret"}}), 200, "")
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "dev", nil), 403, "forbidden")
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "adam", nil), 200, "") // admins are not developers

	// The next request sees the new account attribute.
	s.expect(s.do("PUT", "/api/v1/admin/accounts/dev/attributes", "root",
		map[string]any{"attributes": map[string]string{"clearance": "secret"}}), 200, "")
	s.expect(s.do("GET", "/api/v1/trees/acme/web", "dev", nil), 200, "")

	// Simulator: the explanation, and overrides (null unsets).
	sim := map[string]any{"principal": map[string]any{"type": "user", "name": "dev"}, "action": "tree:read", "tenant": "acme", "tree": "web"}
	rec = s.do("POST", "/api/v1/iam/simulate", "adam", sim)
	s.expect(rec, 200, "")
	out := decode[simulateResponse](t, rec)
	if !out.Allowed || len(out.Conditions) != 2 || out.Conditions[0].Key != "tree.classification" || !out.Conditions[0].Matched ||
		out.Conditions[1].Matched || out.Conditions[1].Value == nil || *out.Conditions[1].Value != "secret" ||
		out.Conditions[0].Policy != "clearance" || out.Conditions[0].PolicyID != pol.ID || out.Conditions[0].Statement != "need-clearance" {
		t.Fatalf("simulate = %+v", out)
	}
	if out.Attributes.Principal["clearance"] != "secret" || out.Attributes.Tree["classification"] != "secret" || out.Attributes.Tenant == nil {
		t.Fatalf("attributes = %+v", out.Attributes)
	}
	attrReasons := 0
	for _, r := range out.Reasons {
		if r.Source == authz.SourceAttribute {
			attrReasons++
		}
	}
	if attrReasons != 2 {
		t.Fatalf("reasons = %+v", out.Reasons)
	}
	sim["context"] = map[string]any{"attributes": map[string]any{"principal.clearance": nil}}
	rec = s.do("POST", "/api/v1/iam/simulate", "adam", sim)
	s.expect(rec, 200, "")
	if out := decode[simulateResponse](t, rec); out.Allowed || out.Effect != "deny" || len(out.Attributes.Principal) != 0 {
		t.Fatalf("override simulate = %+v", out)
	}
	sim["context"] = map[string]any{"attributes": map[string]any{"clearance": "x"}}
	s.expect(s.do("POST", "/api/v1/iam/simulate", "adam", sim), 400, "bad_request")
	// Without attribute policies the response still carries empty lists.
	delete(sim, "context")
	sim["tenant"], sim["tree"], sim["principal"] = "partner", "", map[string]any{"type": "user", "name": "pat"}
	rec = s.do("POST", "/api/v1/iam/simulate", "pat", sim)
	s.expect(rec, 200, "")
	if !strings.Contains(rec.Body.String(), `"conditions":[]`) || !strings.Contains(rec.Body.String(), `"principal":{}`) {
		t.Fatalf("simulate = %s", rec.Body.String())
	}
}

func TestAttributeConditionValidationOnPolicyWrites(t *testing.T) {
	s, _, _ := newHistoryStack(t)
	bad := map[string]any{"name": "bad", "statements": []any{map[string]any{
		"effect": "deny", "actions": []string{"tree:read"}, "resources": []string{"*"}, "principals": []string{"*"},
		"conditions": map[string]any{"attributes": []any{
			map[string]any{"key": "department", "op": "equals", "values": []string{"eng"}},
			map[string]any{"key": "principal.dept", "op": "exists", "values": []string{"x"}},
		}},
	}}}
	rec := s.do("POST", polBase, "adam", bad)
	s.expect(rec, 400, "bad_request")
	if !strings.Contains(rec.Body.String(), "conditions.attributes[0].key") {
		t.Fatalf("error = %s", rec.Body.String())
	}
	rec = s.do("POST", polBase+"/validate", "adam", map[string]any{"statements": bad["statements"]})
	s.expect(rec, 200, "")
	res := decode[validatePolicyResponse](t, rec)
	if res.Valid || len(res.Errors) != 2 || res.Errors[1].Field != "conditions.attributes[1].values" {
		t.Fatalf("validate = %+v", res)
	}
}

func TestAttributeChanges(t *testing.T) {
	if d := attributeChanges(map[string]string{"a": "1"}, map[string]string{"a": "1"}); d != nil {
		t.Fatalf("unchanged = %v", d)
	}
	d := attributeChanges(map[string]string{"a": "1", "b": "2"}, map[string]string{"a": "2", "c": "3"})
	if d["set"] != "a,c" || d["removed"] != "b" || d["count"] != "2" {
		t.Fatalf("changes = %v", d)
	}
	if d := attributeChanges(nil, nil); d != nil {
		t.Fatalf("nil = %v", d)
	}
}
