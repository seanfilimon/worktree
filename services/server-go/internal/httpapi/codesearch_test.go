package httpapi

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"reflect"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/codesearch"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// Code search over the §13.1 read-path fixture (paths_test.go) plus:
//
//	tree acme/priv (private): needle.txt, src/deep/needle.go
//	tree partner-org/pub (public): pub.txt
//
// acme/app (public) holds README.md, secret/key.txt (policy-denied for
// bob), vendor/lib.c (proprietary: unreadable for non-members) and
// src/a.go.

type codeSearchFixture struct {
	*pathsFixture
	priv *domain.Tree
	idx  *codesearch.Index
}

func newCodeSearchFixture(t *testing.T) *codeSearchFixture {
	t.Helper()
	ctx := context.Background()
	f := &codeSearchFixture{pathsFixture: newPathsFixture(t)}
	f.priv = &domain.Tree{ID: "tree-priv", TenantID: "t-acme", Name: "priv", DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: t0, UpdatedAt: t0}
	pub := &domain.Tree{ID: "tree-pub", TenantID: "t-partner", Name: "pub", DefaultBranch: "main", Visibility: domain.VisibilityPublic, CreatedAt: t0, UpdatedAt: t0}
	for _, tr := range []*domain.Tree{f.priv, pub} {
		must(t, f.store.CreateTree(ctx, tr))
	}
	f.commit(t, f.priv, "priv-1", "", map[string]string{"needle.txt": "a\nb\nthe needle\nc\nd\ne\n", "src/deep/needle.go": "package deep // Needle\n"})
	f.commit(t, pub, "pub-1", "", map[string]string{"pub.txt": "public needle\n"})

	f.idx = codesearch.New(codesearch.Deps{Store: f.store, Objects: f.objs, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	users := map[string]*domain.Principal{}
	for _, u := range []string{"alice", "bob", "olga"} { // the accounts of newPathsFixture
		users[u] = &domain.Principal{AccountID: "a-" + u, Username: u, TenantID: "t-" + u}
	}
	// search-v2: a server admin.
	must(t, f.store.CreateAccount(ctx, &domain.Account{ID: "a-root", Username: "root", Email: "root@example.com", IsAdmin: true, CreatedAt: t0}))
	users["root"] = &domain.Principal{AccountID: "a-root", Username: "root", IsAdmin: true}
	az := authz.New(f.store, authz.Options{})
	f.h = New(Deps{
		Store:      f.store,
		Objects:    f.objs,
		History:    meta.NewHistory(f.store),
		Auth:       &pathsAuth{users: users},
		Authz:      az,
		Access:     az,
		License:    license.New(f.store),
		Logger:     slog.New(slog.NewTextHandler(io.Discard, nil)),
		CodeSearch: f.idx,
	})
	return f
}

func (f *codeSearchFixture) commit(t *testing.T, tree *domain.Tree, id, parent string, files map[string]string) {
	t.Helper()
	ctx := context.Background()
	var parents []string
	if parent != "" {
		parents = []string{parent}
	}
	must(t, f.store.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: tree.ID, BranchName: "main", Parents: parents, ManifestHash: f.manifest(t, files), Author: "alice", Message: id, CreatedAt: t0}))
	must(t, f.store.CompareAndSwapBranchTip(ctx, tree.ID, "main", parent, id))
}

type codeSearchBody struct {
	Hits          []domain.CodeSearchHit `json:"hits"`
	Truncated     bool                   `json:"truncated"`
	SearchedTrees int                    `json:"searched_trees"`
}

func (f *codeSearchFixture) search(t *testing.T, as string, params url.Values) codeSearchBody {
	t.Helper()
	rec := f.do(t, http.MethodGet, "/api/v1/search/code?"+params.Encode(), as, nil)
	wantStatus(t, rec, http.StatusOK, "")
	return decode[codeSearchBody](t, rec)
}

func hitLocs(b codeSearchBody) []string {
	out := []string{}
	for _, h := range b.Hits {
		out = append(out, fmt.Sprintf("%s/%s:%s:%d", h.TenantSlug, h.Tree, h.Path, h.Line))
	}
	return out
}

func q(kv ...string) url.Values {
	v := url.Values{}
	for i := 0; i+1 < len(kv); i += 2 {
		v.Set(kv[i], kv[i+1])
	}
	return v
}

