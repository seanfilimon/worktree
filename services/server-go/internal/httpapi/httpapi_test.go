package httpapi

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// fixture is a fully wired API over in-memory fakes, seeded with:
//
//	tenant acme (org): alice owner, mallory admin, bob developer, carol viewer
//	accounts: admin (server admin), dave (no acme membership), each with a
//	personal tenant
//	trees: acme/web (private, branch main: s1 <- s2, tag v1, one staged
//	entry, protection "main", license rule, MR !1) and acme/docs (public,
//	no branches)
type fixture struct {
	store *fakeStore
	auth  *fakeAuth
	audit *fakeAudit
	bus   *fakeBus
	objs  *fakeObjects
	done  chan struct{}

	deps    Deps
	h       http.Handler
	users   map[string]*domain.Principal
	tokens  map[string]*domain.Principal // raw API token -> principal
	acme    domain.Tenant
	web     domain.Tree
	docs    domain.Tree
	s1, s2  domain.Snapshot
	orphan  domain.Snapshot // snapshot of web whose manifest object is missing
	foreign domain.Snapshot // snapshot of another tree
}

var t0 = time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)

func manifest(t testing.TB, objs *fakeObjects, files map[string]string) hash.Hash {
	t.Helper()
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	slices.Sort(paths)
	var m syncpb.ManifestRecord
	for _, p := range paths {
		h := objs.put([]byte(files[p]))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{
			Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(files[p])), Executable: strings.HasSuffix(p, ".sh"),
		})
	}
	raw, err := proto.Marshal(&m)
	if err != nil {
		t.Fatal(err)
	}
	return objs.put(raw)
}

func newFixture(t testing.TB, mutate ...func(*Deps)) *fixture {
	t.Helper()
	store := newFakeStore()
	f := &fixture{
		store:  store,
		auth:   &fakeAuth{store: store, passwords: map[string]string{}, regOpen: true},
		audit:  &fakeAudit{},
		bus:    newFakeBus(),
		objs:   newFakeObjects(),
		done:   make(chan struct{}),
		users:  map[string]*domain.Principal{},
		tokens: map[string]*domain.Principal{},
	}
	ctx := context.Background()

	f.acme = domain.Tenant{ID: "tenant-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, Plan: "free", CreatedAt: t0}
	must(t, store.CreateTenant(ctx, &f.acme))
	roles := map[string]string{"alice": domain.RoleOwner, "mallory": domain.RoleAdmin, "bob": domain.RoleDeveloper, "carol": domain.RoleViewer, "dave": "", "admin": ""}
	for _, u := range []string{"admin", "alice", "bob", "carol", "dave", "mallory"} {
		personal := domain.Tenant{ID: "tenant-" + u, Slug: u, Name: u, Kind: domain.TenantUser, CreatedAt: t0}
		must(t, store.CreateTenant(ctx, &personal))
		acct := domain.Account{ID: "acct-" + u, Username: u, Email: u + "@example.com", TenantID: personal.ID, IsAdmin: u == "admin", CreatedAt: t0}
		must(t, store.CreateAccount(ctx, &acct))
		must(t, store.PutMembership(ctx, &domain.Membership{TenantID: personal.ID, AccountID: acct.ID, Role: domain.RoleOwner, CreatedAt: t0}))
		if r := roles[u]; r != "" {
			must(t, store.PutMembership(ctx, &domain.Membership{TenantID: f.acme.ID, AccountID: acct.ID, Role: r, CreatedAt: t0}))
		}
		f.auth.passwords[u] = u + "-password"
		f.users[u] = &domain.Principal{AccountID: acct.ID, Username: u, TenantID: personal.ID, IsAdmin: acct.IsAdmin}
	}

	f.web = domain.Tree{ID: "tree-web", TenantID: f.acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0, UpdatedAt: t0}
	f.docs = domain.Tree{ID: "tree-docs", TenantID: f.acme.ID, Name: "docs", DefaultBranch: "main", Visibility: domain.VisibilityPublic, CreatedAt: t0, UpdatedAt: t0}
	must(t, store.CreateTree(ctx, &f.web))
	must(t, store.CreateTree(ctx, &f.docs))

	m1 := manifest(t, f.objs, map[string]string{"README.md": "# web\n"})
	m2 := manifest(t, f.objs, map[string]string{
		"README.md":      "# web v2\n",
		"src/main.go":    "package main\n\nfunc main() {}\n",
		"secret/key.txt": "hunter2",
		"index.html":     "<html><script>alert(1)</script></html>",
		"LICENSE":        "Permission is hereby granted, free of charge...",
		"run.sh":         "#!/bin/sh\necho hi\n",
	})
	f.s1 = domain.Snapshot{ID: "snap-1", TreeID: f.web.ID, BranchName: "main", ManifestHash: m1, Author: "alice", Message: "first", CreatedAt: t0}
	f.s2 = domain.Snapshot{ID: "snap-2", TreeID: f.web.ID, BranchName: "main", Parents: []string{"snap-1"}, ManifestHash: m2, Author: "alice", Message: "second", CreatedAt: t0.Add(time.Hour)}
	f.orphan = domain.Snapshot{ID: "snap-orphan", TreeID: f.web.ID, ManifestHash: hash.Sum([]byte("gone")), CreatedAt: t0}
	f.foreign = domain.Snapshot{ID: "snap-foreign", TreeID: "tree-other", ManifestHash: m1, CreatedAt: t0}
	for _, s := range []domain.Snapshot{f.s1, f.s2, f.orphan, f.foreign} {
		store.snapshots[s.ID] = s
	}
	must(t, store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "main", Tip: f.s2.ID, CreatedAt: t0, UpdatedAt: t0}))
	must(t, store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "feature", Tip: f.s2.ID, CreatedAt: t0, UpdatedAt: t0}))
	store.tags[[2]string{f.web.ID, "v1"}] = domain.Tag{TreeID: f.web.ID, Name: "v1", Target: f.s1.ID, CreatedAt: t0}
	store.staged = append(store.staged, domain.StagedSnapshot{SnapshotID: "wip-1", TreeID: f.web.ID, TenantID: f.acme.ID, AccountID: "acct-bob", Author: "bob", BranchName: "main", StagedAt: t0})
	must(t, store.PutProtection(ctx, &domain.BranchProtection{TreeID: f.web.ID, Pattern: "main", NoDirectPush: true, RequiredApprovals: 1}))
	must(t, store.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: f.web.ID, PathGlob: "secret/**", SPDX: "LicenseRef-Proprietary", Grant: "read-only"}))
	must(t, store.CreateMergeRequest(ctx, &domain.MergeRequest{TreeID: f.web.ID, Title: "Feature", SourceBranch: "feature", TargetBranch: "main", AuthorID: "acct-bob", Author: "bob", State: domain.MRStateOpen, CreatedAt: t0}))

	authz := &fakeAuthz{store: store}
	f.deps = Deps{
		Store:     store,
		Objects:   f.objs,
		History:   &fakeHistory{store: store},
		Auth:      f.auth,
		Authz:     authz,
		Review:    &fakeReview{store: store, authz: authz},
		License:   fakeLicense{},
		Staged:    &fakeStaged{store: store},
		Bus:       f.bus,
		Audit:     f.audit,
		Version:   "test-1.0",
		StartedAt: time.Now().Add(-90 * time.Second),
		Logger:    slog.New(slog.NewTextHandler(io.Discard, nil)),
		Done:      f.done,
	}
	for _, m := range mutate {
		m(&f.deps)
	}
	f.auth.resolve = f.principalFor
	f.h = New(f.deps)
	return f
}

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// principalFor backs the fake Authenticator used by the real iam middleware:
// "Bearer <username>" or "Bearer <raw token>" resolves to the matching principal.
func (f *fixture) principalFor(raw string) *domain.Principal {
	if p := f.tokens[raw]; p != nil {
		return p
	}
	return f.users[raw]
}

