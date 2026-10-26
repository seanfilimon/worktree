package httpapi

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// v0.8 conflict resolution (§16.9) over the REST API.

func TestConflictResolutionREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs))
	divergedMR(t, f, objs, true)

	f.expect("GET", divMR+"/conflicts", "dave", nil, 403, "forbidden")
	f.expect("GET", "/api/v1/trees/acme/web/merge-requests/9/conflicts", "vic", nil, 404, "not_found")
	rep := f.expect("GET", divMR+"/conflicts", "vic", nil, 200, "")
	files, _ := rep["files"].([]any)
	if rep["resolvable"] != true || rep["total"] != float64(1) || rep["source_tip"] != "d2" || rep["target_tip"] != "d1" || len(files) != 1 {
		t.Fatalf("report = %v", rep)
	}
	file := files[0].(map[string]any)
	ours, _ := file["ours"].(map[string]any)
	if file["path"] != "a.txt" || file["kind"] != "content" || ours["text"] != "uno\n2\n3\n" ||
		!strings.HasPrefix(fmt.Sprint(file["markers"]), "<<<<<<< topic\nuno\n||||||| base\n1\n=======\nONE\n>>>>>>> trunk\n") {
		t.Fatalf("file = %v", file)
	}

	resolved := map[string]any{"files": []any{map[string]any{"path": "a.txt", "content": "Uno/ONE\n2\n3\n"}}, "source_tip": "d2", "target_tip": "d1"}
	f.expect("POST", divMR+"/resolve", "", resolved, 401, "unauthenticated")
	f.expect("POST", divMR+"/resolve", "vic", resolved, 403, "forbidden")
	f.expect("POST", divMR+"/resolve", "bob", map[string]any{"files": []any{}}, 400, "bad_request")
	f.expect("POST", divMR+"/resolve", "bob", map[string]any{"files": []any{map[string]any{"path": "a.txt", "take": "mine"}}}, 400, "bad_request")
	f.expect("POST", divMR+"/resolve", "bob", map[string]any{"files": []any{map[string]any{"path": "a.txt", "content": "<<<<<<< a\nx\n=======\ny\n>>>>>>> b\n"}}}, 400, "bad_request")
	f.expect("POST", divMR+"/resolve", "bob", map[string]any{"files": []any{map[string]any{"path": "a.txt", "take": "ours"}}, "bogus": 1}, 400, "bad_request")
	f.expect("POST", divMR+"/resolve", "bob", map[string]any{"files": []any{map[string]any{"path": "a.txt", "take": "ours"}}, "source_tip": "d0"}, 409, review.CodeResolveOutdated)

	res := f.expect("POST", divMR+"/resolve", "bob", resolved, 200, "")
	snap, _ := res["snapshot"].(map[string]any)
	if fmt.Sprint(snap["parents"]) != "[d2 d1]" || snap["branch_name"] != "topic" || snap["author"] != "bob" ||
		fmt.Sprint(res["paths"]) != "[a.txt]" || res["merge_request"].(map[string]any)["number"] != float64(3) {
		t.Fatalf("resolved = %v", res)
	}
	b, err := f.store.GetBranch(context.Background(), f.web.ID, "topic")
	if err != nil || b.Tip != snap["id"] {
		t.Fatalf("topic = %+v, %v", b, err)
	}
	// Resolved: no conflicts are left and the merge fast-forwards.
	rep = f.expect("GET", divMR+"/conflicts", "vic", nil, 200, "")
	if rep["resolvable"] != false || rep["total"] != float64(0) || !strings.Contains(fmt.Sprint(rep["reason"]), "no conflicts") {
		t.Fatalf("after = %v", rep)
	}
	f.expect("POST", divMR+"/resolve", "bob", resolved, 409, review.CodeResolveOutdated)
	mb := f.expect("GET", divMR+"/mergeability", "vic", nil, 200, "")
	if mb["fast_forward"] != true {
		t.Fatalf("mergeability = %v", mb)
	}
	mr := f.expect("POST", divMR+"/merge", "carol", map[string]any{"delete_source_branch": true}, 200, "")
	if mr["merged_snapshot"] != snap["id"] || mr["merge_method"] != domain.MergeFastForward {
		t.Fatalf("merged = %v", mr)
	}
	if _, err := f.store.GetBranch(context.Background(), f.web.ID, "topic"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("source branch kept: %v", err)
	}
}

func TestRebaseMergeREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs))
	divergedMR(t, f, objs, false)
	mb := f.expect("GET", divMR+"/mergeability", "vic", nil, 200, "")
	if !strings.Contains(fmt.Sprint(mb["methods"]), "rebase") {
		t.Fatalf("mergeability = %v", mb)
	}
	// delete_source_branch needs branch:delete (developers lack mr:merge too).
	f.expect("POST", divMR+"/merge", "bob", map[string]any{"method": "rebase"}, 403, "forbidden")
	mr := f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "rebase"}, 200, "")
	if mr["merge_method"] != "rebase" || mr["merged_snapshot"] == "d2" {
		t.Fatalf("merged = %v", mr)
	}
	tip, err := f.store.GetSnapshot(context.Background(), fmt.Sprint(mr["merged_snapshot"]))
	if err != nil || len(tip.Parents) != 1 || tip.Parents[0] != "d1" || tip.Message != "d2" {
		t.Fatalf("rebased = %+v, %v", tip, err)
	}
}

func TestConflictResolutionUnavailable(t *testing.T) {
	f := newRVFixture(t) // review service without an object store
	rep := f.expect("GET", webMR+"/conflicts", "vic", nil, 200, "")
	if rep["resolvable"] != false {
		t.Fatalf("report = %v", rep)
	}
	f.expect("POST", webMR+"/resolve", "bob", map[string]any{"files": []any{map[string]any{"path": "a", "delete": true}}}, 409, "conflict")
}
