package httpapi

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"slices"
	"sort"
	"strings"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Read path security over the real store, authorizer and license checker
// (docs/ENGINEERING_PLAN.md §13.1):
//
//	tenant acme: alice owner, bob developer; olga and partner-org outsiders
//	tree acme/app (public), branch main: p1 <- p2
//	policy hide-secrets: bob may not read secret/**
//	license: vendor/** is proprietary (no grant)

const appAPI = "/api/v1/trees/acme/app"

type pathsAuth struct {
	domain.Authenticator
	users map[string]*domain.Principal
}

func (a *pathsAuth) Authenticate(_ context.Context, token string) (*domain.Principal, error) {
	if p := a.users[token]; p != nil {
		return p, nil
	}
	return nil, domain.ErrUnauthenticated
}

type pathsFixture struct {
	store  *meta.Memory
	objs   *objstore.DiskStore
	h      http.Handler
	tree   *domain.Tree
	p1, p2 string
	big    []byte // content of the chunked file big.bin
}

func newPathsFixture(t *testing.T) *pathsFixture {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	f := &pathsFixture{store: store, objs: objstore.New(t.TempDir())}
	users := map[string]*domain.Principal{}
	must(t, store.CreateTenant(ctx, &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}))
	must(t, store.CreateTenant(ctx, &domain.Tenant{ID: "t-partner", Slug: "partner-org", Name: "Partner", Kind: domain.TenantOrg, CreatedAt: t0}))
	for _, u := range []struct{ name, role string }{{"alice", domain.RoleOwner}, {"bob", domain.RoleDeveloper}, {"olga", ""}} {
		must(t, store.CreateTenant(ctx, &domain.Tenant{ID: "t-" + u.name, Slug: u.name, Name: u.name, Kind: domain.TenantUser, CreatedAt: t0}))
		must(t, store.CreateAccount(ctx, &domain.Account{ID: "a-" + u.name, Username: u.name, Email: u.name + "@example.com", TenantID: "t-" + u.name, CreatedAt: t0}))
		if u.role != "" {
			must(t, store.PutMembership(ctx, &domain.Membership{TenantID: "t-acme", AccountID: "a-" + u.name, Role: u.role, CreatedAt: t0}))
		}
		users[u.name] = &domain.Principal{AccountID: "a-" + u.name, Username: u.name, TenantID: "t-" + u.name}
	}
	f.tree = &domain.Tree{ID: "tree-app", TenantID: "t-acme", Name: "app", DefaultBranch: "main", Visibility: domain.VisibilityPublic, CreatedAt: t0, UpdatedAt: t0}
	must(t, store.CreateTree(ctx, f.tree))
	must(t, store.CreatePolicy(ctx, &domain.Policy{TenantID: "t-acme", Name: "hide-secrets", Enabled: true, Version: 1, CreatedAt: t0, UpdatedAt: t0,
		Statements: []domain.PolicyStatement{{Effect: domain.EffectDeny, Actions: []string{"tree:read"}, Resources: []string{"wt:path/acme/app/secret/**"}, Principals: []string{"user:bob"}}}}))
	must(t, store.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: f.tree.ID, PathGlob: "vendor/**", SPDX: "LicenseRef-Proprietary"}))

	// big.bin is stored chunked: a CHUNK_LIST of three blobs.
	f.big = []byte(strings.Repeat("chunk-a|", 100) + strings.Repeat("chunk-b|", 50) + "tail")
	cl := &syncpb.ChunkList{Size: uint64(len(f.big))}
	for _, part := range [][]byte{f.big[:800], f.big[800:1200], f.big[1200:]} {
		h := f.put(t, part)
		cl.Chunks = append(cl.Chunks, &syncpb.Chunk{Hash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(part))})
	}
	clRaw, err := proto.Marshal(cl)
	must(t, err)
	big := &syncpb.ManifestEntry{Path: "big.bin", Size: uint64(len(f.big)), Chunked: true}
	clHash := f.put(t, clRaw)
	big.BlobHash = &syncpb.ContentHash{Value: clHash[:]}

	m1 := f.manifest(t, map[string]string{"README.md": "one\n", "secret/key.txt": "k1\n", "vendor/lib.c": "lib1\n"}, big)
	m2 := f.manifest(t, map[string]string{"README.md": "two\n", "secret/key.txt": "k2\n", "vendor/lib.c": "lib2\n", "src/a.go": "package a\n"}, big)
	f.p1, f.p2 = "snap-p1", "snap-p2"
	must(t, store.PutSnapshot(ctx, &domain.Snapshot{ID: f.p1, TreeID: f.tree.ID, BranchName: "main", ManifestHash: m1, Author: "alice", Message: "one", CreatedAt: t0}))
	must(t, store.PutSnapshot(ctx, &domain.Snapshot{ID: f.p2, TreeID: f.tree.ID, BranchName: "main", Parents: []string{f.p1}, ManifestHash: m2, Author: "alice", Message: "two", CreatedAt: t0.Add(time.Hour)}))
	must(t, store.PutBranch(ctx, &domain.Branch{TreeID: f.tree.ID, Name: "main", Tip: f.p2, CreatedAt: t0, UpdatedAt: t0}))

	az := authz.New(store, authz.Options{})
	f.h = New(Deps{
		Store:   store,
		Objects: f.objs,
		History: meta.NewHistory(store),
		Auth:    &pathsAuth{users: users},
		Authz:   az,
		Access:  az,
		License: license.New(store),
		Logger:  slog.New(slog.NewTextHandler(io.Discard, nil)),
	})
	return f
}

