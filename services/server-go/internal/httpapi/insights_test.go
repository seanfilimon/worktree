package httpapi

import (
	"context"
	"encoding/json"
	"fmt"
	"net/http/httptest"
	"slices"
	"strings"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// ---- test doubles specific to the insight endpoints ----

// insHistory extends fakeHistory with real ancestry answers.
type insHistory struct{ *fakeHistory }

func (h insHistory) ancestors(ctx context.Context, ids ...string) (map[string]bool, error) {
	set := map[string]bool{}
	for _, id := range ids {
		log, err := h.Log(ctx, id, 0)
		if err != nil {
			return nil, err
		}
		for _, s := range log {
			set[s.ID] = true
		}
	}
	return set, nil
}

func (h insHistory) IsAncestor(ctx context.Context, anc, desc string) (bool, error) {
	set, err := h.ancestors(ctx, desc)
	return set[anc], err
}

func (h insHistory) Missing(ctx context.Context, tip string, haves []string) ([]domain.Snapshot, error) {
	have, err := h.ancestors(ctx, haves...)
	if err != nil {
		return nil, err
	}
	log, err := h.Log(ctx, tip, 0)
	if err != nil {
		return nil, err
	}
	out := []domain.Snapshot{}
	for i := len(log) - 1; i >= 0; i-- { // oldest first
		if !have[log[i].ID] {
			out = append(out, log[i])
		}
	}
	return out, nil
}

// insSummaryStaged is a StagedService that also implements Summary.
type insSummaryStaged struct {
	*fakeStaged
	acts []staged.BranchActivity
	err  error
}

func (s *insSummaryStaged) Summary(context.Context, *domain.Principal, *domain.Tree) ([]staged.BranchActivity, error) {
	return s.acts, s.err
}

// insRecentBus is an EventBus with a retained history.
type insRecentBus struct {
	*fakeBus
	events []domain.Event
}

func (b *insRecentBus) Recent(f domain.EventFilter, n int) []domain.Event {
	var out []domain.Event
	for _, e := range b.events {
		if (f.TenantID == "" || f.TenantID == e.TenantID) && (f.TreeID == "" || f.TreeID == e.TreeID) &&
			(len(f.Types) == 0 || slices.Contains(f.Types, e.Type)) {
			out = append(out, e)
		}
	}
	if n > 0 && len(out) > n {
		out = out[len(out)-n:]
	}
	return append([]domain.Event{}, out...)
}

type insFile struct {
	content string
	exec    bool
}

func insManifest(t testing.TB, objs *fakeObjects, files map[string]insFile) hash.Hash {
	t.Helper()
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	slices.Sort(paths)
	var m syncpb.ManifestRecord
	for _, p := range paths {
		h := objs.put([]byte(files[p].content))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{
			Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(files[p].content)), Executable: files[p].exec,
		})
	}
	raw, err := proto.Marshal(&m)
	if err != nil {
		t.Fatal(err)
	}
	return objs.put(raw)
}

// insS2Files are the files of the base fixture's snap-2.
func insS2Files() map[string]insFile {
	return map[string]insFile{
		"README.md":      {content: "# web v2\n"},
		"src/main.go":    {content: "package main\n\nfunc main() {}\n"},
		"secret/key.txt": {content: "hunter2"},
		"index.html":     {content: "<html><script>alert(1)</script></html>"},
		"LICENSE":        {content: "Permission is hereby granted, free of charge..."},
		"run.sh":         {content: "#!/bin/sh\necho hi\n", exec: true},
	}
}

func insAddSnap(t testing.TB, f *fixture, id, branch, author string, at time.Time, files map[string]insFile, parents ...string) domain.Snapshot {
	t.Helper()
	s := domain.Snapshot{ID: id, TreeID: f.web.ID, BranchName: branch, Parents: parents,
		ManifestHash: insManifest(t, f.objs, files), Author: author, Message: id, CreatedAt: at}
	f.store.mu.Lock()
	f.store.snapshots[id] = s
	f.store.mu.Unlock()
	must(t, f.store.PutBranch(context.Background(), &domain.Branch{TreeID: f.web.ID, Name: branch, Tip: id, CreatedAt: at, UpdatedAt: at}))
	return s
}

// bigText is just over the 1 MiB diff limit.
var bigText = strings.Repeat("x\n", maxDiffFileSize/2+1)

