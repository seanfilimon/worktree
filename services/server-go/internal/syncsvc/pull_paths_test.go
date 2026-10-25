package syncsvc

import (
	"context"
	"slices"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/pathacl"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// pathWorld runs the sync service over the real metadata store, authorizer
// and license checker (docs/ENGINEERING_PLAN.md §13.1):
//
//	tenant acme: alice owner, bob developer; olga is an outsider
//	tree acme/repo (public), policies: bob may not read secret/** nor
//	write infra/**; license: vendor/** is proprietary.
type pathWorld struct {
	t       *testing.T
	svc     *Service
	store   *meta.Memory
	objects *objstore.DiskStore
	audit   *fakeAudit
	treeID  string
	users   map[string]*domain.Principal
}

func newPathWorld(t *testing.T) *pathWorld {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	now := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	check := func(err error) {
		t.Helper()
		if err != nil {
			t.Fatal(err)
		}
	}
	w := &pathWorld{t: t, store: store, objects: objstore.New(t.TempDir()), audit: &fakeAudit{}, users: map[string]*domain.Principal{}}
	check(store.CreateTenant(ctx, &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: now}))
	for _, u := range []struct{ name, role string }{{"alice", domain.RoleOwner}, {"bob", domain.RoleDeveloper}, {"olga", ""}} {
		check(store.CreateTenant(ctx, &domain.Tenant{ID: "t-" + u.name, Slug: u.name, Name: u.name, Kind: domain.TenantUser, CreatedAt: now}))
		check(store.CreateAccount(ctx, &domain.Account{ID: "a-" + u.name, Username: u.name, Email: u.name + "@example.com", TenantID: "t-" + u.name, CreatedAt: now}))
		if u.role != "" {
			check(store.PutMembership(ctx, &domain.Membership{TenantID: "t-acme", AccountID: "a-" + u.name, Role: u.role, CreatedAt: now}))
		}
		w.users[u.name] = &domain.Principal{AccountID: "a-" + u.name, Username: u.name, TenantID: "t-" + u.name}
	}
	w.treeID = newID()
	check(store.CreateTree(ctx, &domain.Tree{ID: w.treeID, TenantID: "t-acme", Name: "repo", DefaultBranch: "main", Visibility: domain.VisibilityPublic, CreatedAt: now, UpdatedAt: now}))
	check(store.PutBranch(ctx, &domain.Branch{TreeID: w.treeID, Name: "main", CreatedAt: now, UpdatedAt: now}))
	check(store.CreatePolicy(ctx, &domain.Policy{TenantID: "t-acme", Name: "hide-secrets", Enabled: true, Version: 1, CreatedAt: now, UpdatedAt: now,
		Statements: []domain.PolicyStatement{{Effect: domain.EffectDeny, Actions: []string{"tree:read"}, Resources: []string{"wt:path/acme/repo/secret/**"}, Principals: []string{"user:bob"}}}}))
	check(store.CreatePolicy(ctx, &domain.Policy{TenantID: "t-acme", Name: "freeze-infra", Enabled: true, Version: 1, CreatedAt: now, UpdatedAt: now,
		Statements: []domain.PolicyStatement{{Effect: domain.EffectDeny, Actions: []string{"tree:write"}, Resources: []string{"wt:path/acme/repo/infra/**"}, Principals: []string{"user:bob"}}}}))
	check(store.PutLicenseRule(ctx, &domain.LicenseRule{TreeID: w.treeID, PathGlob: "vendor/**", SPDX: "LicenseRef-Proprietary"}))

	az := authz.New(store, authz.Options{})
	w.svc = New(Deps{
		Store:      store,
		Objects:    w.objects,
		History:    meta.NewHistory(store),
		Authz:      az,
		License:    license.New(store),
		Audit:      w.audit,
		PushChecks: []domain.PushChecker{pathacl.New(pathacl.Deps{Store: store, Authz: az})},
	})
	return w
}

func (w *pathWorld) ctx(user string) context.Context {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	w.t.Cleanup(cancel)
	if user == "" {
		return ctx
	}
	return iam.WithPrincipal(ctx, w.users[user])
}