func (f *pathsFixture) put(t *testing.T, data []byte) hash.Hash {
	t.Helper()
	h := hash.Sum(data)
	must(t, f.objs.Put(context.Background(), h, data))
	return h
}

func (f *pathsFixture) manifest(t *testing.T, files map[string]string, extra ...*syncpb.ManifestEntry) hash.Hash {
	t.Helper()
	m := &syncpb.ManifestRecord{Entries: extra}
	for p, c := range files {
		h := f.put(t, []byte(c))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(c))})
	}
	sort.Slice(m.Entries, func(i, j int) bool { return m.Entries[i].GetPath() < m.Entries[j].GetPath() })
	raw, err := proto.Marshal(m)
	must(t, err)
	return f.put(t, raw)
}

func (f *pathsFixture) do(t *testing.T, method, target, as string, body any) *httptest.ResponseRecorder {
	t.Helper()
	var rd io.Reader
	if body != nil {
		buf, err := json.Marshal(body)
		must(t, err)
		rd = strings.NewReader(string(buf))
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

func wantStatus(t *testing.T, rec *httptest.ResponseRecorder, status int, code string) {
	t.Helper()
	if rec.Code != status || (code != "" && errCode(rec) != code) {
		t.Fatalf("status = %d (%s), want %d %s; body: %s", rec.Code, errCode(rec), status, code, rec.Body.String())
	}
}

func TestPathsBlob(t *testing.T) {
	f := newPathsFixture(t)
	cases := []struct {
		name, as, path string
		status         int
		code, body     string
	}{
		{"owner reads secret", "alice", "secret/key.txt", 200, "", "k2\n"},
		{"path denied", "bob", "secret/key.txt", 403, codePathDenied, ""},
		{"path denied hides existence", "bob", "secret/missing.txt", 403, codePathDenied, ""},
		{"bob reads vendor (member)", "bob", "vendor/lib.c", 200, "", "lib2\n"},
		{"license restricted", "olga", "vendor/lib.c", 451, codeLicenseRestricted, ""},
		{"anonymous license restricted", "", "vendor/lib.c", 451, codeLicenseRestricted, ""},
		{"outsider reads secret", "olga", "secret/key.txt", 200, "", "k2\n"},
		{"unrestricted missing file", "bob", "nope.txt", 404, "not_found", ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+"/blob/"+f.p2+"/"+tc.path, tc.as, nil)
			wantStatus(t, rec, tc.status, tc.code)
			if tc.body != "" && rec.Body.String() != tc.body {
				t.Fatalf("body = %q, want %q", rec.Body.String(), tc.body)
			}
			if tc.status >= 400 && (strings.Contains(rec.Body.String(), "k2") || strings.Contains(rec.Body.String(), "lib2")) {
				t.Fatalf("content leaked: %s", rec.Body.String())
			}
		})
	}
	rec := f.do(t, "GET", appAPI+"/blob/"+f.p2+"/secret/key.txt", "bob", nil)
	if !strings.Contains(rec.Body.String(), "hide-secrets") {
		t.Fatalf("path_denied message does not name the policy: %s", rec.Body.String())
	}
}