// newInsightFixture extends the base fixture (main: snap-1 <- snap-2) with
//
//	topic: snap-2 <- snap-3 (bob: edits src/main.go, deletes README.md) <- snap-4 (bob: re-adds README.md)
//	dev:   snap-2 <- abcdef0123 (carol, a day later: adds img.png (binary) and big.txt (> 1 MiB), run.sh loses +x)
func newInsightFixture(t testing.TB, mutate ...func(*Deps)) *fixture {
	t.Helper()
	f := newFixture(t, append([]func(*Deps){func(d *Deps) {
		d.History = insHistory{&fakeHistory{store: d.Store.(*fakeStore)}}
	}}, mutate...)...)
	s3 := insS2Files()
	delete(s3, "README.md")
	s3["src/main.go"] = insFile{content: "package main\n\nfunc main() { println(1) }\n"}
	insAddSnap(t, f, "snap-3", "topic", "bob", t0.Add(2*time.Hour), s3, "snap-2")
	s4 := insS2Files()
	s4["src/main.go"] = s3["src/main.go"]
	s4["README.md"] = insFile{content: "# web v3\n"}
	insAddSnap(t, f, "snap-4", "topic", "bob", t0.Add(3*time.Hour), s4, "snap-3")
	s5 := insS2Files()
	s5["img.png"] = insFile{content: "\x89PNG\r\n\x1a\n\x00\x00"}
	s5["big.txt"] = insFile{content: bigText}
	s5["run.sh"] = insFile{content: s5["run.sh"].content}
	insAddSnap(t, f, "abcdef0123", "dev", "carol", t0.Add(26*time.Hour), s5, "snap-2")
	return f
}

func insRunCases(t *testing.T, cases []endpointCase, mutate ...func(*Deps)) {
	t.Helper()
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			f := newInsightFixture(t, mutate...)
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
			if rec.Code >= 400 && errCode(rec) == "" {
				t.Fatalf("error response without envelope: %s", rec.Body.String())
			}
			if tc.check != nil {
				tc.check(t, f, rec)
			}
		})
	}
}

func insFileByPath(t *testing.T, res compareResult, p string) fileDiff {
	t.Helper()
	for _, f := range res.Files {
		if f.Path == p {
			return f
		}
	}
	t.Fatalf("file %q not in compare result %+v", p, res.Files)
	return fileDiff{}
}

func insIDs(snaps []domain.Snapshot) []string {
	ids := make([]string, len(snaps))
	for i, s := range snaps {
		ids[i] = s.ID
	}
	return ids
}

// ---- compare ----

const webAPI = "/api/v1/trees/acme/web"

