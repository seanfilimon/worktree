package httpapi

import (
	"context"
	"io"
	"log/slog"
	"net/http"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork/snapcopy"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// forkFixture: acme/web (internal) with main s1 <- s2; alice owns acme, bob
// is a viewer of acme, carol has no role. Real store, authorizer, review
// and fork services.
type forkFixture struct {
	*rvFixture
	objs *fakeObjects
	web  domain.Tree
}

func newForkFixture(t *testing.T, withForks bool) *forkFixture {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	objs := newFakeObjects()
	acme := domain.Tenant{ID: "tenant-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}
	must(t, store.CreateTenant(ctx, &acme))
	users := map[string]*domain.Principal{}
	for u, role := range map[string]string{"alice": domain.RoleOwner, "bob": domain.RoleViewer, "carol": ""} {
		ten := domain.Tenant{ID: "tenant-" + u, Slug: u, Name: u, Kind: domain.TenantUser, CreatedAt: t0}
		must(t, store.CreateTenant(ctx, &ten))
		acct := domain.Account{ID: "acct-" + u, Username: u, Email: u + "@example.com", TenantID: ten.ID, CreatedAt: t0}
		must(t, store.CreateAccount(ctx, &acct))
		must(t, store.PutMembership(ctx, &domain.Membership{TenantID: ten.ID, AccountID: acct.ID, Role: domain.RoleOwner, CreatedAt: t0}))
		if role != "" {
			must(t, store.PutMembership(ctx, &domain.Membership{TenantID: acme.ID, AccountID: acct.ID, Role: role, CreatedAt: t0}))
		}
		users[u] = &domain.Principal{AccountID: acct.ID, Username: u, TenantID: ten.ID}
	}
	web := domain.Tree{ID: "tree-web", TenantID: acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityInternal, CreatedAt: t0}
	must(t, store.CreateTree(ctx, &web))
	m1 := manifest(t, objs, map[string]string{"README.md": "# web\n"})
	m2 := manifest(t, objs, map[string]string{"README.md": "# web 2\n", "secret/key.txt": "k"})
	must(t, store.PutSnapshot(ctx, &domain.Snapshot{ID: "s1", TreeID: web.ID, ManifestHash: m1, Message: "s1", CreatedAt: t0}))
	must(t, store.PutSnapshot(ctx, &domain.Snapshot{ID: "s2", TreeID: web.ID, Parents: []string{"s1"}, ManifestHash: m2, Message: "s2", CreatedAt: t0.Add(time.Minute)}))
	must(t, store.PutBranch(ctx, &domain.Branch{TreeID: web.ID, Name: "main", Tip: "s2"}))

	az := authz.New(store, authz.Options{})
	hist := meta.NewHistory(store)
	logger := slog.New(slog.NewTextHandler(io.Discard, nil))
	rev := review.New(store, hist, az, nil, nil)
	rev.SetObjects(objs)
	d := Deps{
		Store: store, Objects: objs, History: hist, Auth: tokenAuth{users: users}, Authz: az,
		Review: rev, License: license.New(store), Logger: logger,
	}
	if withForks {
		svc := fork.New(fork.Deps{Store: store, Objects: objs, History: hist, Authz: az, Logger: logger})
		svc.Configure(fork.Options{License: license.New(store)})
		d.Forks = svc
	}
	return &forkFixture{rvFixture: &rvFixture{t: t, store: store, h: New(d)}, objs: objs, web: web}
}

func TestForkRoutes(t *testing.T) {
	f := newForkFixture(t, true)
	ctx := context.Background()
	const up = "/api/v1/trees/acme/web"

	f.expect("POST", up+"/forks", "", map[string]any{}, http.StatusUnauthorized, "")
	f.expect("POST", up+"/forks", "carol", map[string]any{}, http.StatusForbidden, "forbidden")
	f.expect("POST", up+"/forks", "bob", map[string]any{"branches": []string{"no..pe"}}, http.StatusBadRequest, "")

	// The license redistribution rule refuses forks out of the tenant.
	must(t, f.store.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: f.web.ID, PathGlob: "secret/**", SPDX: "LicenseRef-Proprietary"}))
	f.expect("POST", up+"/forks", "bob", map[string]any{}, http.StatusForbidden, "license_violation")
	must(t, f.store.DeleteLicenseRule(ctx, f.web.ID, "secret/**"))

	created := f.expect("POST", up+"/forks", "bob", map[string]any{"description": "bob's copy"}, http.StatusCreated, "")
	if created["fork_of"] != f.web.ID || created["tenant_id"] != "tenant-bob" || created["name"] != "web" || created["description"] != "bob's copy" {
		t.Fatalf("created = %v", created)
	}
	forkID, _ := created["id"].(string)
	f.expect("POST", up+"/forks", "bob", map[string]any{}, http.StatusConflict, "conflict")
	f.expect("POST", up+"/forks", "bob", map[string]any{"tenant": "acme", "name": "x"}, http.StatusForbidden, "forbidden")

	_, _, list := f.call("GET", up+"/forks", "alice", nil)
	if len(list) != 1 || list[0].(map[string]any)["tenant"] != "bob" {
		t.Fatalf("forks = %v", list)
	}
	upInfo := f.expect("GET", "/api/v1/trees/bob/web/fork", "bob", nil, http.StatusOK, "")
	if upInfo["fork"] != true || upInfo["tenant"] != "acme" || upInfo["name"] != "web" || upInfo["upstream_missing"] != false {
		t.Fatalf("upstream = %v", upInfo)
	}
	if notFork := f.expect("GET", up+"/fork", "alice", nil, http.StatusOK, ""); notFork["fork"] != false {
		t.Fatalf("upstream of the upstream = %v", notFork)
	}

	// The upstream moves; bob syncs his fork (alice cannot write it).
	m3 := manifest(t, f.objs, map[string]string{"README.md": "# web 3\n"})
	must(t, f.store.PutSnapshot(ctx, &domain.Snapshot{ID: "s3", TreeID: f.web.ID, Parents: []string{"s2"}, ManifestHash: m3, Message: "s3", CreatedAt: t0.Add(2 * time.Minute)}))
	must(t, f.store.CompareAndSwapBranchTip(ctx, f.web.ID, "main", "s2", "s3"))
	f.expect("POST", "/api/v1/trees/bob/web/fork/sync", "alice", nil, http.StatusForbidden, "forbidden")
	res := f.expect("POST", "/api/v1/trees/bob/web/fork/sync", "bob", nil, http.StatusOK, "")
	if res["updated"] != float64(1) {
		t.Fatalf("sync = %v", res)
	}
	f.expect("POST", up+"/fork/sync", "alice", nil, http.StatusBadRequest, "")

	// bob works in his fork and opens a merge request upstream (he is only
	// a viewer of acme: tree:fork there plus tree:write on the fork).
	forkTip := snapcopy.DerivedID(forkID, "s3")
	m4 := manifest(t, f.objs, map[string]string{"README.md": "# web 3\n", "fork.txt": "hi\n"})
	must(t, f.store.PutSnapshot(ctx, &domain.Snapshot{ID: "f4", TreeID: forkID, Parents: []string{forkTip}, ManifestHash: m4, Message: "f4", CreatedAt: t0.Add(3 * time.Minute)}))
	must(t, f.store.PutBranch(ctx, &domain.Branch{TreeID: forkID, Name: "main", Tip: "f4"}))
	f.expect("POST", up+"/merge-requests", "bob", map[string]any{"title": "x", "source_branch": "main", "target_branch": "main", "source_tenant": "bob"},
		http.StatusBadRequest, "")
	f.expect("POST", up+"/merge-requests", "bob", map[string]any{"title": "x", "source_branch": "main", "target_branch": "main", "source_tenant": "bob", "source_tree": "nope"},
		http.StatusNotFound, "")
	mr := f.expect("POST", up+"/merge-requests", "bob", map[string]any{
		"title": "From my fork", "source_branch": "main", "target_branch": "main", "source_tenant": "bob", "source_tree": "web",
	}, http.StatusCreated, "")
	if mr["source_tree_id"] != forkID || mr["number"] != float64(1) {
		t.Fatalf("merge request = %v", mr)
	}
	src := f.expect("GET", up+"/merge-requests/1/source", "alice", nil, http.StatusOK, "")
	want := snapcopy.DerivedID(f.web.ID, "f4")
	if src["cross_tree"] != true || src["tip"] != "f4" || src["target_tip"] != want || src["tenant"] != "bob" || src["tree"] != "web" {
		t.Fatalf("source = %v", src)
	}
	// The diff of the merge request works on the copied tip.
	if cmp := f.expect("GET", up+"/compare?from=s3&to="+want, "alice", nil, http.StatusOK, ""); cmp == nil {
		t.Fatal("compare failed")
	}
	merged := f.expect("POST", up+"/merge-requests/1/merge", "alice", nil, http.StatusOK, "")
	if merged["state"] != "merged" || merged["merged_snapshot"] != want {
		t.Fatalf("merged = %v", merged)
	}
	b, err := f.store.GetBranch(ctx, f.web.ID, "main")
	must(t, err)
	if b.Tip != want {
		t.Fatalf("upstream main = %s, want %s", b.Tip, want)
	}
	same := f.expect("GET", up+"/merge-requests/1/source", "alice", nil, http.StatusOK, "")
	if same["cross_tree"] != true {
		t.Fatalf("source after merge = %v", same)
	}
}

func TestForkRoutesDisabled(t *testing.T) {
	f := newForkFixture(t, false)
	const up = "/api/v1/trees/acme/web"
	f.expect("GET", up+"/forks", "alice", nil, http.StatusNotImplemented, "not_implemented")
	f.expect("POST", up+"/forks", "alice", map[string]any{}, http.StatusNotImplemented, "not_implemented")
	f.expect("GET", up+"/fork", "alice", nil, http.StatusNotImplemented, "not_implemented")
	f.expect("POST", up+"/fork/sync", "alice", nil, http.StatusNotImplemented, "not_implemented")
	// Readability is checked first: no 501 oracle for hidden trees.
	f.expect("GET", "/api/v1/trees/acme/nope/forks", "alice", nil, http.StatusNotFound, "not_found")
}
