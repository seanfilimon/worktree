package todo_test

import (
	"context"
	"io"
	"log/slog"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
	"github.com/seanfilimon/worktree/services/server-go/internal/syncsvc"
	"github.com/seanfilimon/worktree/services/server-go/internal/todo"
)

// TestPushThroughSyncService pushes snapshots with dependencies and a
// "Completes:" line through the real sync service and checks that the
// started todo.Service reacts to the published branch.pushed events.
func TestPushThroughSyncService(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	store := meta.NewMemory()
	objects := objstore.New(t.TempDir())
	hist := meta.NewHistory(store)
	az := authz.New(store, authz.Options{})
	bus := events.NewBus()
	log := slog.New(slog.NewTextHandler(io.Discard, nil))
	al := audit.New(store, log)
	sync := syncsvc.New(syncsvc.Deps{
		Store: store, Objects: objects, History: hist, Authz: az,
		Review:  review.New(store, hist, az, bus, al),
		License: license.New(store), Staged: staged.New(store, az, bus),
		Bus: bus, Audit: al, Version: "test",
	})
	svc := todo.New(todo.Deps{Store: store, Objects: objects, History: hist, Authz: az, Bus: bus, Audit: al, Logger: log})
	svc.Start(ctx)

	acme := &domain.Tenant{ID: "t-acme", Slug: "acme", Kind: domain.TenantOrg}
	if err := store.CreateTenant(ctx, acme); err != nil {
		t.Fatal(err)
	}
	alice := &domain.Account{ID: "a-alice", Username: "alice", TenantID: "t-alice"}
	if err := store.CreateAccount(ctx, alice); err != nil {
		t.Fatal(err)
	}
	if err := store.PutMembership(ctx, &domain.Membership{TenantID: acme.ID, AccountID: alice.ID, Role: domain.RoleOwner}); err != nil {
		t.Fatal(err)
	}
	web := &domain.Tree{ID: "tree-web", TenantID: acme.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	api := &domain.Tree{ID: "tree-api", TenantID: acme.ID, Name: "api", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	for _, tr := range []*domain.Tree{web, api} {
		if err := store.CreateTree(ctx, tr); err != nil {
			t.Fatal(err)
		}
	}
	p := &domain.Principal{AccountID: alice.ID, Username: "alice", TenantID: alice.TenantID}
	pctx := iam.WithPrincipal(ctx, p)

	clock := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	push := func(tree *domain.Tree, id, parent, msg string, deps ...*syncpb.Dependency) {
		t.Helper()
		data := []byte(msg)
		bh := hash.Sum(data)
		if err := objects.Put(ctx, bh, data); err != nil {
			t.Fatal(err)
		}
		m, err := proto.MarshalOptions{Deterministic: true}.Marshal(&syncpb.ManifestRecord{Entries: []*syncpb.ManifestEntry{
			{Path: "f.txt", BlobHash: &syncpb.ContentHash{Value: bh[:]}, Size: uint64(len(data))},
		}})
		if err != nil {
			t.Fatal(err)
		}
		mh := hash.Sum(m)
		if err := objects.Put(ctx, mh, m); err != nil {
			t.Fatal(err)
		}
		clock = clock.Add(time.Minute)
		rec := &syncpb.SnapshotRecord{
			Id: &syncpb.SnapshotId{Value: id}, TreeId: &syncpb.TreeId{Value: tree.ID}, BranchName: "main",
			ManifestHash: &syncpb.ContentHash{Value: mh[:]}, Author: "alice", Message: msg,
			CreatedAt: timestamppb.New(clock), Dependencies: deps,
		}
		req := &syncpb.PushRequest{TreeId: &syncpb.TreeId{Value: tree.ID}, BranchName: "main",
			NewTip: &syncpb.SnapshotId{Value: id}, Snapshots: []*syncpb.SnapshotRecord{rec},
			SnapshotChain: []*syncpb.SnapshotId{{Value: id}}}
		if parent != "" {
			rec.Parents = []*syncpb.SnapshotId{{Value: parent}}
			req.OldTip = &syncpb.SnapshotId{Value: parent}
		}
		resp, err := sync.Push(pctx, req)
		if err != nil || !resp.GetAccepted() {
			t.Fatalf("push %s: %v %v", id, resp, err)
		}
	}
	list := func(tree *domain.Tree) []domain.Todo {
		l, err := svc.List(ctx, tree.ID, todo.Filter{})
		if err != nil {
			t.Fatal(err)
		}
		return l
	}
	wait := func(cond func() bool) {
		t.Helper()
		deadline := time.Now().Add(5 * time.Second)
		for !cond() {
			if time.Now().After(deadline) {
				t.Fatal("timed out")
			}
			time.Sleep(5 * time.Millisecond)
		}
	}

	push(web, "s-web-1", "", "Use v2 users\n\nbody",
		&syncpb.Dependency{Tree: "api", Title: "Add v2 users endpoint", Priority: "high", Blocking: true},
		&syncpb.Dependency{Tree: "api"})
	wait(func() bool { return len(list(api)) == 2 })
	got := list(api)
	if got[0].Title != "Add v2 users endpoint" || got[0].Priority != "high" || !got[0].Blocking ||
		got[0].SourceSnapshotID != "s-web-1" || got[0].CreatedBy != alice.ID || got[0].SourceBranch != "main" {
		t.Fatalf("todo #1 = %+v", got[0])
	}
	if got[1].Title != "Use v2 users" {
		t.Fatalf("todo #2 = %+v", got[1])
	}

	push(api, "s-api-1", "", "Add endpoint\n\nCompletes: #1")
	wait(func() bool { return list(api)[0].Status == domain.TodoDone })
	if l := list(api); l[0].ResolvedBy != "s-api-1" || l[1].Status != domain.TodoOpen {
		t.Fatalf("after completion = %+v", l)
	}
	wait(func() bool {
		evs, _ := al.Query(ctx, domain.AuditFilter{TenantID: acme.ID, Action: "todo.complete"})
		return len(evs) == 1
	})
}