func TestCompare(t *testing.T) {
	insRunCases(t, []endpointCase{
		{
			name: "default from is first parent", method: "GET", path: webAPI + "/compare?to=main", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				res := decode[compareResult](t, rec)
				if res.From == nil || res.From.ID != "snap-1" || res.To.ID != "snap-2" {
					t.Fatalf("from/to = %v/%s", res.From, res.To.ID)
				}
				if res.MergeBase != "" {
					t.Fatalf("merge_base = %q for a fast-forward", res.MergeBase)
				}
				if got := insIDs(res.Snapshots); !slices.Equal(got, []string{"snap-2"}) {
					t.Fatalf("snapshots = %v", got)
				}
				paths := make([]string, len(res.Files))
				for i, fd := range res.Files {
					paths[i] = fd.Path
				}
				want := []string{"LICENSE", "README.md", "index.html", "run.sh", "secret/key.txt", "src/main.go"}
				if !slices.Equal(paths, want) {
					t.Fatalf("paths = %v, want %v", paths, want)
				}
				readme := insFileByPath(t, res, "README.md")
				wantPatch := "--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-# web\n+# web v2\n"
				if readme.Status != "modified" || readme.Patch != wantPatch || readme.Additions != 1 || readme.Deletions != 1 {
					t.Fatalf("README.md = %+v", readme)
				}
				if readme.OldHash != hash.Sum([]byte("# web\n")).Hex() || readme.NewHash != hash.Sum([]byte("# web v2\n")).Hex() || readme.OldSize != 6 || readme.NewSize != 9 {
					t.Fatalf("README.md hashes/sizes = %+v", readme)
				}
				main := insFileByPath(t, res, "src/main.go")
				if main.Status != "added" || main.OldHash != "" || !strings.HasPrefix(main.Patch, "--- /dev/null\n+++ b/src/main.go\n@@ -0,0 +1,3 @@\n") || main.Additions != 3 {
					t.Fatalf("src/main.go = %+v", main)
				}
				secret := insFileByPath(t, res, "secret/key.txt")
				if secret.Patch != "" || secret.Additions != 0 || secret.NewHash == "" || secret.Status != "added" {
					t.Fatalf("license-restricted file leaked content: %+v", secret)
				}
				// 1 (LICENSE) + 1 (README) + 1 (index.html) + 2 (run.sh) + 3 (main.go)
				if res.Stats != (compareStats{Files: 6, Additions: 8, Deletions: 1}) {
					t.Fatalf("stats = %+v", res.Stats)
				}
			},
		},
		{
			name: "root snapshot via tag", method: "GET", path: webAPI + "/compare?to=v1", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if !strings.Contains(rec.Body.String(), `"from":null`) {
					t.Fatalf("from not null: %s", rec.Body.String())
				}
				res := decode[compareResult](t, rec)
				if len(res.Files) != 1 || res.Files[0].Status != "added" || res.Files[0].Patch != "--- /dev/null\n+++ b/README.md\n@@ -0,0 +1 @@\n+# web\n" {
					t.Fatalf("files = %+v", res.Files)
				}
				if got := insIDs(res.Snapshots); !slices.Equal(got, []string{"snap-1"}) {
					t.Fatalf("snapshots = %v", got)
				}
				if res.To.Parents == nil {
					t.Fatal("parents encoded as null")
				}
			},
		},
		{
			name: "binary, too large and mode-only change", method: "GET", path: webAPI + "/compare?to=dev", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				res := decode[compareResult](t, rec)
				if len(res.Files) != 3 {
					t.Fatalf("files = %+v", res.Files)
				}
				if img := insFileByPath(t, res, "img.png"); !img.Binary || img.Patch != "" || img.TooLarge {
					t.Fatalf("img.png = %+v", img)
				}
				if big := insFileByPath(t, res, "big.txt"); !big.TooLarge || big.Patch != "" || big.NewSize != uint64(len(bigText)) {
					t.Fatalf("big.txt = %+v", big)
				}
				if sh := insFileByPath(t, res, "run.sh"); sh.Status != "modified" || sh.OldHash != sh.NewHash || sh.Patch != "" {
					t.Fatalf("run.sh = %+v", sh)
				}
			},
		},
		{
			name: "diverged branches report merge base", method: "GET", path: webAPI + "/compare?from=topic&to=dev", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				res := decode[compareResult](t, rec)
				if res.MergeBase != "snap-2" {
					t.Fatalf("merge_base = %q", res.MergeBase)
				}
				if got := insIDs(res.Snapshots); !slices.Equal(got, []string{"abcdef0123"}) {
					t.Fatalf("snapshots = %v", got)
				}
				if readme := insFileByPath(t, res, "README.md"); readme.Patch != "--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-# web v3\n+# web v2\n" {
					t.Fatalf("README.md = %+v", readme)
				}
			},
		},
		{
			name: "snapshots newest first", method: "GET", path: webAPI + "/compare?from=dev&to=topic", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				res := decode[compareResult](t, rec)
				if got := insIDs(res.Snapshots); !slices.Equal(got, []string{"snap-4", "snap-3"}) {
					t.Fatalf("snapshots = %v", got)
				}
				if res.MergeBase != "snap-2" {
					t.Fatalf("merge_base = %q", res.MergeBase)
				}
			},
		},
		{
			name: "backwards compare", method: "GET", path: webAPI + "/compare?from=snap-2&to=snap-1", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				res := decode[compareResult](t, rec)
				if res.MergeBase != "snap-1" || len(res.Snapshots) != 0 {
					t.Fatalf("merge_base = %q, snapshots = %v", res.MergeBase, insIDs(res.Snapshots))
				}
				for _, fd := range res.Files {
					if fd.Path != "README.md" && fd.Status != "deleted" {
						t.Fatalf("%s: status %s, want deleted", fd.Path, fd.Status)
					}
				}
				if lic := insFileByPath(t, res, "LICENSE"); !strings.HasPrefix(lic.Patch, "--- a/LICENSE\n+++ /dev/null\n") || lic.Deletions != 1 || lic.NewHash != "" {
					t.Fatalf("LICENSE = %+v", lic)
				}
			},
		},
		{
			name: "same revision", method: "GET", path: webAPI + "/compare?from=main&to=snap-2", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				res := decode[compareResult](t, rec)
				if len(res.Files) != 0 || len(res.Snapshots) != 0 || res.Stats != (compareStats{}) || res.MergeBase != "" {
					t.Fatalf("res = %+v", res)
				}
				if !strings.Contains(rec.Body.String(), `"files":[]`) || !strings.Contains(rec.Body.String(), `"snapshots":[]`) {
					t.Fatalf("empty lists not encoded as []: %s", rec.Body.String())
				}
			},
		},
		{
			name: "context zero", method: "GET", path: webAPI + "/compare?from=snap-2&to=snap-3&context=0", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				res := decode[compareResult](t, rec)
				want := "--- a/src/main.go\n+++ b/src/main.go\n@@ -3 +3 @@\n-func main() {}\n+func main() { println(1) }\n"
				if got := insFileByPath(t, res, "src/main.go").Patch; got != want {
					t.Fatalf("patch = %q, want %q", got, want)
				}
			},
		},
		{
			name: "id prefix", method: "GET", path: webAPI + "/compare?to=abcd", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if res := decode[compareResult](t, rec); res.To.ID != "abcdef0123" {
					t.Fatalf("to = %s", res.To.ID)
				}
			},
		},
		{name: "ambiguous prefix", method: "GET", path: webAPI + "/compare?to=snap", as: "alice", status: 400, code: "bad_request"},
		{name: "prefix too short", method: "GET", path: webAPI + "/compare?to=abc", as: "alice", status: 404, code: "not_found"},
		{name: "unknown revision", method: "GET", path: webAPI + "/compare?to=nope-nope", as: "alice", status: 404, code: "not_found"},
		{name: "unknown from", method: "GET", path: webAPI + "/compare?to=main&from=zzzz", as: "alice", status: 404, code: "not_found"},
		{name: "snapshot of another tree", method: "GET", path: webAPI + "/compare?to=snap-foreign", as: "alice", status: 404, code: "not_found"},
		{name: "missing to", method: "GET", path: webAPI + "/compare", as: "alice", status: 400, code: "bad_request"},
		{name: "bad context", method: "GET", path: webAPI + "/compare?to=main&context=-1", as: "alice", status: 400, code: "bad_request"},
		{name: "missing manifest is internal", method: "GET", path: webAPI + "/compare?to=snap-orphan", as: "alice", status: 500, code: "internal"},
		{
			name: "empty branch", method: "GET", path: webAPI + "/compare?to=empty", as: "alice", status: 404, code: "not_found",
			setup: func(t *testing.T, f *fixture) {
				must(t, f.store.PutBranch(context.Background(), &domain.Branch{TreeID: f.web.ID, Name: "empty"}))
			},
		},
		{name: "anonymous on private tree", method: "GET", path: webAPI + "/compare?to=main", status: 401, code: "unauthenticated"},
		{name: "outsider", method: "GET", path: webAPI + "/compare?to=main", as: "dave", status: 403, code: "forbidden"},
		{name: "unknown tree", method: "GET", path: "/api/v1/trees/acme/nope/compare?to=main", as: "alice", status: 404, code: "not_found"},
	})
}

