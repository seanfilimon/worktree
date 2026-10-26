package httpapi

// history-v2 (§16.10): GET {tree}/reflog and the merge bypass over REST.

import (
	"context"
	"fmt"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/syncsvc"
)

func TestReflogEndpoint(t *testing.T) {
	f := newRVFixture(t)
	ctx := context.Background()
	t0 := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)
	for i, e := range []syncsvc.ReflogEntry{
		{Branch: "main", OldTip: "", NewTip: "s1", Kind: "push", Actor: "bob", Count: 1},
		{Branch: "main", OldTip: "s1", NewTip: "s9", Kind: "force_push", Forced: true, Actor: "alice", Bypass: true, Bypassed: []string{"force push past require_linear"}},
		{Branch: "feature", OldTip: "", NewTip: "s2", Kind: "push", Actor: "bob", Count: 2},
	} {
		e.TreeID = f.web.ID
		e.At = t0.Add(time.Duration(i) * time.Minute)
		key := domain.DocKey(f.web.ID, e.Branch, fmt.Sprintf("%020d", e.At.UnixNano()), "00")
		must(t, f.store.PutDoc(ctx, syncsvc.ReflogCollection, key, &e))
	}

	all := f.expect("GET", webT+"/reflog", "vic", nil, 200, "")
	entries := all["entries"].([]any)
	if len(entries) != 3 || entries[0].(map[string]any)["branch"] != "feature" {
		t.Fatalf("entries = %v", entries)
	}
	main := f.expect("GET", webT+"/reflog?branch=main&limit=1", "vic", nil, 200, "")
	entries = main["entries"].([]any)
	if len(entries) != 1 {
		t.Fatalf("main = %v", entries)
	}
	if e := entries[0].(map[string]any); e["kind"] != "force_push" || e["forced"] != true || e["old_tip"] != "s1" || e["bypass"] != true {
		t.Fatalf("force entry = %v", e)
	}
	none := f.expect("GET", webT+"/reflog?branch=nope", "vic", nil, 200, "")
	if fmt.Sprint(none["entries"]) != "[]" {
		t.Fatalf("unknown branch = %v", none)
	}
	f.expect("GET", webT+"/reflog?limit=zero", "vic", nil, 400, "bad_request")
	f.expect("GET", webT+"/reflog", "dave", nil, 403, "forbidden")
}

func TestMergeBypassREST(t *testing.T) {
	f := newRVFixture(t)
	rule := f.expect("PUT", webT+"/protections", "alice", map[string]any{"pattern": "main", "required_approvals": 2, "no_force_push": true}, 200, "")
	if rule["no_force_push"] != true {
		t.Fatalf("rule = %v", rule)
	}
	f.expect("POST", webMR+"/merge", "carol", nil, 403, "review_required")
	f.expect("POST", webMR+"/merge", "carol", map[string]any{"bypass": true}, 403, "forbidden")
	mr := f.expect("POST", webMR+"/merge", "alice", map[string]any{"bypass": true}, 200, "")
	if mr["state"] != "merged" {
		t.Fatalf("merge = %v", mr)
	}
}