// do performs a request; body may be nil, a string (sent verbatim) or a
// value encoded as JSON.
func (f *fixture) do(t testing.TB, method, target, as string, body any) *httptest.ResponseRecorder {
	t.Helper()
	var rd io.Reader
	switch b := body.(type) {
	case nil:
	case string:
		rd = strings.NewReader(b)
	default:
		buf, err := json.Marshal(b)
		if err != nil {
			t.Fatal(err)
		}
		rd = bytes.NewReader(buf)
	}
	req := httptest.NewRequest(method, target, rd)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if as != "" {
		req.Header.Set("Authorization", "Bearer "+as)
	}
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	return rec
}

func decode[T any](t testing.TB, rec *httptest.ResponseRecorder) T {
	t.Helper()
	var v T
	if err := json.Unmarshal(rec.Body.Bytes(), &v); err != nil {
		t.Fatalf("decode %T: %v; body: %s", v, err, rec.Body.String())
	}
	return v
}

func errCode(rec *httptest.ResponseRecorder) string {
	var e errorBody
	if json.Unmarshal(rec.Body.Bytes(), &e) != nil {
		return ""
	}
	return e.Error.Code
}

type endpointCase struct {
	name   string
	setup  func(t *testing.T, f *fixture)
	method string
	path   string
	as     string
	body   any
	status int
	code   string // expected error code ("" = not checked / success)
	check  func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder)
}

func runCases(t *testing.T, cases []endpointCase) {
	t.Helper()
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			f := newFixture(t)
			if tc.setup != nil {
				tc.setup(t, f)
			}
			rec := f.do(t, tc.method, tc.path, tc.as, tc.body)
			if rec.Code != tc.status {
				t.Fatalf("%s %s: status = %d, want %d; body: %s", tc.method, tc.path, rec.Code, tc.status, rec.Body.String())
			}
			if tc.code != "" {
				if got := errCode(rec); got != tc.code {
					t.Fatalf("error code = %q, want %q; body: %s", got, tc.code, rec.Body.String())
				}
			}
			if rec.Code >= 400 && strings.HasPrefix(tc.path, "/api/") {
				if ct := rec.Header().Get("Content-Type"); !strings.HasPrefix(ct, "application/json") {
					t.Fatalf("error Content-Type = %q, want JSON", ct)
				}
				if errCode(rec) == "" {
					t.Fatalf("error response without envelope: %s", rec.Body.String())
				}
			}
			if tc.check != nil {
				tc.check(t, f, rec)
			}
		})
	}
}

func hasAudit(t *testing.T, f *fixture, action string) {
	t.Helper()
	if !slices.Contains(f.audit.actions(), action) {
		t.Fatalf("audit %q not recorded; have %v", action, f.audit.actions())
	}
}

func TestHealthz(t *testing.T) {
	runCases(t, []endpointCase{{
		name: "ok", method: "GET", path: "/healthz", status: 200,
		check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			h := decode[map[string]any](t, rec)
			if h["status"] != "ok" || h["version"] != "test-1.0" {
				t.Fatalf("health = %v", h)
			}
			if up, _ := h["uptime_seconds"].(float64); up < 89 {
				t.Fatalf("uptime_seconds = %v, want >= 89", h["uptime_seconds"])
			}
		},
	}})
}

func TestAuthEndpoints(t *testing.T) {
	big := `{"username":"erin","email":"e@x.io","password":"` + strings.Repeat("a", maxJSONBody) + `"}`
	runCases(t, []endpointCase{
		{name: "register", method: "POST", path: "/api/v1/auth/register",
			body: map[string]string{"username": "erin", "email": "erin@example.com", "password": "correct-horse"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				s := decode[map[string]json.RawMessage](t, rec)
				var acct domain.Account
				must(t, json.Unmarshal(s["account"], &acct))
				if acct.Username != "erin" || len(s["token"]) < 5 {
					t.Fatalf("session = %s", rec.Body.String())
				}
				if strings.Contains(rec.Body.String(), "password") {
					t.Fatalf("response leaks password: %s", rec.Body.String())
				}
				hasAudit(t, f, "auth.register")
			}},
		{name: "register bad username", method: "POST", path: "/api/v1/auth/register",
			body: map[string]string{"username": "Bad Name", "email": "x@y.z", "password": "correct-horse"}, status: 400, code: "bad_request"},
		{name: "register short password", method: "POST", path: "/api/v1/auth/register",
			body: map[string]string{"username": "erin", "email": "x@y.z", "password": "short"}, status: 400, code: "bad_request"},
		{name: "register bad email", method: "POST", path: "/api/v1/auth/register",
			body: map[string]string{"username": "erin", "email": "nope", "password": "correct-horse"}, status: 400, code: "bad_request"},
		{name: "register unknown field", method: "POST", path: "/api/v1/auth/register",
			body: `{"username":"erin","email":"x@y.z","password":"correct-horse","admin":true}`, status: 400, code: "bad_request"},
		{name: "register malformed", method: "POST", path: "/api/v1/auth/register", body: `{"username":`, status: 400, code: "bad_request"},
		{name: "register trailing data", method: "POST", path: "/api/v1/auth/register",
			body: `{"username":"erin","email":"x@y.z","password":"correct-horse"} {}`, status: 400, code: "bad_request"},
		{name: "register empty body", method: "POST", path: "/api/v1/auth/register", body: "", status: 400, code: "bad_request"},
		{name: "register too large", method: "POST", path: "/api/v1/auth/register", body: big, status: 413, code: "payload_too_large"},
		{name: "register duplicate", method: "POST", path: "/api/v1/auth/register",
			body: map[string]string{"username": "alice", "email": "a@b.c", "password": "correct-horse"}, status: 409, code: "conflict"},
		{name: "register disabled", setup: func(t *testing.T, f *fixture) { f.auth.regOpen = false },
			method: "POST", path: "/api/v1/auth/register",
			body: map[string]string{"username": "erin", "email": "a@b.c", "password": "correct-horse"}, status: 403, code: "forbidden"},
		{name: "login", method: "POST", path: "/api/v1/auth/login",
			body: map[string]string{"username": "alice", "password": "alice-password"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				s := decode[sessionResponse](t, rec)
				if s.Token == "" || s.Account == nil || s.Account.Username != "alice" {
					t.Fatalf("session = %s", rec.Body.String())
				}
				hasAudit(t, f, "auth.login")
			}},
		{name: "login wrong password", method: "POST", path: "/api/v1/auth/login",
			body: map[string]string{"username": "alice", "password": "nope"}, status: 401, code: "unauthenticated",
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if rec.Header().Get("WWW-Authenticate") == "" {
					t.Fatal("missing WWW-Authenticate")
				}
				hasAudit(t, f, "auth.login_failed")
			}},
		{name: "login missing fields", method: "POST", path: "/api/v1/auth/login", body: map[string]string{"username": "alice"}, status: 400, code: "bad_request"},
		{name: "logout anonymous", method: "POST", path: "/api/v1/auth/logout", status: 401, code: "unauthenticated"},
		{name: "me", method: "GET", path: "/api/v1/auth/me", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				me := decode[meResponse](t, rec)
				if me.Account.Username != "alice" || len(me.Memberships) != 2 {
					t.Fatalf("me = %s", rec.Body.String())
				}
				if me.Memberships[0].Tenant.Slug != "acme" || me.Memberships[0].Role != "owner" {
					t.Fatalf("memberships = %+v", me.Memberships)
				}
			}},
		{name: "me anonymous", method: "GET", path: "/api/v1/auth/me", status: 401, code: "unauthenticated"},
		{name: "create token", method: "POST", path: "/api/v1/auth/tokens", as: "bob",
			body: map[string]any{"name": "ci", "ttl_hours": 24}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[createTokenResponse](t, rec)
				if r.Token == "" || r.APIToken == nil || r.APIToken.Name != "ci" || r.APIToken.ExpiresAt == nil {
					t.Fatalf("token = %s", rec.Body.String())
				}
				if strings.Contains(rec.Body.String(), "token_hash") {
					t.Fatal("token hash leaked")
				}
				hasAudit(t, f, "token.create")
			}},
		{name: "create token bad ttl", method: "POST", path: "/api/v1/auth/tokens", as: "bob",
			body: map[string]any{"name": "ci", "ttl_hours": -1}, status: 400, code: "bad_request"},
		{name: "create token no name", method: "POST", path: "/api/v1/auth/tokens", as: "bob",
			body: map[string]any{"ttl_hours": 1}, status: 400, code: "bad_request"},
		{name: "create token anonymous", method: "POST", path: "/api/v1/auth/tokens",
			body: map[string]any{"name": "x"}, status: 401, code: "unauthenticated"},
		{name: "list tokens", method: "GET", path: "/api/v1/auth/tokens", as: "carol", status: 200,
			setup: func(t *testing.T, f *fixture) {
				_, _, err := f.auth.CreateToken(context.Background(), "acct-carol", "laptop", 0)
				must(t, err)
				_, _, err = f.auth.CreateToken(context.Background(), "acct-bob", "other", 0)
				must(t, err)
			},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				toks := decode[[]domain.APIToken](t, rec)
				if len(toks) != 1 || toks[0].Name != "laptop" {
					t.Fatalf("tokens = %s", rec.Body.String())
				}
			}},
		{name: "list tokens excludes sessions", method: "GET", path: "/api/v1/auth/tokens", as: "carol", status: 200,
			setup: func(t *testing.T, f *fixture) {
				ctx := context.Background()
				must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "tok-sess", AccountID: "acct-carol", Name: "session", Kind: domain.TokenSession, TokenHash: "h-sess"}))
				must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "tok-legacy", AccountID: "acct-carol", Name: "session", TokenHash: "h-legacy"}))
				must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "tok-api", AccountID: "acct-carol", Name: "ci", Kind: domain.TokenAPI, TokenHash: "h-api"}))
			},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				toks := decode[[]domain.APIToken](t, rec)
				if len(toks) != 1 || toks[0].ID != "tok-api" {
					t.Fatalf("tokens = %s", rec.Body.String())
				}
			}},
		{name: "list tokens empty is array", method: "GET", path: "/api/v1/auth/tokens", as: "dave", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if strings.TrimSpace(rec.Body.String()) != "[]" {
					t.Fatalf("body = %s", rec.Body.String())
				}
			}},
		{name: "revoke foreign token", method: "DELETE", path: "/api/v1/auth/tokens/tok-x", as: "bob", status: 404, code: "not_found",
			setup: func(t *testing.T, f *fixture) {
				f.store.tokens["tok-x"] = domain.APIToken{ID: "tok-x", AccountID: "acct-carol"}
			}},
	})
}