func TestCompareSnapshotsCapped(t *testing.T) {
	f := newInsightFixture(t)
	parent := "snap-2"
	for i := 0; i < maxCompareSnapshots+10; i++ {
		id := fmt.Sprintf("long-%03d", i)
		insAddSnap(t, f, id, "long", "alice", t0.Add(time.Duration(10+i)*time.Hour), insS2Files(), parent)
		parent = id
	}
	rec := f.do(t, "GET", webAPI+"/compare?from=main&to=long", "alice", nil)
	if rec.Code != 200 {
		t.Fatalf("status %d: %s", rec.Code, rec.Body.String())
	}
	res := decode[compareResult](t, rec)
	if len(res.Snapshots) != maxCompareSnapshots || res.Snapshots[0].ID != parent {
		t.Fatalf("got %d snapshots, first %s", len(res.Snapshots), res.Snapshots[0].ID)
	}
}

func TestSnapshotForRevPrecedence(t *testing.T) {
	f := newInsightFixture(t)
	ctx := context.Background()
	// A branch whose name is also a unique id prefix wins over the prefix.
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "abcd", Tip: "snap-1"}))
	a := &api{d: f.deps}
	tests := []struct {
		rev, want string
	}{
		{"snap-4", "snap-4"},
		{"abcd", "snap-1"},
		{"abcde", "abcdef0123"},
		{"v1", "snap-1"},
		{"topic", "snap-4"},
	}
	for _, tt := range tests {
		s, err := a.snapshotForRev(ctx, &f.web, tt.rev)
		if err != nil || s.ID != tt.want {
			t.Errorf("snapshotForRev(%q) = %v, %v; want %s", tt.rev, s, err, tt.want)
		}
	}
	if _, err := a.snapshotForRev(ctx, &f.web, ""); err == nil {
		t.Error("empty revision accepted")
	}
}

// ---- file history ----

