package httpapi

// history-v2 (§16.10): GET {tree}/reflog, POST {tree}/reflog/restore and
// the merge bypass over REST.

import (
	"context"
	"fmt"
	"strings"
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

// withRestorer wires a real sync service as Deps.Reflog.
func withRestorer(d *Deps) {
	d.Reflog = syncsvc.New(syncsvc.Deps{Store: d.Store, Objects: d.Objects, History: d.History, Authz: d.Authz, Review: d.Review})
}

func TestReflogRestoreEndpoint(t *testing.T) {
	f := newRVFixture(t, withRestorer)
	ctx := context.Background()
	// main went w1 -> w2 -> w3 through two pushes.
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "main", Tip: "w3"}))
	t0 := time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)
	for i, e := range []syncsvc.ReflogEntry{
		{Branch: "main", OldTip: "w1", NewTip: "w2", Kind: "push", Actor: "bob", Count: 1},
		{Branch: "main", OldTip: "w2", NewTip: "w3", Kind: "push", Actor: "bob", Count: 1},
	} {
		e.TreeID = f.web.ID
		e.At = t0.Add(time.Duration(i) * time.Minute)
		key := domain.DocKey(f.web.ID, e.Branch, fmt.Sprintf("%020d", e.At.UnixNano()), "00")
		must(t, f.store.PutDoc(ctx, syncsvc.ReflogCollection, key, &e))
	}
	path := webT + "/reflog/restore"
	body := func(tip, expected string) map[string]any {
		return map[string]any{"branch": "main", "tip": tip, "expected_tip": expected}
	}

	f.expect("POST", path, "", body("w2", "w3"), 401, "unauthenticated")
	f.expect("POST", path, "dave", body("w2", "w3"), 403, "forbidden")
	f.expect("POST", path, "bob", body("w2", "w3"), 403, "forbidden")   // developer: no branch:force_push
	f.expect("POST", path, "carol", body("w2", "w3"), 403, "forbidden") // maintainer: no branch:force_push
	f.expect("POST", path, "alice", map[string]any{"branch": "main", "tip": "w2"}, 400, "bad_request")
	f.expect("POST", path, "alice", map[string]any{"branch": "main", "tip": "w2", "expected_tip": "w3", "extra": 1}, 400, "bad_request")
	f.expect("POST", path, "alice", body("a1", "w3"), 400, "bad_request") // not in main's reflog
	stale := f.expect("POST", path, "alice", body("w2", "w2"), 409, "conflict")
	if d := stale["error"].(map[string]any)["details"].(map[string]any); d["current_tip"] != "w3" || d["expected_tip"] != "w2" {
		t.Fatalf("conflict details = %v", d)
	}

	res := f.expect("POST", path, "alice", body("w1", "w3"), 200, "")
	if res["old_tip"] != "w3" || res["new_tip"] != "w1" || res["forced"] != true || fmt.Sprint(res["bypassed"]) != "[]" {
		t.Fatalf("restore = %v", res)
	}
	if e := res["entry"].(map[string]any); e["kind"] != "restore" || e["actor"] != "alice" {
		t.Fatalf("entry = %v", e)
	}
	if b, _ := f.store.GetBranch(ctx, f.web.ID, "main"); b.Tip != "w1" {
		t.Fatalf("main = %s", b.Tip)
	}
	newest := f.expect("GET", webT+"/reflog?branch=main&limit=1", "vic", nil, 200, "")["entries"].([]any)[0].(map[string]any)
	if newest["kind"] != "restore" || newest["old_tip"] != "w3" {
		t.Fatalf("reflog = %v", newest)
	}

	// Protection: no_force_push refuses rewinds, even with the bypass.
	f.expect("POST", path, "alice", body("w3", "w1"), 200, "") // forward: back to w3
	must(t, f.store.PutProtection(ctx, &domain.BranchProtection{TreeID: f.web.ID, Pattern: "main", NoForcePush: true}))
	denied := f.expect("POST", path, "alice", map[string]any{"branch": "main", "tip": "w1", "expected_tip": "w3", "bypass": true}, 403, "branch_protection")
	if msg := denied["error"].(map[string]any)["message"].(string); !strings.Contains(msg, "no_force_push") {
		t.Fatalf("message = %q", msg)
	}
}

func TestReflogRestoreNotImplemented(t *testing.T) {
	f := newRVFixture(t)
	f.expect("POST", webT+"/reflog/restore", "alice", map[string]any{"branch": "main", "tip": "w1", "expected_tip": "w1"}, 501, "not_implemented")
}
