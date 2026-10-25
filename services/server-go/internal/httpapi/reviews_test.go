package httpapi

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"slices"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/ci"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// v0.5 review/CI endpoints (§13.4) over the real review and ci services
// backed by meta.NewMemory.

// tokenAuth resolves "Bearer <username>" to the fixture principal.
type tokenAuth struct {
	domain.Authenticator
	users map[string]*domain.Principal
}

func (a tokenAuth) Authenticate(_ context.Context, raw string) (*domain.Principal, error) {
	if p := a.users[raw]; p != nil {
		return p, nil
	}
	return nil, domain.ErrUnauthenticated
}

// roleAuthz authorizes by tenant role (owner/admin: everything) using the
// shared roleActions table plus ci:report for developers and up.
type roleAuthz struct{ store domain.MetaStore }

func (z roleAuthz) Authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, _ *domain.Tree) error {
	if p == nil {
		return fmt.Errorf("login required: %w", domain.ErrUnauthenticated)
	}
	role, _ := z.RoleOf(ctx, p.AccountID, tenantID)
	switch role {
	case domain.RoleOwner, domain.RoleAdmin:
		return nil
	case "":
		return fmt.Errorf("not a member: %w", domain.ErrForbidden)
	}
	if slices.Contains(roleActions[role], act) || (act == domain.ActCIReport && role != domain.RoleViewer) {
		return nil
	}
	return fmt.Errorf("%s may not %s: %w", role, act, domain.ErrForbidden)
}

func (z roleAuthz) RoleOf(ctx context.Context, accountID, tenantID string) (string, error) {
	m, err := z.store.GetMembership(ctx, tenantID, accountID)
	if err != nil {
		return "", nil
	}
	return m.Role, nil
}

type rvFixture struct {
	t     *testing.T
	store *meta.Memory
	h     http.Handler
	web   domain.Tree
	api   domain.Tree
}