func TestFileHistory(t *testing.T) {
	type entry struct{ ID, Change string }
	entries := func(t *testing.T, rec *httptest.ResponseRecorder) []entry {
		var out []entry
		for _, e := range decode[[]fileHistoryEntry](t, rec) {
			out = append(out, entry{e.Snapshot.ID, e.Change})
		}
		return out
	}
	expect := func(want ...entry) func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			if got := entries(t, rec); !slices.Equal(got, want) {
				t.Fatalf("history = %v, want %v", got, want)
			}
		}
	}
	insRunCases(t, []endpointCase{
		{name: "default branch", method: "GET", path: webAPI + "/history?path=README.md", as: "carol", status: 200,
			check: expect(entry{"snap-2", "modified"}, entry{"snap-1", "added"})},
		{name: "added later", method: "GET", path: webAPI + "/history?path=src/main.go", as: "carol", status: 200,
			check: expect(entry{"snap-2", "added"})},
		{name: "deleted and re-added", method: "GET", path: webAPI + "/history?path=README.md&branch=topic", as: "alice", status: 200,
			check: expect(entry{"snap-4", "added"}, entry{"snap-3", "deleted"}, entry{"snap-2", "modified"}, entry{"snap-1", "added"})},
		{name: "limit", method: "GET", path: webAPI + "/history?path=README.md&branch=topic&limit=2", as: "alice", status: 200,
			check: expect(entry{"snap-4", "added"}, entry{"snap-3", "deleted"})},
		{name: "mode change counts as modified", method: "GET", path: webAPI + "/history?path=run.sh&branch=dev", as: "alice", status: 200,
			check: expect(entry{"abcdef0123", "modified"}, entry{"snap-2", "added"})},
		{name: "never existed", method: "GET", path: webAPI + "/history?path=nope.txt", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if strings.TrimSpace(rec.Body.String()) != "[]" {
					t.Fatalf("body = %s", rec.Body.String())
				}
			}},
		{name: "nothing pushed to default branch", method: "GET", path: "/api/v1/trees/acme/docs/history?path=README.md", status: 200,
			check: expect()},
		{name: "license restricted path", method: "GET", path: webAPI + "/history?path=secret/key.txt", as: "alice", status: 403, code: "license_violation"},
		{name: "missing path", method: "GET", path: webAPI + "/history", as: "alice", status: 400, code: "bad_request"},
		{name: "unclean path", method: "GET", path: webAPI + "/history?path=src/../README.md", as: "alice", status: 400, code: "bad_request"},
		{name: "absolute path", method: "GET", path: webAPI + "/history?path=/README.md", as: "alice", status: 400, code: "bad_request"},
		{name: "parent path", method: "GET", path: webAPI + "/history?path=../x", as: "alice", status: 400, code: "bad_request"},
		{name: "unknown branch", method: "GET", path: webAPI + "/history?path=README.md&branch=nope", as: "alice", status: 404, code: "not_found"},
		{name: "bad limit", method: "GET", path: webAPI + "/history?path=README.md&limit=0", as: "alice", status: 400, code: "bad_request"},
		{name: "outsider", method: "GET", path: webAPI + "/history?path=README.md", as: "dave", status: 403, code: "forbidden"},
	})
}

// ---- stats ----

// TestTreeStats is not parallel: it pins the stats clock.
func TestTreeStats(t *testing.T) {
	old := statsNow
	statsNow = func() time.Time { return t0.Add(48 * time.Hour) } // 2026-01-04
	t.Cleanup(func() { statsNow = old })

	f := newInsightFixture(t)
	get := func(t *testing.T, query, as string, status int) treeStats {
		t.Helper()
		rec := f.do(t, "GET", webAPI+"/stats"+query, as, nil)
		if rec.Code != status {
			t.Fatalf("status = %d, want %d: %s", rec.Code, status, rec.Body.String())
		}
		if status != 200 {
			return treeStats{}
		}
		return decode[treeStats](t, rec)
	}

	t.Run("window of three days", func(t *testing.T) {
		st := get(t, "?branch=dev&days=3", "carol", 200)
		if st.Branch != "dev" || st.SnapshotCount != 3 {
			t.Fatalf("branch/count = %s/%d", st.Branch, st.SnapshotCount)
		}
		wantAct := []activityDay{{"2026-01-02", 2}, {"2026-01-03", 1}, {"2026-01-04", 0}}
		if !slices.Equal(st.Activity, wantAct) {
			t.Fatalf("activity = %v", st.Activity)
		}
		if len(st.Contributors) != 2 || st.Contributors[0] != (contributorStat{"alice", 2, t0.Add(time.Hour)}) ||
			st.Contributors[1] != (contributorStat{"carol", 1, t0.Add(26 * time.Hour)}) {
			t.Fatalf("contributors = %+v", st.Contributors)
		}
		if st.FileCount != 8 {
			t.Fatalf("file_count = %d", st.FileCount)
		}
		var total uint64
		for _, fl := range insS2Files() {
			total += uint64(len(fl.content))
		}
		total += uint64(len(bigText)) + 10 // img.png
		if st.TotalBytes != total {
			t.Fatalf("total_bytes = %d, want %d", st.TotalBytes, total)
		}
		langs := map[string]languageShare{}
		for _, l := range st.Languages {
			langs[l.Language] = l
		}
		if langs["Go"].Files != 1 || langs["HTML"].Bytes != 38 || langs["Shell"].Files != 1 || langs["Markdown"].Files != 1 {
			t.Fatalf("languages = %+v", st.Languages)
		}
		// LICENSE, secret/key.txt, img.png and big.txt are unrecognised.
		if last := st.Languages[len(st.Languages)-1]; last.Language != "Other" || last.Files != 4 {
			t.Fatalf("last language = %+v", last)
		}
	})
	t.Run("narrow window", func(t *testing.T) {
		st := get(t, "?branch=dev&days=2", "carol", 200)
		if st.SnapshotCount != 1 || len(st.Contributors) != 1 || st.Contributors[0].Author != "carol" ||
			!slices.Equal(st.Activity, []activityDay{{"2026-01-03", 1}, {"2026-01-04", 0}}) {
			t.Fatalf("stats = %+v", st)
		}
	})
	t.Run("default branch and days", func(t *testing.T) {
		st := get(t, "", "carol", 200)
		if st.Branch != "main" || len(st.Activity) != defaultStatsDays || st.Activity[len(st.Activity)-1].Date != "2026-01-04" || st.SnapshotCount != 2 {
			t.Fatalf("stats = %+v", st)
		}
	})
	t.Run("days clamped", func(t *testing.T) {
		if st := get(t, "?days=1000", "carol", 200); len(st.Activity) != maxStatsDays {
			t.Fatalf("activity has %d days", len(st.Activity))
		}
	})
	t.Run("empty tree", func(t *testing.T) {
		rec := f.do(t, "GET", "/api/v1/trees/acme/docs/stats?days=5", "", nil)
		if rec.Code != 200 {
			t.Fatalf("status %d: %s", rec.Code, rec.Body.String())
		}
		for _, frag := range []string{`"contributors":[]`, `"languages":[]`, `"file_count":0`} {
			if !strings.Contains(rec.Body.String(), frag) {
				t.Fatalf("missing %s in %s", frag, rec.Body.String())
			}
		}
		if st := decode[treeStats](t, rec); len(st.Activity) != 5 || st.Activity[0].Date != "2025-12-31" {
			t.Fatalf("activity = %v", st.Activity)
		}
	})
	for _, tc := range []struct {
		name, query, as string
		status          int
	}{
		{"zero days", "?days=0", "alice", 400},
		{"bad days", "?days=x", "alice", 400},
		{"unknown branch", "?branch=nope", "alice", 404},
		{"outsider", "", "dave", 403},
		{"anonymous", "", "", 401},
	} {
		t.Run(tc.name, func(t *testing.T) { get(t, tc.query, tc.as, tc.status) })
	}
}

