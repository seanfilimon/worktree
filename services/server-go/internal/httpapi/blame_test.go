package httpapi

import (
	"context"
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// GET {tree}/blame/{rev}/{path...} (docs/ENGINEERING_PLAN.md §14.4) over
// the read-path security fixture of paths_test.go.

func decodeBlame(t *testing.T, body string) blameResponse {
	t.Helper()
	var out blameResponse
	if err := json.Unmarshal([]byte(body), &out); err != nil {
		t.Fatalf("decode %s: %v", body, err)
	}
	return out
}

func TestBlameEndpoint(t *testing.T) {
	f := newPathsFixture(t)
	ctx := context.Background()
	// p3 edits README.md (adds a line) and adds a binary file.
	m3 := f.manifest(t, map[string]string{"README.md": "two\nthree\n", "secret/key.txt": "k2\n", "vendor/lib.c": "lib2\n", "src/a.go": "package a\n", "img.bin": "\x00\x01\x02"})
	p3 := "snap-p3"
	must(t, f.store.PutSnapshot(ctx, &domain.Snapshot{ID: p3, TreeID: f.tree.ID, BranchName: "main", Parents: []string{f.p2}, ManifestHash: m3, Author: "bob", Message: "three\n\nbody", CreatedAt: t0.Add(2 * time.Hour)}))
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: f.tree.ID, Name: "main", Tip: p3, CreatedAt: t0, UpdatedAt: t0}))

	rec := f.do(t, "GET", appAPI+"/blame/main/README.md", "alice", nil)
	wantStatus(t, rec, 200, "")
	got := decodeBlame(t, rec.Body.String())
	if got.Snapshot.ID != p3 || len(got.Lines) != 2 {
		t.Fatalf("blame = %s", rec.Body.String())
	}
	if l := got.Lines[0]; l.Line != 1 || l.Text != "two" || l.SnapshotID != f.p2 || l.Message != "two" || l.Author != "alice" {
		t.Fatalf("line 1 = %+v", l)
	}
	if l := got.Lines[1]; l.Line != 2 || l.Text != "three" || l.SnapshotID != p3 || l.Message != "three" {
		t.Fatalf("line 2 = %+v", l)
	}

	// By snapshot id, at an older revision, and a line range.
	got = decodeBlame(t, f.do(t, "GET", appAPI+"/blame/"+f.p1+"/README.md", "alice", nil).Body.String())
	if len(got.Lines) != 1 || got.Lines[0].SnapshotID != f.p1 || got.Snapshot.ID != f.p1 {
		t.Fatalf("at p1 = %+v", got)
	}
	rec = f.do(t, "GET", appAPI+"/blame/"+p3+"/README.md?start=2&end=9", "alice", nil)
	wantStatus(t, rec, 200, "")
	got = decodeBlame(t, rec.Body.String())
	if len(got.Lines) != 1 || got.Lines[0].Line != 2 {
		t.Fatalf("range = %s", rec.Body.String())
	}

	// Chunked file: unchanged since p1, read through content.ReadEntry.
	rec = f.do(t, "GET", appAPI+"/blame/"+f.p2+"/big.bin", "olga", nil)
	wantStatus(t, rec, 200, "")
	got = decodeBlame(t, rec.Body.String())
	if len(got.Lines) != 1 || got.Lines[0].SnapshotID != f.p1 || got.Lines[0].Text != string(f.big) {
		t.Fatalf("chunked blame = %+v", got.Lines)
	}

	cases := []struct {
		name, as, target string
		status           int
		code             string
	}{
		{"anonymous public read", "", "/blame/main/README.md", 200, ""},
		{"path denied", "bob", "/blame/main/secret/key.txt", 403, codePathDenied},
		{"path denied hides existence", "bob", "/blame/main/secret/missing.txt", 403, codePathDenied},
		{"license restricted", "olga", "/blame/main/vendor/lib.c", 451, codeLicenseRestricted},
		{"anonymous license restricted", "", "/blame/main/vendor/lib.c", 451, codeLicenseRestricted},
		{"member reads vendor", "bob", "/blame/main/vendor/lib.c", 200, ""},
		{"missing file", "alice", "/blame/main/nope.txt", 404, "not_found"},
		{"unknown revision", "alice", "/blame/nope/README.md", 404, "not_found"},
		{"binary file", "alice", "/blame/main/img.bin", 422, "binary_file"},
		{"range past the end", "alice", "/blame/main/README.md?start=5", 400, "bad_request"},
		{"bad range parameter", "alice", "/blame/main/README.md?start=x", 400, "bad_request"},
		{"unclean path", "alice", "/blame/main/src/../README.md", 400, ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+tc.target, tc.as, nil)
			if tc.name == "unclean path" {
				// The mux may clean the path itself (redirect) before the
				// handler sees it; either way the request must not succeed.
				if rec.Code == 200 {
					t.Fatalf("unclean path succeeded: %s", rec.Body.String())
				}
				return
			}
			wantStatus(t, rec, tc.status, tc.code)
			if tc.status >= 400 && (strings.Contains(rec.Body.String(), "k2") || strings.Contains(rec.Body.String(), "lib2")) {
				t.Fatalf("content leaked: %s", rec.Body.String())
			}
		})
	}
}

func TestBlameRequiresTreeRead(t *testing.T) {
	f := newPathsFixture(t)
	ctx := context.Background()
	f.tree.Visibility = domain.VisibilityPrivate
	must(t, f.store.UpdateTree(ctx, f.tree))
	rec := f.do(t, "GET", appAPI+"/blame/main/README.md", "olga", nil)
	if rec.Code != 403 && rec.Code != 404 {
		t.Fatalf("outsider on private tree: %d %s", rec.Code, rec.Body.String())
	}
	rec = f.do(t, "GET", appAPI+"/blame/main/README.md", "alice", nil)
	wantStatus(t, rec, 200, "")
}