func TestCodeSearchScopes(t *testing.T) {
	f := newCodeSearchFixture(t)
	cases := []struct {
		name, as string
		params   url.Values
		want     []string
		searched int
	}{
		{"owner, whole server", "alice", q("q", "needle"),
			[]string{"acme/priv:needle.txt:3", "acme/priv:src/deep/needle.go:1", "partner-org/pub:pub.txt:1"}, 3},
		{"anonymous sees public trees only", "", q("q", "needle"),
			[]string{"partner-org/pub:pub.txt:1"}, 2},
		{"outsider skips private trees", "olga", q("q", "needle"),
			[]string{"partner-org/pub:pub.txt:1"}, 2},
		{"tenant scope", "alice", q("q", "needle", "tenant", "acme"),
			[]string{"acme/priv:needle.txt:3", "acme/priv:src/deep/needle.go:1"}, 2},
		{"tree scope", "alice", q("q", "needle", "tree", "acme/priv"),
			[]string{"acme/priv:needle.txt:3", "acme/priv:src/deep/needle.go:1"}, 1},
		{"tree plus tenant", "alice", q("q", "needle", "tenant", "acme", "tree", "priv"),
			[]string{"acme/priv:needle.txt:3", "acme/priv:src/deep/needle.go:1"}, 1},
		{"case sensitive", "alice", q("q", "Needle", "case", "true"),
			[]string{"acme/priv:src/deep/needle.go:1"}, 3},
		{"path glob", "alice", q("q", "needle", "path", "*.go"),
			[]string{"acme/priv:src/deep/needle.go:1"}, 3},
		{"regex", "alice", q("q", `^the \w+$`, "regex", "true"),
			[]string{"acme/priv:needle.txt:3"}, 3},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			b := f.search(t, c.as, c.params)
			if got := hitLocs(b); !reflect.DeepEqual(got, c.want) {
				t.Fatalf("hits = %v, want %v", got, c.want)
			}
			if b.SearchedTrees != c.searched || b.Truncated {
				t.Fatalf("searched_trees = %d truncated = %v, want %d false", b.SearchedTrees, b.Truncated, c.searched)
			}
		})
	}
}

func TestCodeSearchReadRestrictions(t *testing.T) {
	f := newCodeSearchFixture(t)
	app := func(as string, kv ...string) []string {
		return hitLocs(f.search(t, as, q(append([]string{"tree", "acme/app"}, kv...)...)))
	}
	// Path-deny policy: bob may not read secret/**.
	eq := func(got, want []string) {
		t.Helper()
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("hits = %v, want %v", got, want)
		}
	}
	eq(app("alice", "q", "k2", "regex", "true"), []string{"acme/app:secret/key.txt:1"})
	eq(app("bob", "q", "k2", "regex", "true"), []string{})
	eq(app("olga", "q", "k2", "regex", "true"), []string{"acme/app:secret/key.txt:1"})
	// License: vendor/** is proprietary, closed to non-members.
	eq(app("bob", "q", "lib2"), []string{"acme/app:vendor/lib.c:1"})
	eq(app("olga", "q", "lib2"), []string{})
	eq(app("", "q", "lib2"), []string{})
	// Chunked files are not indexed.
	eq(app("alice", "q", "chunk-a"), []string{})
	// Unrestricted paths stay visible to everyone who may read the tree.
	eq(app("", "q", "package"), []string{"acme/app:src/a.go:1"})
}