func (w *pathWorld) putObjects(cs ...commit) {
	w.t.Helper()
	for _, c := range cs {
		for _, o := range c.objects {
			if err := w.objects.Put(context.Background(), hash.Hash(o.GetHash().GetValue()), o.GetData()); err != nil {
				w.t.Fatal(err)
			}
		}
	}
}

func (w *pathWorld) push(user, oldTip string, cs ...commit) *syncpb.PushResponse {
	w.t.Helper()
	w.putObjects(cs...)
	resp, err := w.svc.Push(w.ctx(user), pushReq(w.treeID, "main", oldTip, cs...))
	if err != nil {
		w.t.Fatalf("push: %v", err)
	}
	return resp
}

func (w *pathWorld) pull(user string) *syncpb.PullResponse {
	w.t.Helper()
	resp, err := w.svc.Pull(w.ctx(user), &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: w.treeID}})
	if err != nil {
		w.t.Fatalf("pull as %q: %v", user, err)
	}
	return resp
}

func blobHex(s string) string { return hash.Sum([]byte(s)).Hex() }

func TestPullStubsRestrictedPaths(t *testing.T) {
	w := newPathWorld(t)
	c1 := makeCommit(w.treeID, map[string]string{
		"README.md":      "readme",
		"secret/key.txt": "key-v1",
		"vendor/lib.c":   "lib",
		"infra/main.tf":  "tf",
	})
	c2 := makeCommit(w.treeID, map[string]string{
		"README.md":      "readme",
		"secret/key.txt": "key-v2",
		"secret/old.txt": "same-as-public", // shares content with a readable path
		"public.txt":     "same-as-public",
		"vendor/lib.c":   "lib",
		"infra/main.tf":  "tf",
	}, c1)
	if resp := w.push("alice", "", c1, c2); !resp.GetAccepted() {
		t.Fatalf("alice push rejected: %v", resp.GetRejectionReason())
	}

	cases := []struct {
		user    string
		stubbed []string
		absent  []string // blobs left out
		present []string
		audited bool
	}{
		{user: "alice", present: []string{"readme", "key-v1", "key-v2", "lib", "tf", "same-as-public"}},
		{user: "bob", stubbed: []string{"secret/key.txt", "secret/old.txt"}, absent: []string{"key-v1", "key-v2"}, present: []string{"readme", "lib", "tf", "same-as-public"}},
		{user: "olga", stubbed: []string{"vendor/lib.c"}, absent: []string{"lib"}, present: []string{"readme", "key-v1", "key-v2", "tf"}, audited: true},
		{user: "", stubbed: []string{"vendor/lib.c"}, absent: []string{"lib"}, present: []string{"readme"}, audited: true},
	}
	for _, tc := range cases {
		t.Run("as "+tc.user, func(t *testing.T) {
			before := len(w.audit.actions("license.denied"))
			resp := w.pull(tc.user)
			if !resp.GetHasUpdates() || len(resp.GetSnapshots()) != 2 {
				t.Fatalf("pull = %v", resp)
			}
			if got := resp.GetStubbedPaths(); !slices.Equal(got, tc.stubbed) {
				t.Fatalf("stubbed_paths = %v, want %v", got, tc.stubbed)
			}
			objs := hexes(resp.GetObjectsToFetch())
			for _, s := range tc.absent {
				if objs[blobHex(s)] {
					t.Errorf("restricted blob %q is in objects_to_fetch", s)
				}
			}
			for _, s := range tc.present {
				if !objs[blobHex(s)] {
					t.Errorf("readable blob %q is missing from objects_to_fetch", s)
				}
			}
			if !objs[c1.manifest.Hex()] || !objs[c2.manifest.Hex()] {
				t.Error("manifests are missing from objects_to_fetch")
			}
			evs := w.audit.actions("license.denied")
			if !tc.audited {
				if len(evs) != before {
					t.Fatalf("unexpected license.denied audit: %+v", evs[before:])
				}
				return
			}
			if len(evs) != before+1 {
				t.Fatalf("license.denied audited %d times, want once", len(evs)-before)
			}
			e := evs[len(evs)-1]
			if e.Details["count"] != "1" || e.Details["path"] != "vendor/lib.c" || e.ResourceID != w.treeID || e.TenantID != "t-acme" {
				t.Fatalf("audit = %+v", e)
			}
		})
	}
}