func TestLoginWrongContentType(t *testing.T) {
	f := newFixture(t)
	req := httptest.NewRequest("POST", "/api/v1/auth/login", strings.NewReader("username=alice"))
	req.Header.Set("Content-Type", "application/x-www-form-urlencoded")
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	if rec.Code != http.StatusUnsupportedMediaType || errCode(rec) != "unsupported_media_type" {
		t.Fatalf("status = %d body %s", rec.Code, rec.Body.String())
	}
}

func TestLogoutAndRevoke(t *testing.T) {
	f := newFixture(t)
	ctx := context.Background()
	raw, tok, err := f.auth.CreateToken(ctx, "acct-bob", "session", 0)
	must(t, err)
	f.tokens[raw] = f.users["bob"]

	if rec := f.do(t, "POST", "/api/v1/auth/logout", raw, nil); rec.Code != http.StatusNoContent {
		t.Fatalf("logout = %d %s", rec.Code, rec.Body.String())
	}
	if _, ok := f.store.tokens[tok.ID]; ok {
		t.Fatal("token not revoked")
	}
	hasAudit(t, f, "auth.logout")
	// Logging out again with the (now unknown) token is idempotent.
	if rec := f.do(t, "POST", "/api/v1/auth/logout", raw, nil); rec.Code != http.StatusNoContent {
		t.Fatalf("second logout = %d", rec.Code)
	}

	_, tok2, err := f.auth.CreateToken(ctx, "acct-bob", "ci", 0)
	must(t, err)
	if rec := f.do(t, "DELETE", "/api/v1/auth/tokens/"+tok2.ID, "bob", nil); rec.Code != http.StatusNoContent {
		t.Fatalf("revoke = %d %s", rec.Code, rec.Body.String())
	}
	if _, ok := f.store.tokens[tok2.ID]; ok {
		t.Fatal("token not revoked")
	}
	hasAudit(t, f, "token.revoke")
}

