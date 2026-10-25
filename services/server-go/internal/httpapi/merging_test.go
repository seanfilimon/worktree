package httpapi

import (
	"context"
	"fmt"
	"slices"
	"sort"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// v0.6 server-side merges (§14.1) over the REST API.

// withMerges wires a review service that can write merge snapshots into the
// fixture's object store (returned through objs).
func withMerges(objs **fakeObjects) func(*Deps) {
	return func(d *Deps) {
		o := newFakeObjects()
		d.Objects = o
		rv := review.New(d.Store, d.History, d.Authz, nil, nil)
		rv.SetObjects(o)
		d.Review = rv
		*objs = o
	}
}

// commitFiles stores a snapshot of tree with files and returns its id.
func commitFiles(t *testing.T, f *rvFixture, objs *fakeObjects, tree, id string, files map[string]string, parents ...string) string {
	t.Helper()
	m := &syncpb.ManifestRecord{}
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	for _, p := range paths {
		h := objs.put([]byte(files[p]))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(files[p]))})
	}
	raw, err := proto.Marshal(m)
	must(t, err)
	mh := objs.put(raw)
	must(t, f.store.PutSnapshot(context.Background(), &domain.Snapshot{ID: id, TreeID: tree, Parents: parents, ManifestHash: mh, Message: id, CreatedAt: t0.Add(time.Hour)}))
	return id
}

// divergedMR opens bob's MR !3 of web from "topic" into "trunk"; conflict
// makes both sides edit the same line.
func divergedMR(t *testing.T, f *rvFixture, objs *fakeObjects, conflict bool) {
	t.Helper()
	ctx := context.Background()
	commitFiles(t, f, objs, f.web.ID, "d0", map[string]string{"a.txt": "1\n2\n3\n"})
	commitFiles(t, f, objs, f.web.ID, "d1", map[string]string{"a.txt": "ONE\n2\n3\n"}, "d0")
	theirs := "1\n2\nTHREE\n"
	if conflict {
		theirs = "uno\n2\n3\n"
	}
	commitFiles(t, f, objs, f.web.ID, "d2", map[string]string{"a.txt": theirs}, "d0")
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "trunk", Tip: "d1"}))
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: f.web.ID, Name: "topic", Tip: "d2"}))
	mr := &domain.MergeRequest{TreeID: f.web.ID, Title: "Topic", SourceBranch: "topic", TargetBranch: "trunk",
		AuthorID: "acct-bob", Author: "bob", State: domain.MRStateOpen, CreatedAt: t0}
	must(t, f.store.CreateMergeRequest(ctx, mr))
	if mr.Number != 3 {
		t.Fatalf("number = %d", mr.Number)
	}
}

const divMR = "/api/v1/trees/acme/web/merge-requests/3"

func TestMergeMethodsREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs))
	divergedMR(t, f, objs, false)

	mb := f.expect("GET", divMR+"/mergeability", "vic", nil, 200, "")
	if mb["fast_forward"] != false || fmt.Sprint(mb["methods"]) != "[merge squash]" || mb["default"] != "merge" ||
		mb["merge_base"] != "d0" || fmt.Sprint(mb["conflicts"]) != "[]" {
		t.Fatalf("mergeability = %v", mb)
	}
	f.expect("GET", divMR+"/mergeability", "dave", nil, 403, "forbidden")
	f.expect("GET", "/api/v1/trees/acme/web/merge-requests/9/mergeability", "vic", nil, 404, "not_found")

	// Readiness agrees: a non-fast-forwardable MR is mergeable with merge.
	c := f.expect("GET", divMR+"/checks", "vic", nil, 200, "")
	if c["mergeable"] != true || fmt.Sprint(c["methods"]) != "[merge squash]" || fmt.Sprint(c["blockers"]) != "[]" {
		t.Fatalf("checks = %v", c)
	}

	f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "rebase"}, 400, "bad_request")
	f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "squash", "extra": 1}, 400, "bad_request")
	f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "fast-forward"}, 409, "conflict")

	// Protection restricts the methods.
	const prot = "/api/v1/trees/acme/web/protections"
	f.expect("PUT", prot, "alice", map[string]any{"pattern": "trunk", "merge_methods": []string{"octopus"}}, 400, "bad_request")
	rule := f.expect("PUT", prot, "alice", map[string]any{"pattern": "trunk", "merge_methods": []string{"squash", " fast-forward", "squash"}}, 200, "")
	if fmt.Sprint(rule["merge_methods"]) != "[fast-forward squash]" {
		t.Fatalf("rule = %v", rule)
	}
	f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "merge"}, 409, "merge_method_not_allowed")
	mb = f.expect("GET", divMR+"/mergeability", "vic", nil, 200, "")
	if fmt.Sprint(mb["allowed"]) != "[fast-forward squash]" || fmt.Sprint(mb["methods"]) != "[squash]" {
		t.Fatalf("restricted mergeability = %v", mb)
	}

	f.expect("POST", divMR+"/merge", "bob", map[string]any{"method": "squash"}, 403, "forbidden")
	mr := f.expect("POST", divMR+"/merge", "carol", map[string]any{"method": "squash", "message": "Squashed topic"}, 200, "")
	if mr["state"] != "merged" || mr["merge_method"] != "squash" {
		t.Fatalf("merged = %v", mr)
	}
	snap, err := f.store.GetSnapshot(context.Background(), mr["merged_snapshot"].(string))
	must(t, err)
	if snap.Message != "Squashed topic" || !slices.Equal(snap.Parents, []string{"d1"}) || snap.Author != "carol" || snap.PushedBy != "acct-carol" {
		t.Fatalf("snapshot = %+v", snap)
	}
	if b, _ := f.store.GetBranch(context.Background(), f.web.ID, "trunk"); b.Tip != snap.ID {
		t.Fatalf("trunk = %s", b.Tip)
	}
	// The merged content is served like any snapshot's.
	raw, err := objs.Get(context.Background(), snap.ManifestHash)
	must(t, err)
	var m syncpb.ManifestRecord
	must(t, proto.Unmarshal(raw, &m))
	var h hash.Hash
	copy(h[:], m.Entries[0].GetBlobHash().GetValue())
	if data, _ := objs.Get(context.Background(), h); string(data) != "ONE\n2\nTHREE\n" {
		t.Fatalf("merged a.txt = %q", data)
	}
}

func TestMergeConflictREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs))
	divergedMR(t, f, objs, true)

	mb := f.expect("GET", divMR+"/mergeability", "vic", nil, 200, "")
	if fmt.Sprint(mb["methods"]) != "[]" || fmt.Sprint(mb["conflicts"]) != "[map[kind:content path:a.txt]]" {
		t.Fatalf("mergeability = %v", mb)
	}
	c := f.expect("GET", divMR+"/checks", "vic", nil, 200, "")
	if c["mergeable"] != false || fmt.Sprint(c["blockers"]) == "[]" {
		t.Fatalf("checks = %v", c)
	}
	for _, body := range []any{nil, map[string]any{}, map[string]any{"method": "merge"}} {
		res := f.expect("POST", divMR+"/merge", "carol", body, 409, "merge_conflict")
		want := "[map[kind:content path:a.txt]]"
		e, _ := res["error"].(map[string]any)
		if fmt.Sprint(res["conflicts"]) != want || fmt.Sprint(e["conflicts"]) != want {
			t.Fatalf("body %v: response = %v", body, res)
		}
	}
	if b, _ := f.store.GetBranch(context.Background(), f.web.ID, "trunk"); b.Tip != "d1" {
		t.Fatalf("trunk moved to %s", b.Tip)
	}
}

func TestLinkedGroupMergeMethodREST(t *testing.T) {
	var objs *fakeObjects
	f := newRVFixture(t, withMerges(&objs))
	const groups = "/api/v1/tenants/acme/linked-groups"
	g := f.expect("POST", groups, "carol", map[string]any{"name": "g", "members": []map[string]any{
		{"tree": "acme/web", "number": 1}, {"tree": "acme/api", "number": 1},
	}}, 201, "")
	id := g["id"].(string)
	f.expect("POST", groups+"/"+id+"/merge", "carol", map[string]any{"method": "cherry-pick"}, 400, "bad_request")
	f.expect("PUT", "/api/v1/trees/acme/api/protections", "alice", map[string]any{"pattern": "main", "merge_methods": []string{"squash"}}, 200, "")
	f.expect("POST", groups+"/"+id+"/merge", "carol", map[string]any{"method": "fast-forward"}, 409, "merge_blocked")
	f.expect("DELETE", "/api/v1/trees/acme/api/protections?pattern=main", "alice", nil, 204, "")
	merged := f.expect("POST", groups+"/"+id+"/merge", "carol", map[string]any{"method": "fast-forward"}, 200, "")
	if merged["state"] != "merged" {
		t.Fatalf("group = %v", merged)
	}
	for _, tr := range []domain.Tree{f.web, f.api} {
		mr, err := f.store.GetMergeRequest(context.Background(), tr.ID, 1)
		must(t, err)
		if mr.MergeMethod != domain.MergeFastForward {
			t.Fatalf("%s: method = %q", tr.Name, mr.MergeMethod)
		}
	}
}

// plainReview (reviews_test.go) hides the v0.6 merge surface too.
func TestMergeMethodsWithoutServerMerges(t *testing.T) {
	f := newRVFixture(t, func(d *Deps) { d.Review = plainReview{d.Review} })
	f.expect("GET", webMR+"/mergeability", "vic", nil, 501, "not_implemented")
	f.expect("POST", webMR+"/merge", "carol", map[string]any{"method": "squash"}, 501, "not_implemented")
	mr := f.expect("POST", webMR+"/merge", "carol", map[string]any{"method": "fast-forward"}, 200, "")
	if mr["state"] != "merged" {
		t.Fatalf("mr = %v", mr)
	}
}
