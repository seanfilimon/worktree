package httpapi

// staged-v2 (§16.8): labels, history, overlaps, path ACLs on staged
// metadata, hiding staged metadata from anonymous readers.

import (
	"context"
	"net/http"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// stagedV2Fixture is stagedFilesFixture plus alice's labeled entry "exp" on
// main touching README.md, secret/key.txt (bob may not read it) and
// vendor/lib.c (olga may not read it).
func stagedV2Fixture(t *testing.T) *pathsFixture {
	t.Helper()
	f := stagedFilesFixture(t)
	must(t, f.store.PutStaged(context.Background(), &domain.StagedSnapshot{
		SnapshotID: "wip-a", TreeID: f.tree.ID, TenantID: "t-acme", AccountID: "a-alice", Author: "alice",
		BranchName: "main", Label: "exp", StagedAt: time.Now().UTC(),
		FilesChanged: []string{"README.md", "secret/key.txt", "vendor/lib.c"},
	}))
	return f
}

func TestStagedListLabelsAndACL(t *testing.T) {
	f := stagedV2Fixture(t)
	for _, tc := range []struct {
		as    string
		files string
	}{
		{"alice", "README.md,secret/key.txt,vendor/lib.c"},
		{"bob", "README.md,vendor/lib.c"},
		{"olga", "README.md,secret/key.txt"},
	} {
		rec := f.do(t, "GET", appAPI+"/staged", tc.as, nil)
		wantStatus(t, rec, 200, "")
		list := decode[[]domain.StagedSnapshot](t, rec)
		var exp *domain.StagedSnapshot
		for i := range list {
			if list[i].Label == "exp" {
				exp = &list[i]
			}
		}
		if exp == nil || strings.Join(exp.FilesChanged, ",") != tc.files {
			t.Fatalf("%s: exp = %+v, want files %s", tc.as, exp, tc.files)
		}
	}
	rec := f.do(t, "GET", appAPI+"/staged", "", nil)
	wantStatus(t, rec, 200, "")
	if list := decode[[]domain.StagedSnapshot](t, rec); len(list) != 0 {
		t.Fatalf("anonymous list = %+v", list)
	}
}

func TestStagedCountHiddenFromAnonymous(t *testing.T) {
	f := stagedV2Fixture(t)
	type summary struct {
		StagedCount int `json:"staged_count"`
	}
	rec := f.do(t, "GET", appAPI, "", nil)
	wantStatus(t, rec, 200, "")
	if s := decode[summary](t, rec); s.StagedCount != 0 {
		t.Fatalf("anonymous staged_count = %d", s.StagedCount)
	}
	rec = f.do(t, "GET", appAPI, "olga", nil)
	wantStatus(t, rec, 200, "")
	if s := decode[summary](t, rec); s.StagedCount != 4 {
		t.Fatalf("staged_count = %d", s.StagedCount)
	}
}

func TestStagedOverlapsEndpoint(t *testing.T) {
	f := stagedV2Fixture(t)
	for _, tc := range []struct {
		name, as, query string
		status          int
		code            string
		want            []string // author/label:paths
	}{
		{"bob sees alice", "bob", "?paths=README.md,secret/key.txt&path=src/a.go", 200, "", []string{"alice/exp:README.md"}},
		{"secret path is never reported to bob", "bob", "?path=secret/key.txt", 200, "", []string{}},
		{"olga cannot see vendor", "olga", "?paths=vendor/lib.c,README.md", 200, "", []string{"alice/exp:README.md"}},
		{"own entries are not overlaps", "alice", "?paths=README.md", 200, "", []string{}},
		{"no paths", "bob", "", 200, "", []string{}},
		{"anonymous", "", "?paths=README.md", 401, "unauthenticated", nil},
		{"too many paths", "bob", "?paths=" + strings.Repeat("a,", staged.MaxOverlapPaths+1), 400, "bad_request", nil},
	} {
		t.Run(tc.name, func(t *testing.T) {
			rec := f.do(t, "GET", appAPI+"/staged/overlaps"+tc.query, tc.as, nil)
			wantStatus(t, rec, tc.status, tc.code)
			if tc.status != 200 {
				return
			}
			got := []string{}
			for _, o := range decode[stagedOverlapsResponse](t, rec).Overlaps {
				got = append(got, o.Author+"/"+o.Label+":"+strings.Join(o.Paths, ","))
			}
			if !slices.Equal(got, tc.want) {
				t.Fatalf("overlaps = %v, want %v", got, tc.want)
			}
		})
	}
}

func TestUnstageLabelAndHistoryEndpoints(t *testing.T) {
	f := stagedV2Fixture(t)
	for _, tc := range []struct {
		query  string
		status int
	}{
		{"?label=exp", 400},             // label needs branch
		{"?branch=main&label=BAD", 400}, // invalid label
	} {
		rec := f.do(t, http.MethodDelete, appAPI+"/staged"+tc.query, "alice", nil)
		wantStatus(t, rec, tc.status, "bad_request")
	}
	// Removing the default slot of main leaves the labeled one.
	rec := f.do(t, http.MethodDelete, appAPI+"/staged?branch=main&label=", "alice", nil)
	wantStatus(t, rec, 200, "")
	if res := decode[unstageResponse](t, rec); len(res.Removed) != 0 {
		t.Fatalf("alice has no default slot: %+v", res.Removed)
	}
	rec = f.do(t, http.MethodDelete, appAPI+"/staged?branch=main&label=exp", "alice", nil)
	wantStatus(t, rec, 200, "")
	if res := decode[unstageResponse](t, rec); len(res.Removed) != 1 || res.Removed[0].Label != "exp" {
		t.Fatalf("removed = %+v", res.Removed)
	}

	rec = f.do(t, "GET", appAPI+"/staged/history", "bob", nil)
	wantStatus(t, rec, 200, "")
	hist := decode[stagedHistoryResponse](t, rec).History
	if len(hist) != 1 || hist[0].Status != staged.StatusCleared || hist[0].Label != "exp" || hist[0].EndedBy != "alice" {
		t.Fatalf("history = %+v", hist)
	}
	if strings.Join(hist[0].FilesChanged, ",") != "README.md,vendor/lib.c" {
		t.Fatalf("bob sees restricted history paths: %v", hist[0].FilesChanged)
	}
	for _, q := range []string{"?status=cleared", "?branch=main", "?account=alice", "?limit=1"} {
		rec = f.do(t, "GET", appAPI+"/staged/history"+q, "bob", nil)
		wantStatus(t, rec, 200, "")
		if n := len(decode[stagedHistoryResponse](t, rec).History); n != 1 {
			t.Fatalf("%s: %d entries", q, n)
		}
	}
	for _, q := range []string{"?status=pushed", "?branch=dev", "?account=bob"} {
		rec = f.do(t, "GET", appAPI+"/staged/history"+q, "bob", nil)
		wantStatus(t, rec, 200, "")
		if n := len(decode[stagedHistoryResponse](t, rec).History); n != 0 {
			t.Fatalf("%s: %d entries", q, n)
		}
	}
	for _, q := range []string{"?status=staged", "?limit=0", "?limit=x", "?branch=a%20b"} {
		rec = f.do(t, "GET", appAPI+"/staged/history"+q, "bob", nil)
		wantStatus(t, rec, 400, "bad_request")
	}
	rec = f.do(t, "GET", appAPI+"/staged/history?account=nobody", "bob", nil)
	wantStatus(t, rec, 404, "not_found")
	rec = f.do(t, "GET", appAPI+"/staged/history", "", nil)
	wantStatus(t, rec, 200, "")
	if n := len(decode[stagedHistoryResponse](t, rec).History); n != 0 {
		t.Fatalf("anonymous history: %d entries", n)
	}
}