func TestLanguageShares(t *testing.T) {
	var entries []*syncpb.ManifestEntry
	add := func(p string, size uint64) {
		entries = append(entries, &syncpb.ManifestEntry{Path: p, Size: size})
	}
	exts := []string{".go", ".rs", ".py", ".js", ".ts", ".java", ".c", ".cpp", ".rb", ".php", ".swift", ".kt"}
	for i, ext := range exts {
		add("f"+ext, uint64(1000-i*10)) // descending sizes
	}
	add("Makefile", 5)
	add("docs/Dockerfile", 7)
	add("UPPER.GO", 1)
	add("blob.bin", 3)
	got := languageShares(entries)
	if len(got) != maxStatsLanguages+1 {
		t.Fatalf("got %d languages: %+v", len(got), got)
	}
	if got[0] != (languageShare{"Go", 2, 1001}) {
		t.Fatalf("first = %+v", got[0])
	}
	// Folded: Swift (900), Kotlin (890), Makefile (5), Dockerfile (7) and
	// the unrecognised blob.bin (3).
	if other := got[len(got)-1]; other != (languageShare{"Other", 5, 900 + 890 + 5 + 7 + 3}) {
		t.Fatalf("other = %+v", other)
	}
	for i := 1; i < maxStatsLanguages; i++ {
		if got[i].Bytes > got[i-1].Bytes {
			t.Fatalf("not sorted by bytes: %+v", got)
		}
	}
	tests := map[string]string{
		"a/b/main.go": "Go", "Makefile": "Makefile", "x/Dockerfile.dev": "Dockerfile", "CMakeLists.txt": "CMake",
		"README.MD": "Markdown", "noext": "", "archive.tar.gz": "", "app.tsx": "TypeScript",
	}
	for p, want := range tests {
		if got := languageOf(p); got != want {
			t.Errorf("languageOf(%q) = %q, want %q", p, got, want)
		}
	}
}

// ---- staged summary ----

func insSeedStaged(f *fixture) {
	f.store.mu.Lock()
	defer f.store.mu.Unlock()
	f.store.staged = append(f.store.staged,
		domain.StagedSnapshot{SnapshotID: "wip-2", TreeID: f.web.ID, TenantID: f.acme.ID, AccountID: "acct-carol", Author: "carol",
			BranchName: "main", FilesChanged: []string{"a.go", "b.go"}, FilesModified: 2, StagedAt: t0.Add(2 * time.Hour)},
		domain.StagedSnapshot{SnapshotID: "wip-3", TreeID: f.web.ID, TenantID: f.acme.ID, AccountID: "acct-alice", Author: "alice",
			BranchName: "feature", FilesChanged: []string{"a.go"}, FilesAdded: 1, StagedAt: t0.Add(3 * time.Hour)},
		domain.StagedSnapshot{SnapshotID: "wip-x", TreeID: f.docs.ID, TenantID: f.acme.ID, AccountID: "acct-bob", Author: "bob",
			BranchName: "main", FilesChanged: []string{"a.go"}, StagedAt: t0.Add(5 * time.Hour)},
	)
}