func TestCodeSearchResponse(t *testing.T) {
	f := newCodeSearchFixture(t)
	b := f.search(t, "alice", q("q", "the needle", "tree", "acme/priv"))
	if len(b.Hits) != 1 {
		t.Fatalf("hits = %+v", b.Hits)
	}
	h := b.Hits[0]
	want := domain.CodeSearchHit{TenantSlug: "acme", Tree: "priv", Branch: "main", SnapshotID: "priv-1", Path: "needle.txt",
		Line: 3, Text: "the needle", Before: []string{"a", "b"}, After: []string{"c", "d"}}
	if !reflect.DeepEqual(h, want) {
		t.Fatalf("hit = %+v\nwant %+v", h, want)
	}
	b = f.search(t, "alice", q("q", "the needle", "tree", "acme/priv", "context", "0"))
	if b.Hits[0].Before != nil || b.Hits[0].After != nil {
		t.Fatalf("context=0 returned context: %+v", b.Hits[0])
	}
	b = f.search(t, "alice", q("q", "needle", "limit", "1"))
	if len(b.Hits) != 1 || !b.Truncated {
		t.Fatalf("limit=1: %+v", b)
	}

	// Raw JSON field names (§14.3).
	rec := f.do(t, http.MethodGet, "/api/v1/search/code?q=zzz-none", "alice", nil)
	wantStatus(t, rec, http.StatusOK, "")
	if got := rec.Body.String(); got != `{"hits":[],"truncated":false,"searched_trees":3}`+"\n" {
		t.Fatalf("body = %s", got)
	}

	// A push to the default branch is picked up by the next search.
	f.commit(t, f.priv, "priv-2", "priv-1", map[string]string{"needle.txt": "moved needle\n"})
	b = f.search(t, "alice", q("q", "needle", "tree", "acme/priv"))
	if got := hitLocs(b); !reflect.DeepEqual(got, []string{"acme/priv:needle.txt:1"}) || b.Hits[0].SnapshotID != "priv-2" {
		t.Fatalf("after push: %+v", b.Hits)
	}
	if m, ok := f.idx.Status(f.priv.ID); !ok || m.SnapshotID != "priv-2" {
		t.Fatalf("index status = %+v %v", m, ok)
	}
}

func TestCodeSearchErrors(t *testing.T) {
	f := newCodeSearchFixture(t)
	cases := []struct {
		name, as, query string
		status          int
		code            string
	}{
		{"missing q", "alice", "", 400, "bad_request"},
		{"short literal", "alice", "q=ab", 400, "bad_request"},
		{"bad regex", "alice", "q=(x&regex=true", 400, "bad_request"},
		{"bad bool", "alice", "q=abc&regex=maybe", 400, "bad_request"},
		{"bad case", "alice", "q=abc&case=2x", 400, "bad_request"},
		{"bad context", "alice", "q=abc&context=11", 400, "bad_request"},
		{"negative context", "alice", "q=abc&context=-1", 400, "bad_request"},
		{"bad limit", "alice", "q=abc&limit=0", 400, "bad_request"},
		{"bad glob", "alice", "q=abc&path=%5Bx", 400, "bad_request"},
		{"tree without tenant", "alice", "q=abc&tree=priv", 400, "bad_request"},
		{"tenant mismatch", "alice", "q=abc&tenant=acme&tree=partner-org/pub", 400, "bad_request"},
		{"unknown tenant", "alice", "q=abc&tenant=nope", 404, "not_found"},
		{"unknown tree", "alice", "q=abc&tree=acme/nope", 404, "not_found"},
		{"private tree, outsider", "olga", "q=abc&tree=acme/priv", 403, ""},
		{"private tree, anonymous", "", "q=abc&tree=acme/priv", 401, ""},
	}
	for _, c := range cases {
		t.Run(c.name, func(t *testing.T) {
			rec := f.do(t, http.MethodGet, "/api/v1/search/code?"+c.query, c.as, nil)
			wantStatus(t, rec, c.status, c.code)
		})
	}

	// Without Deps.CodeSearch the endpoint is not implemented.
	plain := newPathsFixture(t)
	rec := plain.do(t, http.MethodGet, "/api/v1/search/code?q=abc", "alice", nil)
	wantStatus(t, rec, http.StatusNotImplemented, "not_implemented")
}

// ---- search-v2 (§15.9): symbols, branches, index administration ----

// commitOn stores a snapshot with files as the new tip of branch.
func (f *codeSearchFixture) commitOn(t *testing.T, tree *domain.Tree, branch, id string, files map[string]string) {
	t.Helper()
	ctx := context.Background()
	old := ""
	if b, err := f.store.GetBranch(ctx, tree.ID, branch); err == nil {
		old = b.Tip
	}
	must(t, f.store.PutSnapshot(ctx, &domain.Snapshot{ID: id, TreeID: tree.ID, BranchName: branch, ManifestHash: f.manifest(t, files), Author: "alice", Message: id, CreatedAt: t0}))
	must(t, f.store.CompareAndSwapBranchTip(ctx, tree.ID, branch, old, id))
}

type symbolBody struct {
	Symbols       []codesearch.SymbolHit `json:"symbols"`
	Truncated     bool                   `json:"truncated"`
	SearchedTrees int                    `json:"searched_trees"`
}

