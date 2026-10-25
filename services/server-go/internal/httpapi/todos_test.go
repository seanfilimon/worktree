package httpapi

import (
	"context"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/release"
	"github.com/seanfilimon/worktree/services/server-go/internal/todo"
)

// ---- fakes specific to the TODO and release endpoints (§13.5) ----

// depsStore adds a real DocStore (meta.Memory), tags (manageStore) and
// GetTree to the shared fake store.
type depsStore struct {
	*manageStore
	domain.DocStore
}

func (s *depsStore) GetTree(_ context.Context, id string) (*domain.Tree, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.trees[id]
	if !ok {
		return nil, nf("tree")
	}
	return &t, nil
}

// newDepsFixture is the shared fixture with Todos and Releases wired over
// depsStore (web: main = s1 <- s2, tag v1 at s1).
func newDepsFixture(t *testing.T) *fixture {
	t.Helper()
	return newFixture(t, func(d *Deps) {
		ds := &depsStore{manageStore: &manageStore{fakeStore: d.Store.(*fakeStore)}, DocStore: meta.NewMemory()}
		d.Store = ds
		log := slog.New(slog.NewTextHandler(io.Discard, nil))
		d.Todos = todo.New(todo.Deps{Store: ds, Objects: d.Objects, History: d.History, Authz: d.Authz, Bus: d.Bus, Audit: d.Audit, Logger: log})
		d.Releases = release.New(release.Deps{Store: ds, Objects: d.Objects, History: d.History, Authz: d.Authz, Bus: d.Bus, Audit: d.Audit, Logger: log})
	})
}

const depsWebAPI = "/api/v1/trees/acme/web"

func TestTodoEndpoints(t *testing.T) {
	f := newDepsFixture(t)
	do := func(method, path, as string, body any, want int) *httptest.ResponseRecorder {
		t.Helper()
		rec := f.do(t, method, path, as, body)
		if rec.Code != want {
			t.Fatalf("%s %s as %q: %d, want %d; %s", method, path, as, rec.Code, want, rec.Body.String())
		}
		return rec
	}

	created := decode[domain.Todo](t, do("POST", depsWebAPI+"/todos", "bob", map[string]any{"title": "Write docs", "priority": "high", "blocking": true}, 201))
	if created.Number != 1 || created.Author != "bob" || created.Priority != "high" || !created.Blocking || created.TargetBranch != "main" {
		t.Fatalf("created = %+v", created)
	}
	do("POST", depsWebAPI+"/todos", "bob", map[string]any{"title": "Second", "target_branch": "feature"}, 201)
	do("POST", depsWebAPI+"/todos", "carol", map[string]any{"title": "x"}, 403)
	do("POST", depsWebAPI+"/todos", "", map[string]any{"title": "x"}, 401)
	if rec := do("POST", depsWebAPI+"/todos", "bob", map[string]any{"title": ""}, 400); errCode(rec) != codeBadRequest {
		t.Fatalf("code = %s", errCode(rec))
	}
	do("POST", depsWebAPI+"/todos", "bob", map[string]any{"title": "x", "bogus": 1}, 400)

	list := decode[todoList](t, do("GET", depsWebAPI+"/todos", "carol", nil, 200))
	if len(list.Todos) != 2 {
		t.Fatalf("list = %+v", list)
	}
	do("GET", depsWebAPI+"/todos", "dave", nil, 403)
	do("GET", depsWebAPI+"/todos?status=nope", "carol", nil, 400)
	if l := decode[todoList](t, do("GET", depsWebAPI+"/todos?branch=feature", "carol", nil, 200)); len(l.Todos) != 1 || l.Todos[0].Number != 2 {
		t.Fatalf("branch filter = %+v", l)
	}

	got := decode[domain.Todo](t, do("GET", depsWebAPI+"/todos/1", "carol", nil, 200))
	if got.Title != "Write docs" {
		t.Fatalf("get = %+v", got)
	}
	if rec := do("GET", depsWebAPI+"/todos/9", "carol", nil, 404); !strings.Contains(rec.Body.String(), "todo #9 not found") {
		t.Fatalf("404 body = %s", rec.Body.String())
	}
	do("GET", depsWebAPI+"/todos/x", "carol", nil, 400)
	do("GET", depsWebAPI+"/todos/0", "carol", nil, 400)

	claimed := decode[domain.Todo](t, do("POST", depsWebAPI+"/todos/1/claim", "bob", nil, 200))
	if claimed.Assignee != "bob" || claimed.Status != domain.TodoInProgress {
		t.Fatalf("claimed = %+v", claimed)
	}
	do("POST", depsWebAPI+"/todos/1/claim", "carol", nil, 403)
	if l := decode[todoList](t, do("GET", depsWebAPI+"/todos?assignee=me&status=active", "bob", nil, 200)); len(l.Todos) != 1 {
		t.Fatalf("mine = %+v", l)
	}
	do("GET", depsWebAPI+"/todos?assignee=me", "", nil, 401) // private tree, anonymous

	title := "Write better docs"
	patched := decode[domain.Todo](t, do("PATCH", depsWebAPI+"/todos/1", "bob", map[string]any{"title": title, "priority": "low"}, 200))
	if patched.Title != title || patched.Priority != "low" {
		t.Fatalf("patched = %+v", patched)
	}
	do("PATCH", depsWebAPI+"/todos/1", "bob", map[string]any{"status": "weird"}, 400)

	// complete with a snapshot revision (branch name resolves to its tip).
	done := decode[domain.Todo](t, do("POST", depsWebAPI+"/todos/1/complete", "bob", map[string]any{"snapshot": "main"}, 200))
	if done.Status != domain.TodoDone || done.ResolvedBy != f.s2.ID {
		t.Fatalf("done = %+v", done)
	}
	do("POST", depsWebAPI+"/todos/1/complete", "bob", map[string]any{"snapshot": "no-such-rev"}, 404)
	do("POST", depsWebAPI+"/todos/1/cancel", "bob", nil, 409)
	// complete without a body.
	do("POST", depsWebAPI+"/todos/2/complete", "bob", nil, 200)
	do("PATCH", depsWebAPI+"/todos/2", "bob", map[string]any{"status": "open"}, 200)
	cancelled := decode[domain.Todo](t, do("POST", depsWebAPI+"/todos/2/cancel", "bob", nil, 200))
	if cancelled.Status != domain.TodoCancelled {
		t.Fatalf("cancelled = %+v", cancelled)
	}
	for _, a := range []string{"todo.create", "todo.claim", "todo.update", "todo.complete", "todo.cancel"} {
		hasAudit(t, f, a)
	}

	// Dependency report.
	rep := decode[todo.Report](t, do("GET", depsWebAPI+"/dependencies", "carol", nil, 200))
	if rep.Branch != "main" || rep.Outgoing == nil || rep.Incoming == nil || rep.Trees[f.web.ID] != "acme/web" {
		t.Fatalf("report = %+v", rep)
	}
	do("GET", depsWebAPI+"/dependencies", "dave", nil, 403)
}

