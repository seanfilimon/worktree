package httpapi

import (
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/mirror"
)

// newMirrorFixture wires a mirror.Service over depsStore (see
// todos_test.go) that may push to file:// remotes.
func newMirrorFixture(t *testing.T) *fixture {
	t.Helper()
	data := t.TempDir()
	return newFixture(t, func(d *Deps) {
		ds := &depsStore{manageStore: &manageStore{fakeStore: d.Store.(*fakeStore)}, DocStore: meta.NewMemory()}
		d.Store = ds
		svc := mirror.New(mirror.Deps{Store: ds, Objects: d.Objects, Authz: d.Authz, Bus: d.Bus, Audit: d.Audit,
			Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
		svc.Configure(mirror.Options{DataDir: data, AllowLocal: true, Debounce: time.Hour})
		d.Mirrors = svc
	})
}

func fileRemote(t *testing.T) (string, string) {
	t.Helper()
	p := filepath.Join(t.TempDir(), "remote.git")
	if out, err := exec.Command("git", "init", "--bare", "--quiet", p).CombinedOutput(); err != nil {
		t.Fatalf("git init: %v\n%s", err, out)
	}
	u := filepath.ToSlash(p)
	if runtime.GOOS == "windows" {
		return p, "file:///" + u
	}
	return p, "file://" + u
}

func gitOut(t *testing.T, dir string, args ...string) string {
	t.Helper()
	cmd := exec.Command("git", args...)
	cmd.Dir = dir
	cmd.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1")
	out, err := cmd.CombinedOutput()
	if err != nil {
		t.Fatalf("git %v: %v\n%s", args, err, out)
	}
	return strings.TrimSpace(string(out))
}

func TestMirrorEndpoints(t *testing.T) {
	if _, err := exec.LookPath("git"); err != nil {
		t.Skip("git is not installed")
	}
	f := newMirrorFixture(t)
	do := func(method, path, as string, body any, want int) []byte {
		t.Helper()
		rec := f.do(t, method, path, as, body)
		if rec.Code != want {
			t.Fatalf("%s %s as %q: %d, want %d; %s", method, path, as, rec.Code, want, rec.Body.String())
		}
		return rec.Body.Bytes()
	}
	const base = "/api/v1/trees/acme/web/mirrors"
	remote, url := fileRemote(t)

	l := decode[mirrorList](t, f.do(t, "GET", base, "alice", nil))
	if len(l.Mirrors) != 0 || !l.Git.Available || l.Git.Version == "" {
		t.Fatalf("empty list = %+v", l)
	}
	do("GET", base, "bob", nil, 403)
	do("GET", base, "", nil, 401)

	body := map[string]any{"url": url, "branches": []string{"main"}, "credential": "never-shown"}
	do("POST", base, "alice", body, 400) // credentials are for http(s) remotes
	delete(body, "credential")
	do("POST", base, "bob", body, 403)
	do("POST", base, "alice", map[string]any{"url": "gopher://x"}, 400)
	do("POST", base, "alice", map[string]any{"url": url, "bogus": 1}, 400)
	created := decode[domain.Mirror](t, f.do(t, "POST", base, "mallory", body))
	if created.ID == "" || created.URL != url || !created.Tags || !created.Active || created.CreatedBy != "mallory" {
		t.Fatalf("created = %+v", created)
	}
	do("POST", base, "alice", body, 409)
	got := decode[domain.Mirror](t, f.do(t, "GET", base+"/"+created.ID, "alice", nil))
	if got.ID != created.ID || len(got.Branches) != 1 {
		t.Fatalf("get = %+v", got)
	}
	do("GET", "/api/v1/trees/acme/docs/mirrors/"+created.ID, "alice", nil, 404)
	do("GET", base+"/missing", "alice", nil, 404)

	// Patch: also mirror the feature branch.
	patched := decode[domain.Mirror](t, f.do(t, "PATCH", base+"/"+created.ID, "alice", map[string]any{"branches": []string{"main", "feat*"}}))
	if len(patched.Branches) != 2 {
		t.Fatalf("patched = %+v", patched)
	}
	do("PATCH", base+"/"+created.ID, "alice", map[string]any{"branches": []string{"a b"}}, 400)
	do("PATCH", base+"/"+created.ID, "bob", map[string]any{"active": false}, 403)

	// Sync now: the fixture's history reaches the bare remote; the
	// proprietary read-only path is not redistributed.
	s := decode[mirrorSync](t, f.do(t, "POST", base+"/"+created.ID+"/sync", "alice", nil))
	if s.Mirror.LastStatus != mirror.StatusOK || s.Result == nil || !s.Result.Pushed || s.Result.Commits != 2 {
		t.Fatalf("sync = %+v / %+v", s.Mirror, s.Result)
	}
	refs := gitOut(t, remote, "for-each-ref", "--format=%(refname)")
	if refs != "refs/heads/feature\nrefs/heads/main\nrefs/tags/v1" {
		t.Fatalf("remote refs:\n%s", refs)
	}
	files := gitOut(t, remote, "ls-tree", "-r", "--name-only", "main")
	if strings.Contains(files, "secret/") || !strings.Contains(files, "src/main.go") || !strings.Contains(files, "LICENSES/NOTICE") {
		t.Fatalf("main files:\n%s", files)
	}
	if msg := gitOut(t, remote, "log", "--format=%s", "main"); msg != "second\nfirst" {
		t.Fatalf("log:\n%s", msg)
	}
	do("POST", base+"/"+created.ID+"/sync?wait=maybe", "alice", nil, 400)
	do("POST", base+"/"+created.ID+"/sync", "bob", nil, 403)

	// Inactive mirrors are not synced on demand.
	do("PATCH", base+"/"+created.ID, "alice", map[string]any{"active": false}, 200)
	do("POST", base+"/"+created.ID+"/sync", "alice", nil, 409)

	// Audit, then delete.
	do("DELETE", base+"/"+created.ID, "bob", nil, 403)
	do("DELETE", base+"/"+created.ID, "alice", nil, 204)
	do("DELETE", base+"/"+created.ID, "alice", nil, 404)
	actions := map[string]bool{}
	for _, e := range f.audit.events {
		actions[e.Action] = true
	}
	for _, a := range []string{"mirror.create", "mirror.update", "mirror.sync", "mirror.delete"} {
		if !actions[a] {
			t.Errorf("audit lacks %s (have %v)", a, actions)
		}
	}
	if strings.Contains(string(do("GET", base, "alice", nil, 200)), created.ID) {
		t.Error("deleted mirror still listed")
	}
}

func TestMirrorEndpointsUnavailable(t *testing.T) {
	f := newFixture(t)
	if rec := f.do(t, "GET", "/api/v1/trees/acme/web/mirrors", "alice", nil); rec.Code != 501 {
		t.Fatalf("GET without the service: %d %s", rec.Code, rec.Body.String())
	}
}