func TestTenantEndpoints(t *testing.T) {
	slugs := func(want ...string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			var got []string
			for _, tn := range decode[[]domain.Tenant](t, rec) {
				got = append(got, tn.Slug)
			}
			if !slices.Equal(got, want) {
				t.Fatalf("tenants = %v, want %v", got, want)
			}
		}
	}
	runCases(t, []endpointCase{
		{name: "list as member", method: "GET", path: "/api/v1/tenants", as: "alice", status: 200, check: slugs("acme", "alice")},
		{name: "list as admin", method: "GET", path: "/api/v1/tenants", as: "admin", status: 200,
			check: slugs("acme", "admin", "alice", "bob", "carol", "dave", "mallory")},
		{name: "list anonymous", method: "GET", path: "/api/v1/tenants", status: 401, code: "unauthenticated"},
		{name: "create", method: "POST", path: "/api/v1/tenants", as: "dave", body: map[string]string{"slug": "globex", "name": "Globex"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				tn := decode[domain.Tenant](t, rec)
				if tn.Slug != "globex" || tn.Kind != domain.TenantOrg || tn.ID == "" {
					t.Fatalf("tenant = %+v", tn)
				}
				m, err := f.store.GetMembership(context.Background(), tn.ID, "acct-dave")
				if err != nil || m.Role != domain.RoleOwner {
					t.Fatalf("owner membership = %+v, %v", m, err)
				}
				hasAudit(t, f, "tenant.create")
			}},
		{name: "create duplicate", method: "POST", path: "/api/v1/tenants", as: "dave", body: map[string]string{"slug": "acme", "name": "x"}, status: 409, code: "conflict"},
		{name: "create bad slug", method: "POST", path: "/api/v1/tenants", as: "dave", body: map[string]string{"slug": "-Bad-", "name": "x"}, status: 400, code: "bad_request"},
		{name: "create anonymous", method: "POST", path: "/api/v1/tenants", body: map[string]string{"slug": "x", "name": "x"}, status: 401, code: "unauthenticated"},
		{name: "get as viewer", method: "GET", path: "/api/v1/tenants/acme", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if decode[domain.Tenant](t, rec).ID != f.acme.ID {
					t.Fatal("wrong tenant")
				}
			}},
		{name: "get by id", method: "GET", path: "/api/v1/tenants/tenant-acme", as: "carol", status: 200},
		{name: "get as outsider", method: "GET", path: "/api/v1/tenants/acme", as: "dave", status: 403, code: "forbidden"},
		{name: "get anonymous", method: "GET", path: "/api/v1/tenants/acme", status: 401, code: "unauthenticated"},
		{name: "get missing", method: "GET", path: "/api/v1/tenants/nope", as: "alice", status: 404, code: "not_found"},
		{name: "members", method: "GET", path: "/api/v1/tenants/acme/members", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				ms := decode[[]memberView](t, rec)
				var got []string
				for _, m := range ms {
					got = append(got, m.Account.Username+":"+m.Role)
				}
				want := []string{"alice:owner", "bob:developer", "carol:viewer", "mallory:admin"}
				if !slices.Equal(got, want) {
					t.Fatalf("members = %v, want %v", got, want)
				}
			}},
		{name: "put member", method: "PUT", path: "/api/v1/tenants/acme/members/dave", as: "mallory", body: map[string]string{"role": "developer"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				m := decode[memberView](t, rec)
				if m.Account.Username != "dave" || m.Role != "developer" {
					t.Fatalf("member = %+v", m)
				}
				hasAudit(t, f, "member.update")
			}},
		{name: "put member as developer", method: "PUT", path: "/api/v1/tenants/acme/members/dave", as: "bob", body: map[string]string{"role": "viewer"}, status: 403, code: "forbidden"},
		{name: "put member bad role", method: "PUT", path: "/api/v1/tenants/acme/members/dave", as: "alice", body: map[string]string{"role": "god"}, status: 400, code: "bad_request"},
		{name: "put member unknown account", method: "PUT", path: "/api/v1/tenants/acme/members/zed", as: "alice", body: map[string]string{"role": "viewer"}, status: 404, code: "not_found"},
		{name: "grant owner as non-owner admin", method: "PUT", path: "/api/v1/tenants/acme/members/bob", as: "mallory", body: map[string]string{"role": "owner"}, status: 403, code: "forbidden"},
		{name: "grant owner as owner", method: "PUT", path: "/api/v1/tenants/acme/members/bob", as: "alice", body: map[string]string{"role": "owner"}, status: 200},
		{name: "demote last owner", method: "PUT", path: "/api/v1/tenants/acme/members/alice", as: "alice", body: map[string]string{"role": "admin"}, status: 409, code: "conflict"},
		{name: "delete member", method: "DELETE", path: "/api/v1/tenants/acme/members/bob", as: "alice", status: 204,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if _, err := f.store.GetMembership(context.Background(), f.acme.ID, "acct-bob"); !errors.Is(err, domain.ErrNotFound) {
					t.Fatal("membership not deleted")
				}
				hasAudit(t, f, "member.remove")
			}},
		{name: "leave tenant", method: "DELETE", path: "/api/v1/tenants/acme/members/carol", as: "carol", status: 204},
		{name: "delete other as developer", method: "DELETE", path: "/api/v1/tenants/acme/members/carol", as: "bob", status: 403, code: "forbidden"},
		{name: "last owner leaves", method: "DELETE", path: "/api/v1/tenants/acme/members/alice", as: "alice", status: 409, code: "conflict"},
		{name: "delete non-member", method: "DELETE", path: "/api/v1/tenants/acme/members/dave", as: "alice", status: 404, code: "not_found"},
	})
}

func TestTreeEndpoints(t *testing.T) {
	names := func(want ...string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			var got []string
			for _, s := range decode[[]treeSummary](t, rec) {
				got = append(got, s.Tree.Name)
			}
			if !slices.Equal(got, want) {
				t.Fatalf("trees = %v, want %v", got, want)
			}
		}
	}
	runCases(t, []endpointCase{
		{name: "list anonymous sees public", method: "GET", path: "/api/v1/tenants/acme/trees", status: 200, check: names("docs")},
		{name: "list member", method: "GET", path: "/api/v1/tenants/acme/trees", as: "carol", status: 200, check: names("docs", "web")},
		{name: "list missing tenant", method: "GET", path: "/api/v1/tenants/nope/trees", status: 404, code: "not_found"},
		{name: "list store failure is 500 without details", method: "GET", path: "/api/v1/tenants/acme/trees", as: "carol", status: 500, code: "internal",
			setup: func(t *testing.T, f *fixture) { f.store.failList = errors.New("disk on fire") },
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if strings.Contains(rec.Body.String(), "disk on fire") {
					t.Fatal("internal error leaked to client")
				}
			}},
		{name: "create", method: "POST", path: "/api/v1/tenants/acme/trees", as: "bob",
			body: map[string]string{"name": "api", "description": "the API"}, status: 201,
			setup: func(t *testing.T, f *fixture) {
				ch, _ := f.bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeCreated}})
				t.Cleanup(func() {
					select {
					case e := <-ch:
						if e.Type != domain.EventTreeCreated || e.TenantID != f.acme.ID || e.Actor != "bob" {
							t.Errorf("event = %+v", e)
						}
					default:
						t.Error("tree.created not published")
					}
				})
			},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				tr := decode[domain.Tree](t, rec)
				if tr.Name != "api" || tr.Visibility != domain.VisibilityPrivate || tr.DefaultBranch != "main" || tr.TenantID != f.acme.ID {
					t.Fatalf("tree = %+v", tr)
				}
				if b, err := f.store.GetBranch(context.Background(), tr.ID, "main"); err != nil || b.Tip != "" {
					t.Fatalf("default branch = %+v, %v", b, err)
				}
				hasAudit(t, f, "tree.create")
			}},
		{name: "create as viewer", method: "POST", path: "/api/v1/tenants/acme/trees", as: "carol", body: map[string]string{"name": "x"}, status: 403, code: "forbidden"},
		{name: "create anonymous", method: "POST", path: "/api/v1/tenants/acme/trees", body: map[string]string{"name": "x"}, status: 401, code: "unauthenticated"},
		{name: "create bad name", method: "POST", path: "/api/v1/tenants/acme/trees", as: "bob", body: map[string]string{"name": "../etc"}, status: 400, code: "bad_request"},
		{name: "create bad visibility", method: "POST", path: "/api/v1/tenants/acme/trees", as: "bob", body: map[string]string{"name": "x", "visibility": "secret"}, status: 400, code: "bad_request"},
		{name: "create bad branch", method: "POST", path: "/api/v1/tenants/acme/trees", as: "bob", body: map[string]string{"name": "x", "default_branch": "a..b"}, status: 400, code: "bad_request"},
		{name: "create duplicate", method: "POST", path: "/api/v1/tenants/acme/trees", as: "bob", body: map[string]string{"name": "web"}, status: 409, code: "conflict"},
		{name: "get summary", method: "GET", path: "/api/v1/trees/acme/web", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				s := decode[treeSummary](t, rec)
				if s.Tree.ID != f.web.ID || s.BranchCount != 2 || s.SnapshotCount != 3 || s.StagedCount != 1 || s.OpenMRCount != 1 {
					t.Fatalf("summary = %+v", s)
				}
				raw := decode[map[string]any](t, rec)
				for _, k := range []string{"tree", "branch_count", "snapshot_count", "staged_count", "open_mr_count"} {
					if _, ok := raw[k]; !ok {
						t.Fatalf("summary missing %q: %s", k, rec.Body.String())
					}
				}
			}},
		{name: "get private anonymous", method: "GET", path: "/api/v1/trees/acme/web", status: 401, code: "unauthenticated"},
		{name: "get private outsider", method: "GET", path: "/api/v1/trees/acme/web", as: "dave", status: 403, code: "forbidden"},
		{name: "get public anonymous", method: "GET", path: "/api/v1/trees/acme/docs", status: 200},
		{name: "get missing", method: "GET", path: "/api/v1/trees/acme/nope", as: "alice", status: 404, code: "not_found"},
		{name: "patch", method: "PATCH", path: "/api/v1/trees/acme/web", as: "mallory",
			body: map[string]string{"visibility": "public", "description": "now public"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				tr := decode[domain.Tree](t, rec)
				if tr.Visibility != "public" || tr.Description != "now public" || !tr.UpdatedAt.After(t0) {
					t.Fatalf("tree = %+v", tr)
				}
				if f.store.trees[f.web.ID].Visibility != "public" {
					t.Fatal("not persisted")
				}
				hasAudit(t, f, "tree.update")
			}},
		{name: "patch as developer", method: "PATCH", path: "/api/v1/trees/acme/web", as: "bob", body: map[string]string{"visibility": "public"}, status: 403, code: "forbidden"},
		{name: "patch empty", method: "PATCH", path: "/api/v1/trees/acme/web", as: "alice", body: map[string]string{}, status: 400, code: "bad_request"},
		{name: "patch bad branch", method: "PATCH", path: "/api/v1/trees/acme/web", as: "alice", body: map[string]string{"default_branch": "bad name"}, status: 400, code: "bad_request"},
		{name: "delete", method: "DELETE", path: "/api/v1/trees/acme/web", as: "alice", status: 204,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if _, ok := f.store.trees[f.web.ID]; ok {
					t.Fatal("tree not deleted")
				}
				hasAudit(t, f, "tree.delete")
			}},
		{name: "delete as developer", method: "DELETE", path: "/api/v1/trees/acme/web", as: "bob", status: 403, code: "forbidden"},
		{name: "branches", method: "GET", path: "/api/v1/trees/acme/web/branches", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				bs := decode[[]domain.Branch](t, rec)
				if len(bs) != 2 || bs[0].Name != "feature" || bs[1].Name != "main" || bs[1].Tip != f.s2.ID {
					t.Fatalf("branches = %+v", bs)
				}
			}},
		{name: "branches empty", method: "GET", path: "/api/v1/trees/acme/docs/branches", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if strings.TrimSpace(rec.Body.String()) != "[]" {
					t.Fatalf("body = %s", rec.Body.String())
				}
			}},
		{name: "tags", method: "GET", path: "/api/v1/trees/acme/web/tags", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				tags := decode[[]domain.Tag](t, rec)
				if len(tags) != 1 || tags[0].Name != "v1" || tags[0].Target != f.s1.ID {
					t.Fatalf("tags = %+v", tags)
				}
			}},
		{name: "staged", method: "GET", path: "/api/v1/trees/acme/web/staged", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				st := decode[[]domain.StagedSnapshot](t, rec)
				if len(st) != 1 || st[0].SnapshotID != "wip-1" || st[0].FilesChanged == nil {
					t.Fatalf("staged = %+v", st)
				}
			}},
		{name: "staged other branch", method: "GET", path: "/api/v1/trees/acme/web/staged?branch=dev", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if len(decode[[]domain.StagedSnapshot](t, rec)) != 0 {
					t.Fatal("want empty")
				}
			}},
		{name: "staged outsider", method: "GET", path: "/api/v1/trees/acme/web/staged", as: "dave", status: 403, code: "forbidden"},
	})
}

