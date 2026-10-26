package httpapi

import (
	"context"
	"fmt"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// v0.7 deeper review (§15.7) over the REST API.

// denyQuota refuses every plan as over the storage quota.
type denyQuota struct{}

func (denyQuota) CheckPushPlan(context.Context, *domain.Principal, *domain.Tree, *domain.PushPlan) error {
	return &domain.PushDenied{Kind: "quota_exceeded", Reason: "max_storage_bytes"}
}

func TestOwnersREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs))
	divergedMR(t, f, objs, false)
	const owners = webT + "/owners"

	cfg := f.expect("GET", owners, "vic", nil, 200, "")
	if cfg["source"] != "none" || fmt.Sprint(cfg["rules"]) != "[]" || fmt.Sprint(cfg["errors"]) != "[]" {
		t.Fatalf("owners = %v", cfg)
	}
	f.expect("GET", owners, "dave", nil, 403, "forbidden")
	rules := map[string]any{"rules": []map[string]any{{"pattern": "**", "owners": []string{"@carol"}}}}
	f.expect("PUT", owners, "", rules, 401, "unauthenticated")
	f.expect("PUT", owners, "bob", rules, 403, "forbidden")
	f.expect("PUT", owners, "alice", map[string]any{"rules": []map[string]any{{"pattern": "**", "owners": []string{"ghost"}}}}, 400, "bad_request")
	f.expect("PUT", owners, "alice", map[string]any{"rules": []map[string]any{{"pattern": "", "owners": []string{"carol"}}}}, 400, "bad_request")
	cfg = f.expect("PUT", owners, "alice", rules, 200, "")
	if cfg["source"] != "config" || cfg["updated_by"] != "alice" || fmt.Sprint(cfg["rules"]) != "[map[owners:[user:carol] pattern:**]]" {
		t.Fatalf("put owners = %v", cfg)
	}

	own := f.expect("GET", divMR+"/owners", "vic", nil, 200, "")
	if own["required"] != false || own["satisfied"] != false || fmt.Sprint(own["missing"]) != "[a.txt]" ||
		fmt.Sprint(own["paths"]) != "[map[approved:false approved_by:[] owners:[user:carol] path:a.txt]]" {
		t.Fatalf("mr owners = %v", own)
	}
	f.expect("GET", webT+"/merge-requests/9/owners", "vic", nil, 404, "not_found")

	// RequireOwnerApproval blocks the merge until carol approves.
	f.expect("PUT", webT+"/protections", "alice", map[string]any{"pattern": "trunk", "require_owner_approval": true}, 200, "")
	c := f.expect("GET", divMR+"/checks", "vic", nil, 200, "")
	if c["mergeable"] != false || fmt.Sprint(c["owners"]) != "map[missing:[a.txt] required:true]" {
		t.Fatalf("checks = %v", c)
	}
	f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "merge"}, 409, "merge_blocked")
	f.expect("POST", divMR+"/review", "carol", map[string]any{"decision": "approved"}, 200, "")
	if own := f.expect("GET", divMR+"/owners", "vic", nil, 200, ""); own["satisfied"] != true || own["required"] != true {
		t.Fatalf("after approval = %v", own)
	}
	if mr := f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "merge"}, 200, ""); mr["state"] != "merged" {
		t.Fatalf("merged = %v", mr)
	}
	// An empty list removes the configuration.
	if cfg := f.expect("PUT", owners, "alice", map[string]any{"rules": []any{}}, 200, ""); cfg["source"] != "none" {
		t.Fatalf("cleared = %v", cfg)
	}
}

func TestReviewersDraftsREST(t *testing.T) {
	f := newRVFixture(t)
	const rv = webMR + "/reviewers"
	f.expect("POST", rv, "", map[string]any{"reviewers": []string{"vic"}}, 401, "unauthenticated")
	f.expect("POST", rv, "vic", map[string]any{"reviewers": []string{"carol"}}, 403, "forbidden")
	f.expect("POST", rv, "bob", map[string]any{"reviewers": []string{}}, 400, "bad_request")
	f.expect("POST", rv, "bob", map[string]any{"reviewers": []string{"ghost"}}, 400, "bad_request")
	f.expect("POST", rv, "bob", map[string]any{"reviewers": []string{"bob"}}, 400, "bad_request") // author
	f.expect("POST", rv, "bob", map[string]any{"reviewers": []string{"a b"}}, 400, "bad_request")
	mr := f.expect("POST", rv, "bob", map[string]any{"reviewers": []string{"@vic", "user:carol"}}, 200, "")
	if fmt.Sprint(mr["reviewers"]) != "[user:vic user:carol]" {
		t.Fatalf("reviewers = %v", mr)
	}
	f.expect("DELETE", rv+"?reviewer=vic", "vic", nil, 403, "forbidden")
	mr = f.expect("DELETE", rv+"?reviewer=user:vic", "carol", nil, 200, "")
	if fmt.Sprint(mr["reviewers"]) != "[user:carol]" {
		t.Fatalf("after delete = %v", mr)
	}
	mr = f.expect("DELETE", rv, "bob", map[string]any{"reviewers": []string{"carol"}}, 200, "")
	if _, ok := mr["reviewers"]; ok {
		t.Fatalf("no reviewers left = %v", mr)
	}
	f.expect("DELETE", rv, "bob", nil, 400, "bad_request")

	// Drafts.
	mrs := webT + "/merge-requests"
	d := f.expect("POST", mrs, "bob", map[string]any{"title": "WIP", "source_branch": "feature2", "target_branch": "feature",
		"draft": true, "reviewers": []string{"carol"}}, 201, "")
	if d["draft"] != true || fmt.Sprint(d["reviewers"]) != "[user:carol]" {
		t.Fatalf("draft = %v", d)
	}
	path := fmt.Sprintf("%s/%v", mrs, d["number"])
	f.expect("POST", path+"/merge", "carol", nil, 409, "draft")
	if c := f.expect("GET", path+"/checks", "vic", nil, 200, ""); fmt.Sprint(c["blockers"]) == "[]" {
		t.Fatalf("draft checks = %v", c)
	}
	f.expect("POST", path+"/ready", "vic", nil, 403, "forbidden")
	f.expect("POST", mrs+"/9/ready", "bob", nil, 404, "not_found")
	ready := f.expect("POST", path+"/ready", "bob", nil, 200, "")
	if _, ok := ready["draft"]; ok {
		t.Fatalf("ready = %v", ready)
	}
	f.expect("POST", path+"/merge", "carol", nil, 200, "")
	f.expect("POST", path+"/ready", "bob", nil, 409, "conflict") // merged
}

func TestSuggestionsREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs))
	divergedMR(t, f, objs, false) // topic: d2, a.txt = 1 2 THREE
	const comments = divMR + "/comments"

	f.expect("POST", comments, "carol", map[string]any{"suggestion": map[string]any{"path": "a.txt", "start_line": 2, "end_line": 1}}, 400, "bad_request")
	f.expect("POST", comments, "carol", map[string]any{"suggestion": map[string]any{"path": "a.txt", "start_line": 3, "end_line": 4}}, 400, "bad_request")
	f.expect("POST", comments, "carol", map[string]any{"suggestion": map[string]any{"path": "x.txt", "start_line": 1, "end_line": 1}}, 400, "bad_request")
	f.expect("POST", comments, "carol", map[string]any{"body": ""}, 400, "bad_request")
	c := f.expect("POST", comments, "carol", map[string]any{"suggestion": map[string]any{"path": "a.txt", "start_line": 1, "end_line": 1, "replacement": "uno"}}, 201, "")
	sg, _ := c["suggestion"].(map[string]any)
	if c["path"] != "a.txt" || c["line"] != float64(1) || sg["snapshot_id"] != "d2" || sg["replacement"] != "uno" {
		t.Fatalf("comment = %v", c)
	}
	c3 := f.expect("POST", comments, "carol", map[string]any{"body": "and this", "suggestion": map[string]any{"path": "a.txt", "start_line": 3, "end_line": 3, "replacement": "three"}}, 201, "")
	plain := f.expect("POST", comments, "carol", map[string]any{"body": "plain"}, 201, "")

	apply := func(id any) string { return fmt.Sprintf("%s/%v/apply", comments, id) }
	f.expect("POST", apply(c["id"]), "", nil, 401, "unauthenticated")
	f.expect("POST", apply(c["id"]), "vic", nil, 403, "forbidden")
	f.expect("POST", apply(plain["id"]), "bob", nil, 400, "bad_request")
	f.expect("POST", apply(77), "bob", nil, 404, "not_found")
	res := f.expect("POST", apply(c["id"]), "bob", map[string]any{"message": "Apply carol's idea"}, 200, "")
	snap, _ := res["snapshot"].(map[string]any)
	com, _ := res["comment"].(map[string]any)
	if snap["message"] != "Apply carol's idea" || fmt.Sprint(snap["parents"]) != "[d2]" || snap["author"] != "bob" || com["resolved"] != true {
		t.Fatalf("applied = %v", res)
	}
	if b, _ := f.store.GetBranch(context.Background(), f.web.ID, "topic"); b.Tip != snap["id"] {
		t.Fatalf("topic tip = %s", b.Tip)
	}
	f.expect("POST", apply(c["id"]), "bob", nil, 409, "conflict")

	// The source changes line 3: the second suggestion is outdated.
	commitFiles(t, f, objs, f.web.ID, "d3", map[string]string{"a.txt": "uno\n2\nTRES\n"}, snap["id"].(string))
	must(t, f.store.PutBranch(context.Background(), &domain.Branch{TreeID: f.web.ID, Name: "topic", Tip: "d3"}))
	f.expect("POST", apply(c3["id"]), "bob", nil, 409, "suggestion_outdated")
}

func TestQuotaOnMergeREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs), func(d *Deps) { d.Review.(*review.Service).SetQuota(denyQuota{}) })
	divergedMR(t, f, objs, false)
	f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "merge"}, 409, "quota_exceeded")
	f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "squash"}, 409, "quota_exceeded")
	c := f.expect("POST", divMR+"/comments", "carol", map[string]any{"suggestion": map[string]any{"path": "a.txt", "start_line": 1, "end_line": 1, "replacement": "x"}}, 201, "")
	f.expect("POST", fmt.Sprintf("%s/comments/%v/apply", divMR, c["id"]), "bob", nil, 409, "quota_exceeded")
}