func TestStagedSummaryFallback(t *testing.T) {
	want := []stagedBranchActivity{
		{
			Branch:       "feature",
			Authors:      []stagedAuthorActivity{{Author: "alice", AccountID: "acct-alice", Files: 1, StagedAt: t0.Add(3 * time.Hour)}},
			FilesChanged: 1,
			LatestAt:     t0.Add(3 * time.Hour),
			Overlaps:     []stagedFileOverlap{{Path: "a.go", Authors: []string{"alice", "carol"}, Branches: []string{"feature", "main"}}},
		},
		{
			Branch: "main",
			Authors: []stagedAuthorActivity{
				{Author: "carol", AccountID: "acct-carol", Files: 2, StagedAt: t0.Add(2 * time.Hour)},
				{Author: "bob", AccountID: "acct-bob", Files: 0, StagedAt: t0},
			},
			FilesChanged: 2,
			LatestAt:     t0.Add(2 * time.Hour),
			Overlaps:     []stagedFileOverlap{{Path: "a.go", Authors: []string{"alice", "carol"}, Branches: []string{"feature", "main"}}},
		},
	}
	insRunCases(t, []endpointCase{
		{
			name: "computed from List", method: "GET", path: webAPI + "/staged/summary", as: "carol", status: 200,
			setup: func(t *testing.T, f *fixture) { insSeedStaged(f) },
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				got := decode[[]stagedBranchActivity](t, rec)
				gotJSON, _ := json.Marshal(got)
				wantJSON, _ := json.Marshal(want)
				if string(gotJSON) != string(wantJSON) {
					t.Fatalf("summary =\n%s\nwant\n%s", gotJSON, wantJSON)
				}
			},
		},
		{
			name: "no staged work", method: "GET", path: "/api/v1/trees/acme/docs/staged/summary", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if strings.TrimSpace(rec.Body.String()) != "[]" {
					t.Fatalf("body = %s", rec.Body.String())
				}
			},
		},
		{name: "outsider", method: "GET", path: webAPI + "/staged/summary", as: "dave", status: 403, code: "forbidden"},
		{name: "anonymous", method: "GET", path: webAPI + "/staged/summary", status: 401, code: "unauthenticated"},
	})
}

func TestStagedSummaryFromService(t *testing.T) {
	at := t0.Add(time.Hour)
	acts := []staged.BranchActivity{{
		Branch: "main",
		Authors: []staged.AuthorActivity{
			{AccountID: "acct-bob", Author: "bob", FilesAdded: 1, FilesModified: 2, FilesListed: 1, StagedAt: at},
			{AccountID: "acct-carol", Author: "carol", FilesListed: 4, StagedAt: t0},
		},
		DistinctFiles: 4,
		LatestAt:      at,
		Overlaps: []staged.FileOverlap{{Path: "x.go", Contributors: []staged.Contributor{
			{AccountID: "acct-bob", Author: "bob", Branch: "main"},
			{AccountID: "acct-zed", Branch: "other"},
			{AccountID: "acct-bob", Author: "bob", Branch: "other"},
		}}},
	}}
	svc := func(err error) func(*Deps) {
		return func(d *Deps) {
			d.Staged = &insSummaryStaged{fakeStaged: &fakeStaged{store: d.Store.(*fakeStore)}, acts: acts, err: err}
		}
	}
	insRunCases(t, []endpointCase{{
		name: "mapped", method: "GET", path: webAPI + "/staged/summary", as: "carol", status: 200,
		check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			got := decode[[]stagedBranchActivity](t, rec)
			if len(got) != 1 {
				t.Fatalf("got %+v", got)
			}
			g := got[0]
			if g.Branch != "main" || g.FilesChanged != 4 || !g.LatestAt.Equal(at) || len(g.Authors) != 2 ||
				g.Authors[0].Files != 3 || g.Authors[1].Files != 4 || g.Authors[0].AccountID != "acct-bob" {
				t.Fatalf("got %+v", g)
			}
			wantOv := stagedFileOverlap{Path: "x.go", Authors: []string{"acct-zed", "bob"}, Branches: []string{"main", "other"}}
			if len(g.Overlaps) != 1 || g.Overlaps[0].Path != wantOv.Path || !slices.Equal(g.Overlaps[0].Authors, wantOv.Authors) ||
				!slices.Equal(g.Overlaps[0].Branches, wantOv.Branches) {
				t.Fatalf("overlaps = %+v", g.Overlaps)
			}
		},
	}}, svc(nil))
	insRunCases(t, []endpointCase{{
		name: "service error", method: "GET", path: webAPI + "/staged/summary", as: "carol", status: 403, code: "forbidden",
	}}, svc(fmt.Errorf("denied: %w", domain.ErrForbidden)))
}

// ---- recent events ----

func insRecentDeps(evs []domain.Event) func(*Deps) {
	return func(d *Deps) { d.Bus = &insRecentBus{fakeBus: newFakeBus(), events: evs} }
}