func TestSnapshotEndpoints(t *testing.T) {
	ids := func(want ...string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			var got []string
			for _, s := range decode[[]domain.Snapshot](t, rec) {
				got = append(got, s.ID)
			}
			if !slices.Equal(got, want) {
				t.Fatalf("snapshots = %v, want %v", got, want)
			}
		}
	}
	runCases(t, []endpointCase{
		{name: "list default branch", method: "GET", path: "/api/v1/trees/acme/web/snapshots", as: "carol", status: 200, check: ids("snap-2", "snap-1")},
		{name: "list limit", method: "GET", path: "/api/v1/trees/acme/web/snapshots?limit=1", as: "carol", status: 200, check: ids("snap-2")},
		{name: "list branch", method: "GET", path: "/api/v1/trees/acme/web/snapshots?branch=feature", as: "carol", status: 200, check: ids("snap-2", "snap-1")},
		{name: "list unknown branch", method: "GET", path: "/api/v1/trees/acme/web/snapshots?branch=nope", as: "carol", status: 404, code: "not_found"},
		{name: "list bad limit", method: "GET", path: "/api/v1/trees/acme/web/snapshots?limit=abc", as: "carol", status: 400, code: "bad_request"},
		{name: "list no default branch yet", method: "GET", path: "/api/v1/trees/acme/docs/snapshots", status: 200, check: ids()},
		{name: "list parents never null", method: "GET", path: "/api/v1/trees/acme/web/snapshots", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if strings.Contains(rec.Body.String(), `"parents":null`) {
					t.Fatalf("parents encoded as null: %s", rec.Body.String())
				}
			}},
		{name: "list outsider", method: "GET", path: "/api/v1/trees/acme/web/snapshots", as: "dave", status: 403, code: "forbidden"},
		{name: "detail", method: "GET", path: "/api/v1/trees/acme/web/snapshots/snap-2", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				d := decode[snapshotDetail](t, rec)
				if d.Snapshot.ID != "snap-2" || len(d.Files) != 5 || d.RestrictedCount != 1 { // secret/key.txt is license-restricted (§13.1)
					t.Fatalf("detail = %s", rec.Body.String())
				}
				var paths []string
				for _, fe := range d.Files {
					paths = append(paths, fe.Path)
				}
				if !slices.IsSorted(paths) {
					t.Fatalf("files not sorted: %v", paths)
				}
				main := d.Files[slices.Index(paths, "src/main.go")]
				if main.Hash != hash.Sum([]byte("package main\n\nfunc main() {}\n")) || main.Size != 29 || main.Executable {
					t.Fatalf("entry = %+v", main)
				}
				if !d.Files[slices.Index(paths, "run.sh")].Executable {
					t.Fatal("run.sh should be executable")
				}
				raw := decode[struct {
					Files []map[string]any `json:"files"`
				}](t, rec)
				if h, _ := raw.Files[0]["hash"].(string); len(h) != 64 {
					t.Fatalf("hash not hex: %v", raw.Files[0])
				}
			}},
		{name: "detail other tree", method: "GET", path: "/api/v1/trees/acme/web/snapshots/snap-foreign", as: "carol", status: 404, code: "not_found"},
		{name: "detail missing", method: "GET", path: "/api/v1/trees/acme/web/snapshots/nope", as: "carol", status: 404, code: "not_found"},
		{name: "detail missing manifest is 500", method: "GET", path: "/api/v1/trees/acme/web/snapshots/snap-orphan", as: "carol", status: 500, code: "internal"},
		{name: "blob", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-2/src/main.go", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if rec.Body.String() != "package main\n\nfunc main() {}\n" {
					t.Fatalf("body = %q", rec.Body.String())
				}
				h := rec.Header()
				if h.Get("ETag") == "" || h.Get("X-Content-Type-Options") != "nosniff" || h.Get("Content-Length") != "29" {
					t.Fatalf("headers = %v", h)
				}
			}},
		{name: "blob sniffed type", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-2/LICENSE", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if ct := rec.Header().Get("Content-Type"); ct != "text/plain; charset=utf-8" {
					t.Fatalf("Content-Type = %q", ct)
				}
			}},
		{name: "blob html neutralized", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-2/index.html", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if ct := rec.Header().Get("Content-Type"); ct != "text/plain; charset=utf-8" {
					t.Fatalf("Content-Type = %q", ct)
				}
				if !strings.Contains(rec.Header().Get("Content-Security-Policy"), "sandbox") {
					t.Fatal("missing sandbox CSP")
				}
			}},
		{name: "blob download", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-2/README.md?download", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if cd := rec.Header().Get("Content-Disposition"); cd != `attachment; filename=README.md` {
					t.Fatalf("Content-Disposition = %q", cd)
				}
			}},
		{name: "blob license denied", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-2/secret/key.txt", as: "carol", status: 451, code: "license_restricted",
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if strings.Contains(rec.Body.String(), "hunter2") {
					t.Fatal("content leaked")
				}
			}},
		{name: "blob missing file", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-1/src/main.go", as: "carol", status: 404, code: "not_found"},
		{name: "blob private anonymous", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-2/README.md", status: 401, code: "unauthenticated"},
		{name: "blob foreign snapshot", method: "GET", path: "/api/v1/trees/acme/web/blob/snap-foreign/README.md", as: "carol", status: 404, code: "not_found"},
	})
}