func TestPathsBlobChunked(t *testing.T) {
	f := newPathsFixture(t)
	rec := f.do(t, "GET", appAPI+"/blob/"+f.p2+"/big.bin", "olga", nil)
	wantStatus(t, rec, 200, "")
	if rec.Body.String() != string(f.big) {
		t.Fatalf("chunked file = %d bytes, want %d", rec.Body.Len(), len(f.big))
	}
}

func TestPathsSnapshotFiles(t *testing.T) {
	f := newPathsFixture(t)
	cases := []struct {
		as         string
		files      []string
		restricted int
	}{
		{"alice", []string{"README.md", "big.bin", "secret/key.txt", "src/a.go", "vendor/lib.c"}, 0},
		{"bob", []string{"README.md", "big.bin", "src/a.go", "vendor/lib.c"}, 1},
		{"olga", []string{"README.md", "big.bin", "secret/key.txt", "src/a.go"}, 1},
	}
	for _, tc := range cases {
		t.Run(tc.as, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+"/snapshots/"+f.p2, tc.as, nil)
			wantStatus(t, rec, 200, "")
			d := decode[snapshotDetail](t, rec)
			var got []string
			for _, fe := range d.Files {
				got = append(got, fe.Path)
			}
			if !slices.Equal(got, tc.files) || d.RestrictedCount != tc.restricted {
				t.Fatalf("files = %v (restricted %d), want %v (%d)", got, d.RestrictedCount, tc.files, tc.restricted)
			}
			if !strings.Contains(rec.Body.String(), `"restricted_count":`) {
				t.Fatal("restricted_count missing from the response")
			}
		})
	}
}

func TestPathsCompare(t *testing.T) {
	f := newPathsFixture(t)
	cases := []struct {
		as         string
		files      []string
		restricted int
	}{
		{"alice", []string{"README.md", "secret/key.txt", "src/a.go", "vendor/lib.c"}, 0},
		{"bob", []string{"README.md", "src/a.go", "vendor/lib.c"}, 1},
		{"olga", []string{"README.md", "secret/key.txt", "src/a.go"}, 1},
	}
	for _, tc := range cases {
		t.Run(tc.as, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+"/compare?to="+f.p2, tc.as, nil)
			wantStatus(t, rec, 200, "")
			res := decode[compareResult](t, rec)
			var got []string
			for _, fd := range res.Files {
				got = append(got, fd.Path)
			}
			if !slices.Equal(got, tc.files) || res.RestrictedCount != tc.restricted || res.Stats.Files != len(tc.files) {
				t.Fatalf("files = %v (restricted %d, stats %d), want %v (%d)", got, res.RestrictedCount, res.Stats.Files, tc.files, tc.restricted)
			}
		})
	}
}

func TestPathsFileHistory(t *testing.T) {
	f := newPathsFixture(t)
	cases := []struct {
		as, path string
		status   int
		code     string
	}{
		{"alice", "secret/key.txt", 200, ""},
		{"bob", "secret/key.txt", 403, codePathDenied},
		{"bob", "secret/never-existed", 403, codePathDenied},
		{"olga", "vendor/lib.c", 451, codeLicenseRestricted},
		{"olga", "README.md", 200, ""},
	}
	for _, tc := range cases {
		t.Run(tc.as+" "+tc.path, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+"/history?path="+tc.path, tc.as, nil)
			wantStatus(t, rec, tc.status, tc.code)
			if tc.status == 200 {
				if n := len(decode[[]fileHistoryEntry](t, rec)); n != 2 {
					t.Fatalf("history entries = %d, want 2", n)
				}
			}
		})
	}
}