func TestTodoAndReleaseEndpointsWithoutServices(t *testing.T) {
	f := newFixture(t)
	for _, c := range []struct{ method, path string }{
		{"GET", depsWebAPI + "/todos"},
		{"POST", depsWebAPI + "/todos/1/claim"},
		{"GET", depsWebAPI + "/dependencies"},
		{"GET", depsWebAPI + "/releases"},
		{"PUT", depsWebAPI + "/releases/v1/assets/a.txt"},
	} {
		rec := f.do(t, c.method, c.path, "alice", nil)
		if rec.Code != http.StatusNotImplemented || errCode(rec) != "not_implemented" {
			t.Fatalf("%s %s: %d %s", c.method, c.path, rec.Code, rec.Body.String())
		}
	}
}

func TestReleaseEndpoints(t *testing.T) {
	f := newDepsFixture(t)
	do := func(method, path, as string, body any, want int) *httptest.ResponseRecorder {
		t.Helper()
		rec := f.do(t, method, path, as, body)
		if rec.Code != want {
			t.Fatalf("%s %s as %q: %d, want %d; %s", method, path, as, rec.Code, want, rec.Body.String())
		}
		return rec
	}

	r1 := decode[domain.Release](t, do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "v1", "notes": "First"}, 201))
	if r1.SnapshotID != f.s1.ID || r1.Name != "v1" || r1.Author != "alice" {
		t.Fatalf("r1 = %+v", r1)
	}
	hasAudit(t, f, "release.create")
	// New tag with '/' at a revision.
	r2 := decode[domain.Release](t, do("POST", depsWebAPI+"/releases", "alice",
		map[string]any{"tag": "release/2.0", "snapshot": "main", "draft": true, "prerelease": true}, 201))
	if r2.SnapshotID != f.s2.ID || !r2.Draft {
		t.Fatalf("r2 = %+v", r2)
	}
	do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "v1"}, 409)
	do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "v9"}, 400)
	do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "bad tag", "snapshot": "main"}, 400)
	do("POST", depsWebAPI+"/releases", "alice", map[string]any{"tag": "v3", "snapshot": "nope"}, 404)
	do("POST", depsWebAPI+"/releases", "carol", map[string]any{"tag": "v4", "snapshot": "main"}, 403)
	do("POST", depsWebAPI+"/releases", "", map[string]any{"tag": "v4", "snapshot": "main"}, 401)

	// Drafts are visible to release:create holders only.
	if l := decode[releaseList](t, do("GET", depsWebAPI+"/releases", "alice", nil, 200)); len(l.Releases) != 2 {
		t.Fatalf("alice list = %+v", l)
	}
	if l := decode[releaseList](t, do("GET", depsWebAPI+"/releases", "carol", nil, 200)); len(l.Releases) != 1 || l.Releases[0].Tag != "v1" {
		t.Fatalf("carol list = %+v", l)
	}
	do("GET", depsWebAPI+"/releases/release%2F2.0", "carol", nil, 404)
	if r := decode[domain.Release](t, do("GET", depsWebAPI+"/releases/release%2F2.0", "alice", nil, 200)); r.Tag != "release/2.0" {
		t.Fatalf("encoded tag = %+v", r)
	}
	do("GET", depsWebAPI+"/releases", "dave", nil, 403)

	pub := decode[domain.Release](t, do("PATCH", depsWebAPI+"/releases/release%2F2.0", "alice", map[string]any{"draft": false, "name": "Two"}, 200))
	if pub.Draft || pub.PublishedAt == nil || pub.Name != "Two" {
		t.Fatalf("published = %+v", pub)
	}
	do("PATCH", depsWebAPI+"/releases/v1", "carol", map[string]any{"name": "x"}, 403)
	do("PATCH", depsWebAPI+"/releases/nope", "alice", map[string]any{"name": "x"}, 404)

	// Assets.
	req := httptest.NewRequest("PUT", depsWebAPI+"/releases/v1/assets/app.zip", strings.NewReader("PK-zip-bytes"))
	req.Header.Set("Authorization", "Bearer alice")
	req.Header.Set("Content-Type", "application/zip")
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	if rec.Code != 201 {
		t.Fatalf("upload: %d %s", rec.Code, rec.Body.String())
	}
	asset := decode[domain.ReleaseAsset](t, rec)
	if asset.Name != "app.zip" || asset.Size != 12 || asset.ContentType != "application/zip" {
		t.Fatalf("asset = %+v", asset)
	}
	hasAudit(t, f, "release.asset.upload")

	upload := func(path, as, body string) int {
		req := httptest.NewRequest("PUT", path, strings.NewReader(body))
		if as != "" {
			req.Header.Set("Authorization", "Bearer "+as)
		}
		rec := httptest.NewRecorder()
		f.h.ServeHTTP(rec, req)
		return rec.Code
	}
	if c := upload(depsWebAPI+"/releases/v1/assets/x.bin", "carol", "x"); c != 403 {
		t.Fatalf("viewer upload = %d", c)
	}
	if c := upload(depsWebAPI+"/releases/nope/assets/x.bin", "alice", "x"); c != 404 {
		t.Fatalf("missing release upload = %d", c)
	}
	if c := upload(depsWebAPI+"/releases/v1/assets/..", "alice", "x"); c != 400 && c != 404 && c != 301 && c != 307 {
		t.Fatalf("dot-dot upload = %d", c)
	}
	big := httptest.NewRequest("PUT", depsWebAPI+"/releases/v1/assets/big.bin", strings.NewReader("x"))
	big.ContentLength = release.MaxAssetSize + 1
	big.Header.Set("Authorization", "Bearer alice")
	rec = httptest.NewRecorder()
	f.h.ServeHTTP(rec, big)
	if rec.Code != http.StatusRequestEntityTooLarge || errCode(rec) != "payload_too_large" {
		t.Fatalf("too large: %d %s", rec.Code, rec.Body.String())
	}

	rec = do("GET", depsWebAPI+"/releases/v1/assets/app.zip", "carol", nil, 200)
	if rec.Body.String() != "PK-zip-bytes" || rec.Header().Get("Content-Type") != "application/zip" ||
		rec.Header().Get("Content-Disposition") != `attachment; filename=app.zip` || rec.Header().Get("ETag") == "" {
		t.Fatalf("download: %v %q", rec.Header(), rec.Body.String())
	}
	dl := httptest.NewRequest("GET", depsWebAPI+"/releases/v1/assets/app.zip", nil)
	dl.Header.Set("Authorization", "Bearer carol")
	dl.Header.Set("If-None-Match", rec.Header().Get("ETag"))
	rec = httptest.NewRecorder()
	f.h.ServeHTTP(rec, dl)
	if rec.Code != http.StatusNotModified {
		t.Fatalf("conditional download = %d", rec.Code)
	}
	do("GET", depsWebAPI+"/releases/v1/assets/none.zip", "carol", nil, 404)

	do("DELETE", depsWebAPI+"/releases/v1/assets/app.zip", "carol", nil, 403)
	do("DELETE", depsWebAPI+"/releases/v1/assets/app.zip", "alice", nil, 204)
	do("DELETE", depsWebAPI+"/releases/v1/assets/app.zip", "alice", nil, 404)
	do("DELETE", depsWebAPI+"/releases/v1", "bob", nil, 403)
	do("DELETE", depsWebAPI+"/releases/v1", "alice", nil, 204)
	do("GET", depsWebAPI+"/releases/v1", "alice", nil, 404)
	hasAudit(t, f, "release.delete")
	hasAudit(t, f, "release.asset.delete")
}
