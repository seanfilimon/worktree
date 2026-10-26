package httpapi

import (
	"context"
	"io"
	"log/slog"
	"net/http"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// stagedFilesFixture extends the read-path fixture (paths_test.go) with
// the real staged service and three staged entries of bob on tree app:
//
//	wip-1        manifest with README.md, secret/key.txt, vendor/lib.c
//	wip-none     no manifest
//	wip-missing  manifest hash of an object that was never uploaded
func stagedFilesFixture(t *testing.T) *pathsFixture {
	t.Helper()
	f := newPathsFixture(t)
	ctx := context.Background()
	m := f.manifest(t, map[string]string{"README.md": "wip readme\n", "secret/key.txt": "wip-key\n", "vendor/lib.c": "wip-lib\n"})
	absent := hash.Sum([]byte("never uploaded"))
	now := time.Now().UTC()
	for _, e := range []domain.StagedSnapshot{
		{SnapshotID: "wip-1", BranchName: "main", ManifestHash: m.Hex()},
		{SnapshotID: "wip-none", BranchName: "feature"},
		{SnapshotID: "wip-missing", BranchName: "fix", ManifestHash: absent.Hex()},
	} {
		e.TreeID, e.TenantID, e.AccountID, e.Author, e.StagedAt = f.tree.ID, "t-acme", "a-bob", "bob", now
		must(t, f.store.PutStaged(ctx, &e))
	}
	users := map[string]*domain.Principal{}
	for _, u := range []string{"alice", "bob", "olga"} {
		users[u] = &domain.Principal{AccountID: "a-" + u, Username: u, TenantID: "t-" + u}
	}
	az := authz.New(f.store, authz.Options{})
	f.h = New(Deps{
		Store:   f.store,
		Objects: f.objs,
		History: meta.NewHistory(f.store),
		Auth:    &pathsAuth{users: users},
		Authz:   az,
		Access:  az,
		License: license.New(f.store),
		Staged:  staged.New(f.store, az, nil),
		Logger:  slog.New(slog.NewTextHandler(io.Discard, nil)),
	})
	return f
}

func TestStagedFilesList(t *testing.T) {
	f := stagedFilesFixture(t)
	cases := []struct {
		name, as, snapshot string
		status             int
		code               string
		files              []string
		restricted         int
	}{
		{"owner sees everything", "alice", "wip-1", 200, "", []string{"README.md", "secret/key.txt", "vendor/lib.c"}, 0},
		{"path deny hides secret", "bob", "wip-1", 200, "", []string{"README.md", "vendor/lib.c"}, 1},
		{"license hides vendor", "olga", "wip-1", 200, "", []string{"README.md", "secret/key.txt"}, 1},
		// staged-v2 (§16.8): staged metadata is hidden from anonymous readers.
		{"anonymous on a public tree", "", "wip-1", 404, "not_found", nil, 0},
		{"unknown staged snapshot", "alice", "nope", 404, "not_found", nil, 0},
		{"committed snapshot is not staged", "alice", f.p2, 404, "not_found", nil, 0},
		{"no manifest", "alice", "wip-none", 404, "not_found", nil, 0},
		{"manifest not uploaded", "alice", "wip-missing", 404, "not_found", nil, 0},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+"/staged/"+tc.snapshot+"/files", tc.as, nil)
			wantStatus(t, rec, tc.status, tc.code)
			if tc.status != 200 {
				return
			}
			res := decode[stagedFilesResponse](t, rec)
			var got []string
			for _, fe := range res.Files {
				got = append(got, fe.Path)
			}
			if !slices.Equal(got, tc.files) || res.RestrictedCount != tc.restricted {
				t.Fatalf("files = %v (restricted %d), want %v (%d)", got, res.RestrictedCount, tc.files, tc.restricted)
			}
			if res.Staged.SnapshotID != tc.snapshot || res.Staged.Author != "bob" {
				t.Fatalf("staged = %+v", res.Staged)
			}
		})
	}
}

func TestStagedFileContent(t *testing.T) {
	f := stagedFilesFixture(t)
	cases := []struct {
		name, as, snapshot, path string
		status                   int
		code, body               string
	}{
		{"reads staged content", "bob", "wip-1", "README.md", 200, "", "wip readme\n"},
		{"owner reads secret", "alice", "wip-1", "secret/key.txt", 200, "", "wip-key\n"},
		{"path denied", "bob", "wip-1", "secret/key.txt", 403, codePathDenied, ""},
		{"path denied hides existence", "bob", "wip-1", "secret/other.txt", 403, codePathDenied, ""},
		{"license restricted", "olga", "wip-1", "vendor/lib.c", 451, codeLicenseRestricted, ""},
		{"anonymous sees no staged entry", "", "wip-1", "README.md", 404, "not_found", ""}, // staged-v2 (§16.8)
		{"missing file", "alice", "wip-1", "nope.txt", 404, "not_found", ""},
		{"unknown staged snapshot", "alice", "nope", "README.md", 404, "not_found", ""},
		{"invalid path", "alice", "wip-1", "a/../README.md", 400, "bad_request", ""},
		{"no manifest", "alice", "wip-none", "README.md", 404, "not_found", ""},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+"/staged/"+tc.snapshot+"/files/"+tc.path, tc.as, nil)
			if tc.code == "bad_request" {
				// The mux may clean ".." itself (redirect) before the handler.
				if rec.Code == http.StatusOK {
					t.Fatalf("invalid path served: %s", rec.Body.String())
				}
				return
			}
			wantStatus(t, rec, tc.status, tc.code)
			if tc.body != "" && rec.Body.String() != tc.body {
				t.Fatalf("body = %q, want %q", rec.Body.String(), tc.body)
			}
			if tc.status >= 400 && (strings.Contains(rec.Body.String(), "wip-key") || strings.Contains(rec.Body.String(), "wip-lib")) {
				t.Fatalf("content leaked: %s", rec.Body.String())
			}
			if tc.status == 200 {
				if got := rec.Header().Get("X-Content-Type-Options"); got != "nosniff" {
					t.Fatalf("X-Content-Type-Options = %q", got)
				}
			}
		})
	}
}

func TestStagedFilesDownload(t *testing.T) {
	f := stagedFilesFixture(t)
	rec := f.do(t, "GET", appAPI+"/staged/wip-1/files/README.md?download", "bob", nil)
	wantStatus(t, rec, 200, "")
	if cd := rec.Header().Get("Content-Disposition"); !strings.HasPrefix(cd, "attachment") {
		t.Fatalf("Content-Disposition = %q", cd)
	}
}

func TestStagedFilesNotImplemented(t *testing.T) {
	f := newPathsFixture(t) // no Deps.Staged
	rec := f.do(t, "GET", appAPI+"/staged/wip-1/files", "alice", nil)
	wantStatus(t, rec, http.StatusNotImplemented, "not_implemented")
}