func TestBlobConditionalAndRange(t *testing.T) {
	f := newFixture(t)
	const target = "/api/v1/trees/acme/web/blob/snap-2/src/main.go"
	rec := f.do(t, "GET", target, "carol", nil)
	etag := rec.Header().Get("ETag")

	req := httptest.NewRequest("GET", target, nil)
	req.Header.Set("Authorization", "Bearer carol")
	req.Header.Set("If-None-Match", etag)
	rec = httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	if rec.Code != http.StatusNotModified {
		t.Fatalf("If-None-Match status = %d", rec.Code)
	}

	req = httptest.NewRequest("GET", target, nil)
	req.Header.Set("Authorization", "Bearer carol")
	req.Header.Set("Range", "bytes=0-6")
	rec = httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	if rec.Code != http.StatusPartialContent || rec.Body.String() != "package" {
		t.Fatalf("Range: status %d body %q", rec.Code, rec.Body.String())
	}
}

func TestProtectionAndLicenseEndpoints(t *testing.T) {
	runCases(t, []endpointCase{
		{name: "list protections", method: "GET", path: "/api/v1/trees/acme/web/protections", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				ps := decode[[]domain.BranchProtection](t, rec)
				if len(ps) != 1 || ps[0].Pattern != "main" || ps[0].AllowedPushers == nil {
					t.Fatalf("protections = %s", rec.Body.String())
				}
			}},
		{name: "put protection", method: "PUT", path: "/api/v1/trees/acme/web/protections", as: "alice",
			body: domain.BranchProtection{TreeID: "spoofed", Pattern: "release/*", RequiredApprovals: 2, AllowedPushers: []string{"bob"}, NoDelete: true}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				p := decode[domain.BranchProtection](t, rec)
				if p.TreeID != f.web.ID || p.Pattern != "release/*" || p.RequiredApprovals != 2 {
					t.Fatalf("protection = %+v", p)
				}
				if _, ok := f.store.protections[[2]string{f.web.ID, "release/*"}]; !ok {
					t.Fatal("not stored under the path's tree")
				}
				hasAudit(t, f, "protection.update")
			}},
		{name: "put protection as developer", method: "PUT", path: "/api/v1/trees/acme/web/protections", as: "bob",
			body: domain.BranchProtection{Pattern: "main"}, status: 403, code: "forbidden"},
		{name: "put protection bad glob", method: "PUT", path: "/api/v1/trees/acme/web/protections", as: "alice",
			body: domain.BranchProtection{Pattern: "[oops"}, status: 400, code: "bad_request"},
		{name: "put protection bad approvals", method: "PUT", path: "/api/v1/trees/acme/web/protections", as: "alice",
			body: domain.BranchProtection{Pattern: "main", RequiredApprovals: -1}, status: 400, code: "bad_request"},
		{name: "delete protection", method: "DELETE", path: "/api/v1/trees/acme/web/protections?pattern=main", as: "alice", status: 204,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) { hasAudit(t, f, "protection.delete") }},
		{name: "delete protection missing param", method: "DELETE", path: "/api/v1/trees/acme/web/protections", as: "alice", status: 400, code: "bad_request"},
		{name: "delete protection unknown", method: "DELETE", path: "/api/v1/trees/acme/web/protections?pattern=dev", as: "alice", status: 404, code: "not_found"},
		{name: "list licenses", method: "GET", path: "/api/v1/trees/acme/web/licenses", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				rs := decode[[]domain.LicenseRule](t, rec)
				if len(rs) != 1 || rs[0].PathGlob != "secret/**" {
					t.Fatalf("licenses = %+v", rs)
				}
			}},
		{name: "put license", method: "PUT", path: "/api/v1/trees/acme/web/licenses", as: "alice",
			body: domain.LicenseRule{PathGlob: "vendor/**", SPDX: "MIT", Grant: "redistribute"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[domain.LicenseRule](t, rec)
				if r.TreeID != f.web.ID || r.SPDX != "MIT" {
					t.Fatalf("rule = %+v", r)
				}
				hasAudit(t, f, "license.update")
			}},
		{name: "put license bad grant", method: "PUT", path: "/api/v1/trees/acme/web/licenses", as: "alice",
			body: domain.LicenseRule{PathGlob: "**", SPDX: "MIT", Grant: "steal"}, status: 400, code: "bad_request"},
		{name: "put license as viewer", method: "PUT", path: "/api/v1/trees/acme/web/licenses", as: "carol",
			body: domain.LicenseRule{PathGlob: "**", SPDX: "MIT", Grant: "modify"}, status: 403, code: "forbidden"},
		{name: "delete license", method: "DELETE", path: "/api/v1/trees/acme/web/licenses?path_glob=secret/**", as: "alice", status: 204,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) { hasAudit(t, f, "license.delete") }},
		{name: "delete license missing param", method: "DELETE", path: "/api/v1/trees/acme/web/licenses", as: "alice", status: 400, code: "bad_request"},
	})
}