func newRVFixture(t *testing.T, mutate ...func(*Deps)) *rvFixture {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	acme := domain.Tenant{ID: "tenant-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}
	must(t, store.CreateTenant(ctx, &acme))
	users := map[string]*domain.Principal{}
	roles := map[string]string{"alice": domain.RoleOwner, "bob": domain.RoleDeveloper, "carol": domain.RoleMaintainer, "vic": domain.RoleViewer, "dave": ""}
	for u, role := range roles {
		acct := domain.Account{ID: "acct-" + u, Username: u, Email: u + "@example.com", TenantID: "tenant-" + u, CreatedAt: t0}
		must(t, store.CreateAccount(ctx, &acct))
		if role != "" {
			must(t, store.PutMembership(ctx, &domain.Membership{TenantID: acme.ID, AccountID: acct.ID, Role: role, CreatedAt: t0}))
		}
		users[u] = &domain.Principal{AccountID: acct.ID, Username: u, TenantID: acct.TenantID}
	}
	f := &rvFixture{t: t, store: store}
	f.web = domain.Tree{ID: "tree-web", TenantID: acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	f.api = domain.Tree{ID: "tree-api", TenantID: acme.ID, Name: "api", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	for _, tr := range []*domain.Tree{&f.web, &f.api} {
		must(t, store.CreateTree(ctx, tr))
	}
	snap := func(id, tree string, at int, parents ...string) {
		must(t, store.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: tree, Parents: parents, Message: id, CreatedAt: t0.Add(time.Duration(at) * time.Minute)}))
	}
	snap("w1", f.web.ID, 1)
	snap("w2", f.web.ID, 2, "w1")
	snap("w3", f.web.ID, 3, "w2")
	snap("a1", f.api.ID, 1)
	snap("a2", f.api.ID, 2, "a1")
	branch := func(tree, name, tip string) {
		must(t, store.PutBranch(ctx, &domain.Branch{TreeID: tree, Name: name, Tip: tip}))
	}
	branch(f.web.ID, "main", "w1")
	branch(f.web.ID, "feature", "w2")
	branch(f.web.ID, "feature2", "w3")
	branch(f.api.ID, "main", "a1")
	branch(f.api.ID, "feature", "a2")
	for _, tr := range []domain.Tree{f.web, f.api} {
		must(t, store.CreateMergeRequest(ctx, &domain.MergeRequest{TreeID: tr.ID, Title: tr.Name + " feature", SourceBranch: "feature", TargetBranch: "main", AuthorID: "acct-bob", Author: "bob", State: domain.MRStateOpen, CreatedAt: t0}))
	}
	must(t, store.CreateMergeRequest(ctx, &domain.MergeRequest{TreeID: f.web.ID, Title: "web feature2", SourceBranch: "feature2", TargetBranch: "main", AuthorID: "acct-bob", Author: "bob", State: domain.MRStateOpen, CreatedAt: t0}))

	authz := roleAuthz{store: store}
	hist := meta.NewHistory(store)
	d := Deps{
		Store:   store,
		Objects: newFakeObjects(),
		History: hist,
		Auth:    tokenAuth{users: users},
		Authz:   authz,
		Review:  review.New(store, hist, authz, nil, nil),
		License: fakeLicense{},
		CI:      ci.New(ci.Deps{Store: store, Authz: authz}),
		Logger:  slog.New(slog.NewTextHandler(io.Discard, nil)),
	}
	for _, m := range mutate {
		m(&d)
	}
	f.h = New(d)
	return f
}

// call performs a request and returns the status and decoded JSON body.
func (f *rvFixture) call(method, path, as string, body any) (int, map[string]any, []any) {
	f.t.Helper()
	var rd io.Reader
	if body != nil {
		raw, err := json.Marshal(body)
		if err != nil {
			f.t.Fatal(err)
		}
		rd = bytes.NewReader(raw)
	}
	req := httptest.NewRequest(method, path, rd)
	if as != "" {
		req.Header.Set("Authorization", "Bearer "+as)
	}
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	var obj map[string]any
	var arr []any
	if b := bytes.TrimSpace(rec.Body.Bytes()); len(b) > 0 {
		if b[0] == '[' {
			_ = json.Unmarshal(b, &arr)
		} else {
			_ = json.Unmarshal(b, &obj)
		}
	}
	return rec.Code, obj, arr
}

// expect asserts the status (and error code when want is an error status).
func (f *rvFixture) expect(method, path, as string, body any, status int, code string) map[string]any {
	f.t.Helper()
	got, obj, _ := f.call(method, path, as, body)
	if got != status {
		f.t.Fatalf("%s %s as %q = %d %v, want %d", method, path, as, got, obj, status)
	}
	if code != "" {
		e, _ := obj["error"].(map[string]any)
		if e == nil || e["code"] != code {
			f.t.Fatalf("%s %s as %q: error = %v, want code %q", method, path, as, obj, code)
		}
	}
	return obj
}

const (
	webMR = "/api/v1/trees/acme/web/merge-requests/1"
	webT  = "/api/v1/trees/acme/web"
)

func TestReviewEndpoints(t *testing.T) {
	f := newRVFixture(t)
	f.expect("POST", webMR+"/review", "", map[string]any{"decision": "approved"}, 401, "unauthenticated")
	f.expect("POST", webMR+"/review", "carol", map[string]any{"decision": "lgtm"}, 400, "bad_request")
	f.expect("POST", webMR+"/review", "vic", map[string]any{"decision": "approved"}, 403, "forbidden")
	f.expect("POST", webMR+"/review", "bob", map[string]any{"decision": "approved"}, 403, "forbidden") // author
	f.expect("POST", webMR+"/review", "vic", map[string]any{"decision": "commented"}, 400, "bad_request")
	f.expect("POST", webMR+"/review", "dave", map[string]any{"decision": "commented", "body": "x"}, 403, "forbidden")
	f.expect("POST", "/api/v1/trees/acme/web/merge-requests/9/review", "carol", map[string]any{"decision": "approved"}, 404, "not_found")

	mr := f.expect("POST", webMR+"/review", "carol", map[string]any{"decision": "changes_requested", "body": "fix it"}, 200, "")
	apps, _ := mr["approvals"].([]any)
	if len(apps) != 1 || apps[0].(map[string]any)["decision"] != "changes_requested" || apps[0].(map[string]any)["snapshot_id"] != "w2" {
		t.Fatalf("approvals = %v", mr["approvals"])
	}
	f.expect("POST", webMR+"/review", "vic", map[string]any{"decision": "commented", "body": "+1"}, 200, "")

	checks := f.expect("GET", webMR+"/checks", "vic", nil, 200, "")
	if checks["mergeable"] != false {
		t.Fatalf("checks = %v", checks)
	}
	bl := checks["blockers"].([]any)
	if len(bl) != 1 || bl[0].(map[string]any)["kind"] != "changes_requested" {
		t.Fatalf("blockers = %v", bl)
	}
	for _, k := range []string{"approvals", "ci", "todos"} {
		if _, ok := checks[k].(map[string]any); !ok {
			t.Fatalf("checks lacks %s: %v", k, checks)
		}
	}
	f.expect("GET", webMR+"/checks", "dave", nil, 403, "")
	f.expect("POST", webMR+"/merge", "carol", nil, 409, "merge_blocked")

	f.expect("POST", webMR+"/approve", "carol", nil, 200, "")
	if c := f.expect("GET", webMR+"/checks", "vic", nil, 200, ""); c["mergeable"] != true {
		t.Fatalf("checks after approval = %v", c)
	}
	merged := f.expect("POST", webMR+"/merge", "carol", nil, 200, "")
	if merged["state"] != "merged" || merged["merged_snapshot"] != "w2" {
		t.Fatalf("merged = %v", merged)
	}
}

func TestCommentEndpoints(t *testing.T) {
	f := newRVFixture(t)
	f.expect("POST", webMR+"/comments", "vic", map[string]any{"body": ""}, 400, "bad_request")
	f.expect("POST", webMR+"/comments", "vic", map[string]any{"body": "x", "line": 2}, 400, "bad_request")
	f.expect("POST", webMR+"/comments", "vic", map[string]any{"body": "x", "bogus": 1}, 400, "bad_request")
	f.expect("POST", webMR+"/comments", "", map[string]any{"body": "x"}, 401, "")
	c := f.expect("POST", webMR+"/comments", "vic", map[string]any{"body": "why?", "path": "a.txt", "line": 3}, 201, "")
	if c["id"] != "1" || c["author"] != "vic" || c["snapshot_id"] != "w2" || c["line"] != float64(3) {
		t.Fatalf("comment = %v", c)
	}
	f.expect("POST", webMR+"/comments", "bob", map[string]any{"body": "because", "snapshot_id": "w1"}, 201, "")
	_, _, list := f.call("GET", webMR+"/comments", "vic", nil)
	if len(list) != 2 || list[1].(map[string]any)["body"] != "because" {
		t.Fatalf("list = %v", list)
	}
	f.expect("PATCH", webMR+"/comments/1", "bob", map[string]any{"resolved": true}, 403, "forbidden")
	f.expect("PATCH", webMR+"/comments/1", "vic", map[string]any{}, 400, "bad_request")
	up := f.expect("PATCH", webMR+"/comments/1", "vic", map[string]any{"resolved": true, "body": "why??"}, 200, "")
	if up["resolved"] != true || up["body"] != "why??" {
		t.Fatalf("updated = %v", up)
	}
	f.expect("PATCH", webMR+"/comments/77", "vic", map[string]any{"resolved": true}, 404, "not_found")
	f.expect("DELETE", webMR+"/comments/2", "carol", nil, 403, "forbidden") // maintainer is not tree admin
	f.expect("DELETE", webMR+"/comments/2", "alice", nil, 204, "")
	f.expect("DELETE", webMR+"/comments/2", "alice", nil, 404, "not_found")
	if _, _, list := f.call("GET", webMR+"/comments", "vic", nil); len(list) != 1 {
		t.Fatalf("after delete = %v", list)
	}
	f.expect("GET", "/api/v1/trees/acme/web/merge-requests/8/comments", "vic", nil, 404, "not_found")
}

func TestCIEndpointsAndRequiredChecks(t *testing.T) {
	f := newRVFixture(t)
	rule := f.expect("PUT", webT+"/protections", "alice", map[string]any{
		"pattern": "main", "require_ci_pass": true, "required_checks": []string{"build", " build", "test"}, "require_linear": true, "block_on_todos": true,
	}, 200, "")
	if fmt.Sprint(rule["required_checks"]) != "[build test]" || rule["require_linear"] != true || rule["block_on_todos"] != true {
		t.Fatalf("rule = %v", rule)
	}
	f.expect("PUT", webT+"/protections", "alice", map[string]any{"pattern": "main", "required_checks": []string{""}}, 400, "bad_request")

	report := map[string]any{"snapshot": "feature", "context": "build", "state": "success", "target_url": "https://ci.example/1"}
	f.expect("POST", webT+"/ci/status", "vic", report, 403, "forbidden")
	f.expect("POST", webT+"/ci/status", "", report, 401, "")
	f.expect("POST", webT+"/ci/status", "bob", map[string]any{"snapshot": "feature", "context": "build", "state": "ok"}, 400, "bad_request")
	f.expect("POST", webT+"/ci/status", "bob", map[string]any{"snapshot": "nope", "context": "build", "state": "success"}, 404, "not_found")
	f.expect("POST", webT+"/ci/status", "bob", map[string]any{"snapshot": "a1", "context": "build", "state": "success"}, 404, "not_found")
	st := f.expect("POST", webT+"/ci/status", "bob", report, 201, "")
	if st["snapshot_id"] != "w2" || st["reporter"] != "bob" || st["state"] != "success" {
		t.Fatalf("status = %v", st)
	}

	got := f.expect("GET", webT+"/ci/status?branch=feature", "vic", nil, 200, "")
	if got["snapshot"] != "w2" || got["state"] != "success" || len(got["checks"].([]any)) != 1 {
		t.Fatalf("status = %v", got)
	}
	if got := f.expect("GET", webT+"/ci/status?snapshot=w2", "vic", nil, 200, ""); got["state"] != "success" {
		t.Fatalf("by snapshot = %v", got)
	}
	if got := f.expect("GET", webT+"/ci/status", "vic", nil, 200, ""); got["snapshot"] != "w1" || got["state"] != "" {
		t.Fatalf("default branch = %v", got)
	}
	f.expect("GET", webT+"/ci/status?snapshot=w2&branch=main", "vic", nil, 400, "bad_request")
	f.expect("GET", webT+"/ci/status?branch=gone", "vic", nil, 404, "not_found")
	f.expect("GET", webT+"/ci/status", "dave", nil, 403, "")

	checks := f.expect("GET", webMR+"/checks", "vic", nil, 200, "")
	ciSum := checks["ci"].(map[string]any)
	if fmt.Sprint(ciSum["missing"]) != "[test]" || fmt.Sprint(ciSum["required"]) != "[build test]" {
		t.Fatalf("ci = %v", ciSum)
	}
	f.expect("POST", webMR+"/merge", "carol", nil, 409, "merge_blocked")
	f.expect("POST", webT+"/ci/status", "bob", map[string]any{"snapshot": "w2", "context": "test", "state": "success"}, 201, "")
	f.expect("POST", webMR+"/merge", "carol", nil, 200, "")
}

func TestLinkedGroupEndpoints(t *testing.T) {
	f := newRVFixture(t)
	const groups = "/api/v1/tenants/acme/linked-groups"
	body := map[string]any{"name": "release", "members": []map[string]any{
		{"tree": "acme/web", "number": 2}, {"tree": "acme/api", "number": 1},
	}}
	f.expect("POST", groups, "bob", body, 403, "forbidden")
	f.expect("POST", groups, "carol", map[string]any{"name": "x", "members": []map[string]any{{"tree": "acme/web", "number": 2}}}, 400, "bad_request")
	f.expect("POST", groups, "carol", map[string]any{"name": "x", "members": []map[string]any{{"tree": "web", "number": 2}, {"tree": "acme/api", "number": 1}}}, 400, "bad_request")
	f.expect("POST", groups, "carol", map[string]any{"name": "x", "members": []map[string]any{{"tree": "acme/nope", "number": 2}, {"tree": "acme/api", "number": 1}}}, 404, "not_found")
	f.expect("POST", groups, "dave", body, 403, "")

	g := f.expect("POST", groups, "carol", body, 201, "")
	id, _ := g["id"].(string)
	members := g["members"].([]any)
	if id == "" || g["state"] != "open" || len(members) != 2 || members[0].(map[string]any)["tree"] != "acme/web" || members[0].(map[string]any)["title"] != "web feature2" {
		t.Fatalf("group = %v", g)
	}
	f.expect("POST", groups, "carol", body, 409, "conflict")
	f.expect("POST", "/api/v1/trees/acme/web/merge-requests/2/merge", "carol", nil, 409, "linked_group")
	if mr := f.expect("GET", "/api/v1/trees/acme/web/merge-requests/2", "vic", nil, 200, ""); mr["linked_group"] != id {
		t.Fatalf("mr = %v", mr)
	}
	_, _, list := f.call("GET", groups, "vic", nil)
	if len(list) != 1 {
		t.Fatalf("list = %v", list)
	}
	f.expect("GET", groups+"/"+id, "vic", nil, 200, "")
	f.expect("GET", groups+"/missing", "vic", nil, 404, "not_found")
	f.expect("GET", groups, "dave", nil, 403, "")
	f.expect("POST", groups+"/"+id+"/merge", "bob", nil, 403, "forbidden")

	merged := f.expect("POST", groups+"/"+id+"/merge", "carol", nil, 200, "")
	if merged["state"] != "merged" || merged["merged_at"] == nil {
		t.Fatalf("merged = %v", merged)
	}
	ctx := context.Background()
	if b, _ := f.store.GetBranch(ctx, f.web.ID, "main"); b.Tip != "w3" {
		t.Fatalf("web main = %s", b.Tip)
	}
	if b, _ := f.store.GetBranch(ctx, f.api.ID, "main"); b.Tip != "a2" {
		t.Fatalf("api main = %s", b.Tip)
	}
	f.expect("POST", groups+"/"+id+"/merge", "carol", nil, 409, "conflict")
	f.expect("DELETE", groups+"/"+id, "carol", nil, 409, "conflict")
}

func TestLinkedGroupBlockedAndClosed(t *testing.T) {
	f := newRVFixture(t)
	const groups = "/api/v1/tenants/acme/linked-groups"
	f.expect("PUT", "/api/v1/trees/acme/api/protections", "alice", map[string]any{"pattern": "main", "required_approvals": 1}, 200, "")
	g := f.expect("POST", groups, "carol", map[string]any{"name": "g", "members": []map[string]any{
		{"tree": "acme/web", "number": 2}, {"tree": "acme/api", "number": 1},
	}}, 201, "")
	id := g["id"].(string)
	f.expect("POST", groups+"/"+id+"/merge", "carol", nil, 409, "merge_blocked")
	if b, _ := f.store.GetBranch(context.Background(), f.web.ID, "main"); b.Tip != "w1" {
		t.Fatalf("web main moved to %s", b.Tip)
	}
	f.expect("DELETE", groups+"/"+id, "bob", nil, 403, "")
	closed := f.expect("DELETE", groups+"/"+id, "carol", nil, 200, "")
	if closed["state"] != "closed" {
		t.Fatalf("closed = %v", closed)
	}
	f.expect("POST", "/api/v1/trees/acme/web/merge-requests/2/merge", "carol", nil, 200, "")
}

func TestTagProtectionEndpoints(t *testing.T) {
	f := newRVFixture(t)
	const tp = webT + "/tag-protections"
	rule := map[string]any{"pattern": "v*", "no_delete": true, "no_move": true, "allowed_taggers": []string{"alice"}}
	f.expect("PUT", tp, "carol", rule, 403, "forbidden")
	f.expect("PUT", tp, "alice", map[string]any{"pattern": ""}, 400, "bad_request")
	f.expect("PUT", tp, "alice", map[string]any{"pattern": "v*", "allowed_taggers": []string{" "}}, 400, "bad_request")
	got := f.expect("PUT", tp, "alice", rule, 200, "")
	if got["tree_id"] != f.web.ID || fmt.Sprint(got["allowed_taggers"]) != "[alice]" {
		t.Fatalf("rule = %v", got)
	}
	if _, _, list := f.call("GET", tp, "vic", nil); len(list) != 1 {
		t.Fatalf("list = %v", list)
	}

	tags := webT + "/tags"
	f.expect("POST", tags, "bob", map[string]any{"name": "v1", "target": "main"}, 403, "tag_protection")
	f.expect("POST", tags, "alice", map[string]any{"name": "v1", "target": "main"}, 201, "")
	f.expect("POST", tags, "alice", map[string]any{"name": "v1", "target": "feature"}, 409, "conflict")
	f.expect("POST", tags, "alice", map[string]any{"name": "v1", "target": "feature", "force": true}, 403, "tag_protection")
	f.expect("DELETE", tags+"/v1", "alice", nil, 403, "tag_protection")

	// Unprotected tags: force replaces (tree:admin), delete works.
	f.expect("POST", tags, "bob", map[string]any{"name": "x1", "target": "main"}, 201, "")
	f.expect("POST", tags, "bob", map[string]any{"name": "x1", "target": "feature", "force": true}, 403, "forbidden")
	moved := f.expect("POST", tags, "alice", map[string]any{"name": "x1", "target": "feature", "force": true}, 201, "")
	if moved["target"] != "w2" {
		t.Fatalf("moved = %v", moved)
	}

	f.expect("DELETE", tp, "alice", nil, 400, "bad_request")
	f.expect("DELETE", tp+"?pattern=v*", "carol", nil, 403, "")
	f.expect("DELETE", tp+"?pattern=v*", "alice", nil, 204, "")
	f.expect("DELETE", tp+"?pattern=v*", "alice", nil, 404, "not_found")
	f.expect("DELETE", tags+"/v1", "alice", nil, 204, "")
}

// plainReview implements only domain.ReviewService.
type plainReview struct{ domain.ReviewService }

func TestReviewEndpointsNotImplemented(t *testing.T) {
	f := newRVFixture(t, func(d *Deps) {
		d.Review = plainReview{d.Review}
		d.CI = nil
	})
	for _, c := range []struct{ method, path string }{
		{"POST", webMR + "/review"},
		{"GET", webMR + "/checks"},
		{"GET", webMR + "/comments"},
		{"GET", "/api/v1/tenants/acme/linked-groups"},
		{"GET", webT + "/tag-protections"},
		{"POST", webT + "/ci/status"},
		{"GET", webT + "/ci/status"},
	} {
		var body any
		if c.method == "POST" {
			body = map[string]any{"decision": "approved", "snapshot": "w1", "context": "x", "state": "success"}
		}
		f.expect(c.method, c.path, "alice", body, 501, "not_implemented")
	}
	// Tags still work without tag protections.
	f.expect("POST", webT+"/tags", "bob", map[string]any{"name": "v1", "target": "main"}, 201, "")
}