func (f *codeSearchFixture) symbols(t *testing.T, as string, params url.Values) symbolBody {
	t.Helper()
	rec := f.do(t, http.MethodGet, "/api/v1/search/symbols?"+params.Encode(), as, nil)
	wantStatus(t, rec, http.StatusOK, "")
	return decode[symbolBody](t, rec)
}

func symLocs(b symbolBody) []string {
	out := []string{}
	for _, s := range b.Symbols {
		out = append(out, fmt.Sprintf("%s/%s@%s:%s:%d %s %s %s", s.TenantSlug, s.Tree, s.Branch, s.Path, s.Line, s.Kind, s.Name, s.Match))
	}
	return out
}

func TestSymbolSearchEndpoint(t *testing.T) {
	f := newCodeSearchFixture(t)
	f.commit(t, f.priv, "priv-2", "priv-1", map[string]string{
		"needle.txt":    "the needle\n",
		"src/needle.go": "package src\n\nfunc FindNeedle() {}\n\ntype Needle struct{}\n\nfunc (n *Needle) Sharpen() {}\n",
	})
	f.commitOn(t, f.tree, "main", "snap-p3", map[string]string{"src/a.go": "package a\n\nfunc NeedleApp() {}\n", "secret/key.go": "package secret\n\nfunc NeedleSecret() {}\n"})
	eq := func(got, want []string) {
		t.Helper()
		if !reflect.DeepEqual(got, want) {
			t.Fatalf("symbols = %v\nwant %v", got, want)
		}
	}

	// Ranking, kinds, container, signature.
	b := f.symbols(t, "alice", q("q", "needle", "tree", "acme/priv"))
	eq(symLocs(b), []string{"acme/priv@main:src/needle.go:5 struct Needle exact", "acme/priv@main:src/needle.go:3 function FindNeedle substring"})
	if b.Symbols[0].Signature != "type Needle struct{}" || b.Symbols[0].Language != "go" || b.SearchedTrees != 1 {
		t.Fatalf("symbol = %+v", b.Symbols[0])
	}
	b = f.symbols(t, "alice", q("q", "sharp", "tree", "acme/priv", "kind", "method"))
	if len(b.Symbols) != 1 || b.Symbols[0].Container != "Needle" || b.Symbols[0].Match != "prefix" {
		t.Fatalf("method = %+v", b.Symbols)
	}
	eq(symLocs(f.symbols(t, "alice", q("q", "needle", "tree", "acme/priv", "kind", "function,const"))),
		[]string{"acme/priv@main:src/needle.go:3 function FindNeedle substring"})

	// Read restrictions: bob may not read secret/** of acme/app; anonymous
	// callers only see public trees.
	eq(symLocs(f.symbols(t, "alice", q("q", "needle", "tree", "acme/app"))),
		[]string{"acme/app@main:src/a.go:3 function NeedleApp prefix", "acme/app@main:secret/key.go:3 function NeedleSecret prefix"})
	eq(symLocs(f.symbols(t, "bob", q("q", "needle", "tree", "acme/app"))), []string{"acme/app@main:src/a.go:3 function NeedleApp prefix"})
	for _, s := range f.symbols(t, "", q("q", "needle")).Symbols {
		if s.Tree == "priv" {
			t.Fatalf("anonymous caller saw a private symbol: %+v", s)
		}
	}

	// Raw JSON field names.
	rec := f.do(t, http.MethodGet, "/api/v1/search/symbols?q=zzznone&tree=acme/priv", "alice", nil)
	wantStatus(t, rec, http.StatusOK, "")
	if got := rec.Body.String(); got != `{"symbols":[],"truncated":false,"searched_trees":1}`+"\n" {
		t.Fatalf("body = %s", got)
	}

	for _, c := range []struct {
		query  string
		status int
		code   string
	}{
		{"", 400, "bad_request"},
		{"q=%20", 400, "bad_request"},
		{"q=x&kind=widget", 400, "bad_request"},
		{"q=x&branch=dev&branches=all", 400, "bad_request"},
		{"q=x&branches=some", 400, "bad_request"},
		{"q=x&limit=0", 400, "bad_request"},
		{"q=x&path=%5Bx", 400, "bad_request"},
		{"q=x&tree=acme/nope", 404, "not_found"},
	} {
		wantStatus(t, f.do(t, http.MethodGet, "/api/v1/search/symbols?"+c.query, "alice", nil), c.status, c.code)
	}
	wantStatus(t, f.do(t, http.MethodGet, "/api/v1/search/symbols?q=x&tree=acme/priv", "olga", nil), http.StatusForbidden, "")
	plain := newPathsFixture(t)
	wantStatus(t, plain.do(t, http.MethodGet, "/api/v1/search/symbols?q=abc", "alice", nil), http.StatusNotImplemented, "not_implemented")
}