func TestMergeRequestEndpoints(t *testing.T) {
	approve := func(t *testing.T, f *fixture) {
		if rec := f.do(t, "POST", "/api/v1/trees/acme/web/merge-requests/1/approve", "bob", nil); rec.Code != 200 {
			t.Fatalf("approve = %d %s", rec.Code, rec.Body.String())
		}
	}
	runCases(t, []endpointCase{
		{name: "list", method: "GET", path: "/api/v1/trees/acme/web/merge-requests", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				mrs := decode[[]domain.MergeRequest](t, rec)
				if len(mrs) != 1 || mrs[0].Number != 1 || mrs[0].Approvals == nil {
					t.Fatalf("mrs = %s", rec.Body.String())
				}
			}},
		{name: "list merged", method: "GET", path: "/api/v1/trees/acme/web/merge-requests?state=merged", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if len(decode[[]domain.MergeRequest](t, rec)) != 0 {
					t.Fatal("want none")
				}
			}},
		{name: "list bad state", method: "GET", path: "/api/v1/trees/acme/web/merge-requests?state=bogus", as: "carol", status: 400, code: "bad_request"},
		{name: "create", method: "POST", path: "/api/v1/trees/acme/web/merge-requests", as: "bob",
			body: map[string]string{"title": "Add login", "description": "d", "source_branch": "feature", "target_branch": "main"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				mr := decode[domain.MergeRequest](t, rec)
				if mr.Number != 2 || mr.Author != "bob" || mr.State != "open" {
					t.Fatalf("mr = %+v", mr)
				}
			}},
		{name: "create same branches", method: "POST", path: "/api/v1/trees/acme/web/merge-requests", as: "bob",
			body: map[string]string{"title": "x", "source_branch": "main", "target_branch": "main"}, status: 400, code: "bad_request"},
		{name: "create no title", method: "POST", path: "/api/v1/trees/acme/web/merge-requests", as: "bob",
			body: map[string]string{"source_branch": "feature", "target_branch": "main"}, status: 400, code: "bad_request"},
		{name: "create invalid per service", method: "POST", path: "/api/v1/trees/acme/web/merge-requests", as: "bob",
			body: map[string]string{"title": "x", "source_branch": "ghost", "target_branch": "main"}, status: 400, code: "bad_request"},
		{name: "create as viewer", method: "POST", path: "/api/v1/trees/acme/web/merge-requests", as: "carol",
			body: map[string]string{"title": "x", "source_branch": "feature", "target_branch": "main"}, status: 403, code: "forbidden"},
		{name: "create anonymous", method: "POST", path: "/api/v1/trees/acme/web/merge-requests",
			body: map[string]string{"title": "x", "source_branch": "feature", "target_branch": "main"}, status: 401, code: "unauthenticated"},
		{name: "get", method: "GET", path: "/api/v1/trees/acme/web/merge-requests/1", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if mr := decode[domain.MergeRequest](t, rec); mr.Title != "Feature" {
					t.Fatalf("mr = %+v", mr)
				}
			}},
		{name: "get missing", method: "GET", path: "/api/v1/trees/acme/web/merge-requests/99", as: "carol", status: 404, code: "not_found"},
		{name: "get bad number", method: "GET", path: "/api/v1/trees/acme/web/merge-requests/abc", as: "carol", status: 400, code: "bad_request"},
		{name: "approve", method: "POST", path: "/api/v1/trees/acme/web/merge-requests/1/approve", as: "bob", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if mr := decode[domain.MergeRequest](t, rec); len(mr.Approvals) != 1 || mr.Approvals[0].Username != "bob" {
					t.Fatalf("mr = %+v", mr)
				}
			}},
		{name: "approve as viewer", method: "POST", path: "/api/v1/trees/acme/web/merge-requests/1/approve", as: "carol", status: 403, code: "forbidden"},
		{name: "merge needs review", method: "POST", path: "/api/v1/trees/acme/web/merge-requests/1/merge", as: "alice", status: 403, code: "review_required"},
		{name: "merge approved", setup: approve, method: "POST", path: "/api/v1/trees/acme/web/merge-requests/1/merge", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if mr := decode[domain.MergeRequest](t, rec); mr.State != domain.MRStateMerged {
					t.Fatalf("state = %s", mr.State)
				}
			}},
		{name: "close", method: "POST", path: "/api/v1/trees/acme/web/merge-requests/1/close", as: "bob", status: 200},
		{name: "close twice", setup: func(t *testing.T, f *fixture) {
			if rec := f.do(t, "POST", "/api/v1/trees/acme/web/merge-requests/1/close", "bob", nil); rec.Code != 200 {
				t.Fatalf("close = %d", rec.Code)
			}
		}, method: "POST", path: "/api/v1/trees/acme/web/merge-requests/1/close", as: "bob", status: 409, code: "conflict"},
		{name: "action anonymous", method: "POST", path: "/api/v1/trees/acme/docs/merge-requests/1/close", status: 401, code: "unauthenticated"},
	})
}

func TestAuditAndAdminEndpoints(t *testing.T) {
	seedAudit := func(t *testing.T, f *fixture) {
		f.audit.Record(context.Background(), domain.AuditEvent{ID: "a1", TenantID: f.acme.ID, Action: "tree.create", At: t0})
		f.audit.Record(context.Background(), domain.AuditEvent{ID: "a2", TenantID: "tenant-dave", Action: "tree.create", At: t0})
		f.audit.Record(context.Background(), domain.AuditEvent{ID: "a3", TenantID: f.acme.ID, Action: "branch.push", At: t0})
	}
	auditIDs := func(want ...string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			var got []string
			for _, e := range decode[[]domain.AuditEvent](t, rec) {
				got = append(got, e.ID)
			}
			if !slices.Equal(got, want) {
				t.Fatalf("audit = %v, want %v", got, want)
			}
		}
	}
	runCases(t, []endpointCase{
		{name: "audit tenant admin", setup: seedAudit, method: "GET", path: "/api/v1/audit?tenant=acme", as: "alice", status: 200, check: auditIDs("a3", "a1")},
		{name: "audit action filter", setup: seedAudit, method: "GET", path: "/api/v1/audit?tenant=acme&action=tree.create", as: "mallory", status: 200, check: auditIDs("a1")},
		{name: "audit limit", setup: seedAudit, method: "GET", path: "/api/v1/audit?limit=1", as: "admin", status: 200, check: auditIDs("a3")},
		{name: "audit server-wide", setup: seedAudit, method: "GET", path: "/api/v1/audit", as: "admin", status: 200, check: auditIDs("a3", "a2", "a1")},
		{name: "audit developer", method: "GET", path: "/api/v1/audit?tenant=acme", as: "bob", status: 403, code: "forbidden"},
		// Without a tenant, non-admins see the tenants they administer.
		{name: "audit unscoped tenant admin", setup: seedAudit, method: "GET", path: "/api/v1/audit", as: "alice", status: 200, check: auditIDs("a3", "a1")},
		{name: "audit unscoped developer", setup: seedAudit, method: "GET", path: "/api/v1/audit", as: "bob", status: 200, check: auditIDs()},
		{name: "audit anonymous", method: "GET", path: "/api/v1/audit", status: 401, code: "unauthenticated"},
		{name: "audit bad since", method: "GET", path: "/api/v1/audit?since=yesterday", as: "admin", status: 400, code: "bad_request"},
		{name: "stats", method: "GET", path: "/api/v1/admin/stats", as: "admin", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				st := decode[domain.ServerStats](t, rec)
				n, b, _ := f.objs.Stats(context.Background())
				want := domain.ServerStats{Tenants: 7, Accounts: 6, Trees: 2, Branches: 2, Snapshots: 4, Staged: 1, Objects: n, StorageBytes: b}
				if st != want {
					t.Fatalf("stats = %+v, want %+v", st, want)
				}
				raw := decode[map[string]any](t, rec)
				if _, ok := raw["storage_bytes"]; !ok {
					t.Fatal("missing storage_bytes")
				}
			}},
		{name: "stats non-admin", method: "GET", path: "/api/v1/admin/stats", as: "alice", status: 403, code: "forbidden"},
		{name: "stats anonymous", method: "GET", path: "/api/v1/admin/stats", status: 401, code: "unauthenticated"},
		{name: "accounts", method: "GET", path: "/api/v1/admin/accounts", as: "admin", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				var got []string
				for _, a := range decode[[]domain.Account](t, rec) {
					got = append(got, a.Username)
				}
				if !slices.Equal(got, []string{"admin", "alice", "bob", "carol", "dave", "mallory"}) {
					t.Fatalf("accounts = %v", got)
				}
			}},
		{name: "accounts non-admin", method: "GET", path: "/api/v1/admin/accounts", as: "alice", status: 403, code: "forbidden"},
		{name: "disable account", method: "PATCH", path: "/api/v1/admin/accounts/bob", as: "admin", body: map[string]bool{"disabled": true}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if a := decode[domain.Account](t, rec); !a.Disabled || !f.store.accounts["acct-bob"].Disabled {
					t.Fatalf("account = %+v", a)
				}
				hasAudit(t, f, "account.update")
			}},
		{name: "promote account", method: "PATCH", path: "/api/v1/admin/accounts/carol", as: "admin", body: map[string]bool{"is_admin": true}, status: 200},
		{name: "self demote", method: "PATCH", path: "/api/v1/admin/accounts/admin", as: "admin", body: map[string]bool{"is_admin": false}, status: 409, code: "conflict"},
		{name: "self disable", method: "PATCH", path: "/api/v1/admin/accounts/admin", as: "admin", body: map[string]bool{"disabled": true}, status: 409, code: "conflict"},
		{name: "patch empty", method: "PATCH", path: "/api/v1/admin/accounts/bob", as: "admin", body: map[string]bool{}, status: 400, code: "bad_request"},
		{name: "patch unknown", method: "PATCH", path: "/api/v1/admin/accounts/zed", as: "admin", body: map[string]bool{"disabled": true}, status: 404, code: "not_found"},
		{name: "patch non-admin", method: "PATCH", path: "/api/v1/admin/accounts/bob", as: "alice", body: map[string]bool{"disabled": true}, status: 403, code: "forbidden"},
	})
}