func TestPathsLicenseGranteeAndExpiry(t *testing.T) {
	f := newPathsFixture(t)
	future := time.Now().Add(24 * time.Hour).UTC().Truncate(time.Second)
	past := time.Now().Add(-time.Hour).UTC()
	rule := func(grantee string, expires *time.Time) map[string]any {
		r := map[string]any{"path_glob": "vendor/**", "spdx": "LicenseRef-Proprietary", "grant": "read-only"}
		if grantee != "" {
			r["grantee"] = grantee
		}
		if expires != nil {
			r["expires_at"] = expires.Format(time.RFC3339)
		}
		return r
	}
	bad := []struct {
		name string
		body map[string]any
	}{
		{"unknown grantee", rule("nobody", nil)},
		{"owning tenant", rule("acme", nil)},
		{"malformed grantee", rule("Not A Slug", nil)},
		{"expired", rule("", &past)},
	}
	for _, tc := range bad {
		t.Run(tc.name, func(t *testing.T) {
			wantStatus(t, f.do(t, "PUT", appAPI+"/licenses", "alice", tc.body), 400, "bad_request")
		})
	}

	// Before the grant olga may not read vendor/.
	wantStatus(t, f.do(t, "GET", appAPI+"/blob/"+f.p2+"/vendor/lib.c", "olga", nil), 451, codeLicenseRestricted)

	// A general rule plus a grantee rule for olga's personal tenant.
	must(t, f.store.PutLicenseRule(context.Background(), &domain.LicenseRule{TreeID: f.tree.ID, PathGlob: "**", SPDX: "LicenseRef-Proprietary"}))
	rec := f.do(t, "PUT", appAPI+"/licenses", "alice", rule(" olga ", &future))
	wantStatus(t, rec, 200, "")
	got := decode[domain.LicenseRule](t, rec)
	if got.Grantee != "olga" || got.ExpiresAt == nil || !got.ExpiresAt.Equal(future) {
		t.Fatalf("stored rule = %+v", got)
	}
	list := decode[[]domain.LicenseRule](t, f.do(t, "GET", appAPI+"/licenses", "alice", nil))
	if len(list) != 2 {
		t.Fatalf("rules = %+v", list)
	}

	// olga reads vendor/ through the grant, but not the rest (general rule).
	wantStatus(t, f.do(t, "GET", appAPI+"/blob/"+f.p2+"/vendor/lib.c", "olga", nil), 200, "")
	wantStatus(t, f.do(t, "GET", appAPI+"/blob/"+f.p2+"/README.md", "olga", nil), 451, codeLicenseRestricted)
	// Anonymous callers are not the grantee.
	wantStatus(t, f.do(t, "GET", appAPI+"/blob/"+f.p2+"/vendor/lib.c", "", nil), 451, codeLicenseRestricted)

	// The grant can name an org tenant too.
	wantStatus(t, f.do(t, "PUT", appAPI+"/licenses", "alice", rule("partner-org", nil)), 200, "")
	// Only tree admins may change license rules.
	wantStatus(t, f.do(t, "PUT", appAPI+"/licenses", "bob", rule("", nil)), 403, "")
}

// TestArchiveNeedsRedistributeGrant: an outsider may read a path licensed
// read-only, but a server archive redistributes it, so it is omitted.
func TestArchiveNeedsRedistributeGrant(t *testing.T) {
	f := newPathsFixture(t)
	must(t, f.store.PutLicenseRule(context.Background(), &domain.LicenseRule{TreeID: f.tree.ID, PathGlob: "src/**", SPDX: "MIT", Grant: "read-only"}))
	rec := f.do(t, "GET", appAPI+"/blob/"+f.p2+"/src/a.go", "olga", nil)
	wantStatus(t, rec, 200, "")
	rec = f.do(t, "GET", appAPI+"/archive/"+f.p2, "olga", nil)
	wantStatus(t, rec, 200, "")
	files := untar(t, rec.Body.Bytes())
	if _, ok := files["src/a.go"]; ok {
		t.Fatal("read-only licensed path included in an archive")
	}
	if _, ok := files["README.md"]; !ok {
		t.Fatalf("unrestricted path missing: %v", names(files))
	}
	// The owner tenant is unrestricted.
	rec = f.do(t, "GET", appAPI+"/archive/"+f.p2, "alice", nil)
	wantStatus(t, rec, 200, "")
	if _, ok := untar(t, rec.Body.Bytes())["src/a.go"]; !ok {
		t.Fatal("owner archive lacks src/a.go")
	}
}