func TestCodeSearchBranches(t *testing.T) {
	f := newCodeSearchFixture(t)
	f.commitOn(t, f.priv, "dev", "priv-dev", map[string]string{"needle.txt": "a\nb\nthe needle\nc\nd\ne\n", "dev.txt": "dev needle\n"})
	b := f.search(t, "alice", q("q", "needle", "tree", "acme/priv", "branch", "dev"))
	if got := hitLocs(b); !reflect.DeepEqual(got, []string{"acme/priv:dev.txt:1", "acme/priv:needle.txt:3"}) || b.Hits[0].Branch != "dev" {
		t.Fatalf("branch=dev: %+v", b.Hits)
	}
	b = f.search(t, "alice", q("q", "needle", "tree", "acme/priv", "branches", "all"))
	got := []string{}
	for _, h := range b.Hits {
		got = append(got, h.Branch+":"+h.Path)
	}
	// The unchanged needle.txt of dev is not reported again.
	if want := []string{"main:needle.txt", "main:src/deep/needle.go", "dev:dev.txt"}; !reflect.DeepEqual(got, want) {
		t.Fatalf("branches=all: %v, want %v", got, want)
	}
	wantStatus(t, f.do(t, http.MethodGet, "/api/v1/search/code?q=needle&branch=dev&branches=all", "alice", nil), 400, "bad_request")
	wantStatus(t, f.do(t, http.MethodGet, "/api/v1/search/code?q=needle&branches=every", "alice", nil), 400, "bad_request")
}

func TestCodeIndexAdminEndpoints(t *testing.T) {
	f := newCodeSearchFixture(t)
	f.search(t, "alice", q("q", "needle"))

	wantStatus(t, f.do(t, http.MethodGet, "/api/v1/admin/code-index", "alice", nil), http.StatusForbidden, "")
	wantStatus(t, f.do(t, http.MethodGet, "/api/v1/admin/code-index", "", nil), http.StatusUnauthorized, "")
	rec := f.do(t, http.MethodGet, "/api/v1/admin/code-index", "root", nil)
	wantStatus(t, rec, http.StatusOK, "")
	st := decode[codesearch.IndexStatus](t, rec)
	names := []string{}
	for _, tr := range st.Trees {
		names = append(names, tr.TenantSlug+"/"+tr.Tree)
	}
	if !reflect.DeepEqual(names, []string{"acme/app", "acme/priv", "partner-org/pub"}) || st.Builds != 3 || st.FormatVersion != codesearch.FormatVersion {
		t.Fatalf("status = %+v (%v)", st, names)
	}

	wantStatus(t, f.do(t, http.MethodPost, "/api/v1/admin/code-index/rebuild?tree=acme/priv", "bob", nil), http.StatusForbidden, "")
	rec = f.do(t, http.MethodPost, "/api/v1/admin/code-index/rebuild?tree=acme/priv", "root", nil)
	wantStatus(t, rec, http.StatusOK, "")
	rb := decode[codesearch.RebuildResult](t, rec)
	if rb.Rebuilt != 1 || len(rb.Trees) != 1 || rb.Trees[0].Tree != "priv" || rb.Trees[0].Files != 2 {
		t.Fatalf("rebuild = %+v", rb)
	}
	rec = f.do(t, http.MethodPost, "/api/v1/admin/code-index/rebuild", "root", nil)
	wantStatus(t, rec, http.StatusOK, "")
	if rb := decode[codesearch.RebuildResult](t, rec); rb.Rebuilt != 3 {
		t.Fatalf("rebuild all = %+v", rb)
	}
	wantStatus(t, f.do(t, http.MethodPost, "/api/v1/admin/code-index/rebuild?tree=priv", "root", nil), 400, "bad_request")
	wantStatus(t, f.do(t, http.MethodPost, "/api/v1/admin/code-index/rebuild?tree=acme/nope", "root", nil), 404, "not_found")
	plain := newPathsFixture(t)
	wantStatus(t, plain.do(t, http.MethodGet, "/api/v1/admin/code-index", "alice", nil), http.StatusForbidden, "")
}