func TestRoutingAndMiddleware(t *testing.T) {
	runCases(t, []endpointCase{
		{name: "unknown api path", method: "GET", path: "/api/v1/nope", status: 404, code: "not_found"},
		{name: "wrong method", method: "DELETE", path: "/api/v1/tenants", as: "alice", status: 405, code: "method_not_allowed",
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				allow := rec.Header().Values("Allow")
				if !slices.Contains(allow, "GET") || !slices.Contains(allow, "POST") {
					t.Fatalf("Allow = %v", allow)
				}
			}},
		{name: "request id generated", method: "GET", path: "/healthz", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if len(rec.Header().Get(RequestIDHeader)) != 24 {
					t.Fatalf("request id = %q", rec.Header().Get(RequestIDHeader))
				}
			}},
		{name: "root redirects to admin", method: "GET", path: "/", status: 302},
		{name: "admin redirect", method: "GET", path: "/admin", status: 301},
		{name: "admin not built", method: "GET", path: "/admin/", status: 404,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if !strings.Contains(rec.Body.String(), "npm run build") || !strings.HasPrefix(rec.Header().Get("Content-Type"), "text/html") {
					t.Fatalf("body = %s", rec.Body.String())
				}
			}},
	})
}

func TestRequestIDPropagated(t *testing.T) {
	f := newFixture(t)
	req := httptest.NewRequest("GET", "/healthz", nil)
	req.Header.Set(RequestIDHeader, "abc-123")
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	if got := rec.Header().Get(RequestIDHeader); got != "abc-123" {
		t.Fatalf("request id = %q", got)
	}
	req.Header.Set(RequestIDHeader, "bad id\x00")
	rec = httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	if got := rec.Header().Get(RequestIDHeader); got == "bad id\x00" || got == "" {
		t.Fatalf("invalid request id not replaced: %q", got)
	}
}

func TestCORS(t *testing.T) {
	f := newFixture(t, func(d *Deps) { d.CORSOrigins = []string{"https://admin.example.com"} })
	tests := []struct {
		origin  string
		allowed bool
	}{
		{"http://localhost:5173", true},
		{"https://admin.example.com", true},
		{"https://evil.example.com", false},
	}
	for _, tc := range tests {
		t.Run(tc.origin, func(t *testing.T) {
			req := httptest.NewRequest("OPTIONS", "/api/v1/tenants", nil)
			req.Header.Set("Origin", tc.origin)
			req.Header.Set("Access-Control-Request-Method", "POST")
			req.Header.Set("Access-Control-Request-Headers", "authorization, content-type")
			rec := httptest.NewRecorder()
			f.h.ServeHTTP(rec, req)
			got := rec.Header().Get("Access-Control-Allow-Origin")
			if tc.allowed {
				if rec.Code != http.StatusNoContent || got != tc.origin ||
					!strings.Contains(rec.Header().Get("Access-Control-Allow-Headers"), "Authorization") {
					t.Fatalf("preflight: %d %v", rec.Code, rec.Header())
				}
			} else if got != "" {
				t.Fatalf("origin %s allowed", tc.origin)
			}

			req = httptest.NewRequest("GET", "/healthz", nil)
			req.Header.Set("Origin", tc.origin)
			rec = httptest.NewRecorder()
			f.h.ServeHTTP(rec, req)
			if (rec.Header().Get("Access-Control-Allow-Origin") != "") != tc.allowed {
				t.Fatalf("simple request CORS header = %q", rec.Header().Get("Access-Control-Allow-Origin"))
			}
		})
	}
}

func TestRecoverMiddleware(t *testing.T) {
	a := &api{log: slog.New(slog.NewTextHandler(io.Discard, nil)), metrics: newMetrics()}
	h := requestIDMiddleware(a.instrumentMiddleware(a.recoverMiddleware(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		panic("boom")
	}))))
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, httptest.NewRequest("GET", "/x", nil))
	if rec.Code != http.StatusInternalServerError || errCode(rec) != "internal" {
		t.Fatalf("status = %d body %s", rec.Code, rec.Body.String())
	}
	if a.metrics.requests[counterKey{"unmatched", 500}] != 1 {
		t.Fatalf("panic not counted: %v", a.metrics.requests)
	}
}

func TestAdminSPA(t *testing.T) {
	dir := t.TempDir()
	must(t, os.WriteFile(filepath.Join(dir, "index.html"), []byte("<!doctype html><div id=root></div>"), 0o644))
	must(t, os.MkdirAll(filepath.Join(dir, "assets"), 0o755))
	must(t, os.WriteFile(filepath.Join(dir, "assets", "app-123.js"), []byte("console.log(1)"), 0o644))
	must(t, os.WriteFile(filepath.Join(dir, "favicon.svg"), []byte("<svg/>"), 0o644))
	f := newFixture(t, func(d *Deps) { d.AdminDir = dir })

	tests := []struct {
		path, wantBody, wantCache string
		status                    int
	}{
		{"/admin/", "id=root", "no-cache", 200},
		{"/admin/trees/acme/web", "id=root", "no-cache", 200}, // SPA fallback
		{"/admin/assets/app-123.js", "console.log", "immutable", 200},
		{"/admin/favicon.svg", "<svg/>", "no-cache", 200},
		{"/admin/assets/missing.js", "", "", 404},
		{"/admin/missing.png", "", "", 404},
	}
	for _, tc := range tests {
		t.Run(tc.path, func(t *testing.T) {
			rec := f.do(t, "GET", tc.path, "", nil)
			if rec.Code != tc.status {
				t.Fatalf("status = %d, want %d", rec.Code, tc.status)
			}
			if !strings.Contains(rec.Body.String(), tc.wantBody) || !strings.Contains(rec.Header().Get("Cache-Control"), tc.wantCache) {
				t.Fatalf("body %q cache %q", rec.Body.String(), rec.Header().Get("Cache-Control"))
			}
		})
	}
}

func TestClassify(t *testing.T) {
	tests := []struct {
		err    error
		status int
		code   string
	}{
		{domain.ErrInvalid, 400, "bad_request"},
		{domain.ErrUnauthenticated, 401, "unauthenticated"},
		{domain.ErrForbidden, 403, "forbidden"},
		{domain.ErrNotFound, 404, "not_found"},
		{domain.ErrAlreadyExists, 409, "conflict"},
		{domain.ErrConflict, 409, "conflict"},
		{&domain.PushDenied{Kind: "branch_protection", Reason: "no direct push"}, 403, "branch_protection"},
		{errors.New("wrapped: " + "x"), 500, "internal"},
	}
	for _, tc := range tests {
		status, code, _ := classify(errors.Join(errors.New("ctx"), tc.err))
		if status != tc.status || code != tc.code {
			t.Errorf("classify(%v) = %d %s, want %d %s", tc.err, status, code, tc.status, tc.code)
		}
	}
}
