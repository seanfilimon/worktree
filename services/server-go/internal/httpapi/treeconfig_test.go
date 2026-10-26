package httpapi

import (
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/treeconfig"
)

// newTreeConfigFixture wires a treeconfig.Service over depsStore (see
// todos_test.go).
func newTreeConfigFixture(t *testing.T) (*fixture, *depsStore) {
	t.Helper()
	var ds *depsStore
	f := newFixture(t, func(d *Deps) {
		ds = &depsStore{manageStore: &manageStore{fakeStore: d.Store.(*fakeStore)}, DocStore: meta.NewMemory()}
		d.Store = ds
		d.TreeConfig = treeconfig.New(treeconfig.Deps{Store: ds, Objects: d.Objects, Authz: d.Authz, Bus: d.Bus, Audit: d.Audit,
			Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	})
	return f, ds
}

func TestTreeConfigRoutes(t *testing.T) {
	f, ds := newTreeConfigFixture(t)
	base := "/api/v1/trees/acme/web"

	// No configuration yet.
	rec := f.do(t, "GET", base+"/config", "carol", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("GET config = %d %s", rec.Code, rec.Body)
	}
	var st treeconfig.State
	must(t, json.Unmarshal(rec.Body.Bytes(), &st))
	if st.TreeID != "tree-web" || st.DefaultBranch != "main" || st.Applied != nil || st.LastAttempt != nil {
		t.Errorf("state = %+v", st)
	}
	if !strings.Contains(rec.Body.String(), `"applied":null`) {
		t.Errorf("body = %s", rec.Body)
	}
	if rec := f.do(t, "GET", base+"/config", "dave", nil); rec.Code != http.StatusNotFound && rec.Code != http.StatusForbidden {
		t.Errorf("outsider GET config = %d", rec.Code)
	}

	// Validate.
	body := map[string]any{"files": map[string]string{
		".wt-tree/branches.toml":    "[[branch]]\nname = \"main\"\nowner = \"user:bob\"\n\n[[branch]]\nname = \"x y\"\n",
		".wt-tree/protections.toml": "[[protection]]\npattern = \"main\"\nrequired_approvals = 1\n",
	}}
	rec = f.do(t, "POST", base+"/config/validate", "bob", body)
	if rec.Code != http.StatusOK {
		t.Fatalf("validate = %d %s", rec.Code, rec.Body)
	}
	var res treeconfig.ValidationResult
	must(t, json.Unmarshal(rec.Body.Bytes(), &res))
	if res.Valid || len(res.Diagnostics) != 1 || res.Diagnostics[0].Code != treeconfig.EBranchName || res.Diagnostics[0].Line != 6 {
		t.Errorf("result = %+v", res)
	}
	if len(res.Protections) != 1 || res.Protections[0].RequiredApprovals != 1 || len(res.Branches) != 1 {
		t.Errorf("result = %+v", res)
	}
	for _, c := range []struct {
		as   string
		body any
		want int
	}{
		{"bob", map[string]any{"files": map[string]string{"README.md": "x"}}, http.StatusBadRequest},
		{"bob", map[string]any{"files": map[string]string{}}, http.StatusBadRequest},
		{"bob", `{"nope":1}`, http.StatusBadRequest},
		{"carol", body, http.StatusForbidden},
		{"", body, http.StatusUnauthorized},
	} {
		if rec := f.do(t, "POST", base+"/config/validate", c.as, c.body); rec.Code != c.want {
			t.Errorf("validate as %q %v = %d, want %d (%s)", c.as, c.body, rec.Code, c.want, rec.Body)
		}
	}

	// PATCH branch metadata.
	rec = f.do(t, "PATCH", base+"/branches/feature", "bob", map[string]any{"description": "  Work in progress  "})
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"description":"Work in progress"`) {
		t.Fatalf("patch description = %d %s", rec.Code, rec.Body)
	}
	if rec := f.do(t, "PATCH", base+"/branches/feature", "bob", map[string]any{"owner": "user:bob"}); rec.Code != http.StatusForbidden {
		t.Errorf("developer sets owner = %d", rec.Code)
	}
	rec = f.do(t, "PATCH", base+"/branches/feature", "mallory", map[string]any{"owner": "user:bob"})
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"owner":"user:bob"`) || !strings.Contains(rec.Body.String(), `"description":"Work in progress"`) {
		t.Fatalf("patch owner = %d %s", rec.Code, rec.Body)
	}
	b, err := ds.GetBranch(context.Background(), "tree-web", "feature")
	must(t, err)
	if b.Tip != "snap-2" || b.Owner != "user:bob" {
		t.Errorf("branch = %+v", b)
	}
	for _, c := range []struct {
		path string
		body any
		want int
	}{
		{"/branches/feature", map[string]any{"owner": "user:ghost"}, http.StatusBadRequest},
		{"/branches/feature", map[string]any{"owner": "nobody"}, http.StatusBadRequest},
		{"/branches/feature", map[string]any{}, http.StatusBadRequest},
		{"/branches/feature", map[string]any{"colour": "red"}, http.StatusBadRequest},
		{"/branches/missing", map[string]any{"description": "x"}, http.StatusNotFound},
	} {
		if rec := f.do(t, "PATCH", base+c.path, "mallory", c.body); rec.Code != c.want {
			t.Errorf("PATCH %s %v = %d, want %d (%s)", c.path, c.body, rec.Code, c.want, rec.Body)
		}
	}
	if rec := f.do(t, "PATCH", base+"/branches/feature", "carol", map[string]any{"description": "x"}); rec.Code != http.StatusForbidden {
		t.Errorf("viewer PATCH = %d", rec.Code)
	}

	// A branch declared in the applied branches.toml is config-managed.
	must(t, ds.PutDoc(context.Background(), treeconfig.Collection, "tree-web", treeconfig.State{
		TreeID: "tree-web", Applied: &treeconfig.Applied{SnapshotID: "snap-2", Branch: "main", Branches: []treeconfig.BranchDecl{{Name: "main", Description: "Prod"}}},
	}))
	rec = f.do(t, "PATCH", base+"/branches/main", "mallory", map[string]any{"description": "manual"})
	if rec.Code != http.StatusConflict || !strings.Contains(rec.Body.String(), "config_managed") {
		t.Errorf("patch managed = %d %s", rec.Code, rec.Body)
	}
	rec = f.do(t, "GET", base+"/config", "carol", nil)
	if rec.Code != http.StatusOK || !strings.Contains(rec.Body.String(), `"snapshot_id":"snap-2"`) {
		t.Errorf("GET config = %d %s", rec.Code, rec.Body)
	}
	found := false
	f.audit.mu.Lock()
	defer f.audit.mu.Unlock()
	for _, e := range f.audit.events {
		found = found || e.Action == "branch.update"
	}
	if !found {
		t.Error("branch.update not audited")
	}
}

func TestTreeConfigRoutesWithoutService(t *testing.T) {
	f := newFixture(t)
	for _, c := range []struct{ method, path string }{
		{"GET", "/config"}, {"POST", "/config/validate"}, {"PATCH", "/branches/main"},
	} {
		var body any
		if c.method != "GET" {
			body = map[string]any{"files": map[string]string{".wt-tree/config.toml": ""}, "description": "x"}
			if c.method == "PATCH" {
				body = map[string]any{"description": "x"}
			}
		}
		if rec := f.do(t, c.method, "/api/v1/trees/acme/web"+c.path, "alice", body); rec.Code != http.StatusNotImplemented {
			t.Errorf("%s %s = %d %s", c.method, c.path, rec.Code, rec.Body)
		}
	}
}