func TestRecentEvents(t *testing.T) {
	evs := []domain.Event{
		{Type: domain.EventBranchPushed, TenantID: "tenant-acme", TreeID: "tree-web", Actor: "alice", At: t0},
		{Type: domain.EventTagPushed, TenantID: "tenant-acme", TreeID: "tree-docs", Actor: "alice", At: t0.Add(time.Minute)},
		{Type: domain.EventTreeCreated, TenantID: "tenant-dave", TreeID: "tree-dave", Actor: "dave", At: t0.Add(2 * time.Minute)},
		{Type: domain.EventSnapshotStaged, TenantID: "tenant-acme", TreeID: "tree-web", Actor: "bob", Payload: map[string]any{"bad": make(chan int)}, At: t0.Add(3 * time.Minute)},
		{Type: domain.EventMROpened, TenantID: "tenant-bob", TreeID: "tree-bob", Actor: "bob", At: t0.Add(4 * time.Minute)},
	}
	types := func(want ...string) func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
		return func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			var got []string
			for _, e := range decode[[]domain.Event](t, rec) {
				got = append(got, e.Type)
			}
			if !slices.Equal(got, want) {
				t.Fatalf("types = %v, want %v", got, want)
			}
		}
	}
	insRunCases(t, []endpointCase{
		{name: "admin sees everything", method: "GET", path: "/api/v1/events/recent", as: "admin", status: 200,
			check: types("branch.pushed", "tag.pushed", "tree.created", "snapshot.staged", "mr.opened")},
		{name: "member sees member tenants", method: "GET", path: "/api/v1/events/recent", as: "bob", status: 200,
			check: types("branch.pushed", "tag.pushed", "snapshot.staged", "mr.opened")},
		{name: "member limit keeps newest", method: "GET", path: "/api/v1/events/recent?limit=2", as: "bob", status: 200,
			check: types("snapshot.staged", "mr.opened")},
		{name: "admin limit", method: "GET", path: "/api/v1/events/recent?limit=1", as: "admin", status: 200,
			check: types("mr.opened")},
		{name: "tree scope", method: "GET", path: "/api/v1/events/recent?tenant=acme&tree=web", as: "carol", status: 200,
			check: types("branch.pushed", "snapshot.staged")},
		{name: "tenant scope with types", method: "GET", path: "/api/v1/events/recent?tenant=acme&types=tag.pushed,mr.opened", as: "carol", status: 200,
			check: types("tag.pushed")},
		{name: "public tree anonymous", method: "GET", path: "/api/v1/events/recent?tenant=acme&tree=docs", status: 200,
			check: types("tag.pushed")},
		{name: "unencodable payload dropped", method: "GET", path: "/api/v1/events/recent?tenant=acme&types=snapshot.staged", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				got := decode[[]domain.Event](t, rec)
				if len(got) != 1 || got[0].Payload != nil || got[0].Actor != "bob" {
					t.Fatalf("got %+v", got)
				}
			}},
		{name: "anonymous unscoped", method: "GET", path: "/api/v1/events/recent", status: 401, code: "unauthenticated"},
		{name: "tree without tenant", method: "GET", path: "/api/v1/events/recent?tree=web", as: "alice", status: 400, code: "bad_request"},
		{name: "tenant outsider", method: "GET", path: "/api/v1/events/recent?tenant=acme", as: "dave", status: 403, code: "forbidden"},
		{name: "bad limit", method: "GET", path: "/api/v1/events/recent?limit=abc", as: "alice", status: 400, code: "bad_request"},
	}, insRecentDeps(evs))

	insRunCases(t, []endpointCase{
		{name: "bus without history", method: "GET", path: "/api/v1/events/recent", as: "admin", status: 200, check: types()},
	})
	insRunCases(t, []endpointCase{
		{name: "no bus", method: "GET", path: "/api/v1/events/recent", as: "admin", status: 200, check: types()},
	}, func(d *Deps) { d.Bus = nil })
}

func TestRecentEventsLimitClamped(t *testing.T) {
	var evs []domain.Event
	for i := 0; i < maxRecentEvents+50; i++ {
		evs = append(evs, domain.Event{Type: domain.EventBranchPushed, TenantID: "tenant-acme", At: t0.Add(time.Duration(i) * time.Second)})
	}
	f := newInsightFixture(t, insRecentDeps(evs))
	for _, as := range []string{"admin", "alice"} {
		rec := f.do(t, "GET", "/api/v1/events/recent?limit=100000", as, nil)
		got := decode[[]domain.Event](t, rec)
		if len(got) != maxRecentEvents || !got[len(got)-1].At.Equal(evs[len(evs)-1].At) {
			t.Fatalf("%s: got %d events", as, len(got))
		}
	}
	rec := f.do(t, "GET", "/api/v1/events/recent", "admin", nil)
	if got := decode[[]domain.Event](t, rec); len(got) != defaultRecentEvents {
		t.Fatalf("default limit returned %d events", len(got))
	}
}