func TestPullStubbedPathsOnlyFromNewTip(t *testing.T) {
	w := newPathWorld(t)
	c1 := makeCommit(w.treeID, map[string]string{"README.md": "r", "secret/gone.txt": "gone"})
	c2 := makeCommit(w.treeID, map[string]string{"README.md": "r2"}, c1)
	w.push("alice", "", c1, c2)
	resp := w.pull("bob")
	if len(resp.GetStubbedPaths()) != 0 {
		t.Fatalf("stubbed_paths = %v, want none (the path is not in the tip)", resp.GetStubbedPaths())
	}
	if hexes(resp.GetObjectsToFetch())[blobHex("gone")] {
		t.Fatal("blob of a restricted path from history was offered")
	}

	// Shallow pull: only the tip snapshot, still stubbed.
	c3 := makeCommit(w.treeID, map[string]string{"README.md": "r3", "secret/new.txt": "n"}, c2)
	w.push("alice", c2.id, c3)
	shallow, err := w.svc.Pull(w.ctx("bob"), &syncpb.PullRequest{TreeId: &syncpb.TreeId{Value: w.treeID}, Depth: proto.Uint32(1)})
	if err != nil {
		t.Fatal(err)
	}
	if len(shallow.GetSnapshots()) != 1 || !slices.Equal(shallow.GetStubbedPaths(), []string{"secret/new.txt"}) {
		t.Fatalf("shallow pull = %d snapshots, stubbed %v", len(shallow.GetSnapshots()), shallow.GetStubbedPaths())
	}
}

func TestPushPathDenied(t *testing.T) {
	w := newPathWorld(t)
	c1 := makeCommit(w.treeID, map[string]string{"README.md": "r", "infra/main.tf": "tf"})
	if resp := w.push("alice", "", c1); !resp.GetAccepted() {
		t.Fatalf("alice push rejected: %v", resp.GetRejectionReason())
	}

	// bob changes infra/main.tf: denied by policy, the tip does not move.
	bad := makeCommit(w.treeID, map[string]string{"README.md": "r", "infra/main.tf": "tf2"}, c1)
	resp := w.push("bob", c1.id, bad)
	if resp.GetAccepted() {
		t.Fatal("push changing a write-denied path was accepted")
	}
	ad := resp.GetRejectionReason().GetAccessDenied()
	if want := "writing infra/main.tf is denied by policy freeze-infra"; ad == nil || ad.GetReason() != want {
		t.Fatalf("rejection = %v, want access_denied %q", resp.GetRejectionReason(), want)
	}
	if b, _ := w.store.GetBranch(context.Background(), w.treeID, "main"); b.Tip != c1.id {
		t.Fatalf("tip moved to %s", b.Tip)
	}
	if len(w.audit.actions("branch.push_rejected")) == 0 {
		t.Fatal("rejected push not audited")
	}

	// Deleting a denied path is writing it too.
	del := makeCommit(w.treeID, map[string]string{"README.md": "r"}, c1)
	if resp := w.push("bob", c1.id, del); resp.GetAccepted() {
		t.Fatal("push deleting a write-denied path was accepted")
	}

	// Other paths are fine for bob; alice is not covered by the policy.
	ok := makeCommit(w.treeID, map[string]string{"README.md": "r2", "infra/main.tf": "tf"}, c1)
	if resp := w.push("bob", c1.id, ok); !resp.GetAccepted() {
		t.Fatalf("bob push of README rejected: %v", resp.GetRejectionReason())
	}
	ok2 := makeCommit(w.treeID, map[string]string{"README.md": "r2", "infra/main.tf": "tf3"}, ok)
	if resp := w.push("alice", ok.id, ok2); !resp.GetAccepted() {
		t.Fatalf("alice push of infra rejected: %v", resp.GetRejectionReason())
	}
}
