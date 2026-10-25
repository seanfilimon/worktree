package syncsvc

import (
	"context"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// watchHarness is a harness whose service uses a real event bus and a real
// staged-snapshot service (the shared fakes cannot deliver events).
func watchHarness(t *testing.T) (*harness, *events.Bus) {
	t.Helper()
	h := newHarness(t)
	bus := events.NewBus()
	// Set before the first RPC; the gRPC transport orders it before reads.
	h.svc.d.Bus = bus
	h.svc.d.Staged = staged.New(meta.NewMemory(), h.authz, bus)
	return h, bus
}

// watch opens a WatchBranches stream and waits for its headers, which the
// server sends once the bus subscription exists.
func watch(t *testing.T, ctx context.Context, h *harness, treeIDs ...string) grpc.ServerStreamingClient[syncpb.BranchEvent] {
	t.Helper()
	req := &syncpb.WatchBranchesRequest{}
	for _, id := range treeIDs {
		req.TreeIds = append(req.TreeIds, &syncpb.TreeId{Value: id})
	}
	stream, err := h.client.WatchBranches(ctx, req)
	if err != nil {
		t.Fatalf("WatchBranches: %v", err)
	}
	if _, err := stream.Header(); err != nil {
		t.Fatalf("WatchBranches headers: %v", err)
	}
	return stream
}

// recv receives one event, failing the test on error.
func recv(t *testing.T, stream grpc.ServerStreamingClient[syncpb.BranchEvent]) *syncpb.BranchEvent {
	t.Helper()
	ev, err := stream.Recv()
	if err != nil {
		t.Fatalf("Recv: %v", err)
	}
	return ev
}

func TestWatchBranchesStreamsPushes(t *testing.T) {
	h, _ := watchHarness(t)
	treeID := h.newTree("web")
	stream := watch(t, h.as("bob"), h, treeID)

	c1 := makeCommit(treeID, map[string]string{"a.txt": "1"})
	h.upload(h.as("alice"), c1)
	h.mustPush(h.as("alice"), pushReq(treeID, "main", "", c1))
	ev := recv(t, stream)
	if ev.GetTreeId().GetValue() != treeID || ev.GetBranchName() != "main" {
		t.Fatalf("event = %v", ev)
	}
	if ev.OldTip != nil || ev.GetNewTip().GetValue() != c1.id || ev.GetActor() != "alice" || ev.GetAt() == nil {
		t.Fatalf("event = %v", ev)
	}

	c2 := makeCommit(treeID, map[string]string{"a.txt": "2"}, c1)
	h.upload(h.as("alice"), c2)
	h.mustPush(h.as("alice"), pushReq(treeID, "main", c1.id, c2))
	ev = recv(t, stream)
	if ev.GetOldTip().GetValue() != c1.id || ev.GetNewTip().GetValue() != c2.id {
		t.Fatalf("event = %v", ev)
	}
}

func TestWatchBranchesPayloadShapesAndFiltering(t *testing.T) {
	h, bus := watchHarness(t)
	web := h.newTree("web")
	docs := h.newTree("docs")
	other := h.newTree("other")
	stream := watch(t, h.as("alice"), h, web, docs)

	// Not watched, not a branch event, undecodable: all skipped.
	bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: tenantAcme, TreeID: other, Payload: map[string]any{"branch": "main", "new_tip": "x"}})
	bus.Publish(domain.Event{Type: domain.EventTagPushed, TenantID: tenantAcme, TreeID: web, Payload: map[string]any{"name": "v1"}})
	bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: tenantAcme, TreeID: web, Payload: 42})

	bus.Publish(domain.Event{Type: domain.EventBranchCreated, TenantID: tenantAcme, TreeID: web, Actor: "alice",
		Payload: &domain.Branch{TreeID: web, Name: "feature", Tip: "tip-1"}})
	bus.Publish(domain.Event{Type: domain.EventBranchDeleted, TenantID: tenantAcme, TreeID: docs,
		Payload: domain.Branch{TreeID: docs, Name: "old", Tip: "tip-2"}})
	bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: tenantAcme, TreeID: docs,
		Payload: map[string]string{"branch": "main", "old_tip": "a", "new_tip": "b"}})

	created := recv(t, stream)
	if created.GetTreeId().GetValue() != web || created.GetBranchName() != "feature" || created.OldTip != nil || created.GetNewTip().GetValue() != "tip-1" {
		t.Fatalf("created = %v", created)
	}
	deleted := recv(t, stream)
	if deleted.GetTreeId().GetValue() != docs || deleted.GetBranchName() != "old" || deleted.GetOldTip().GetValue() != "tip-2" || deleted.NewTip != nil {
		t.Fatalf("deleted = %v", deleted)
	}
	pushed := recv(t, stream)
	if pushed.GetBranchName() != "main" || pushed.GetOldTip().GetValue() != "a" || pushed.GetNewTip().GetValue() != "b" {
		t.Fatalf("pushed = %v", pushed)
	}
}

func TestWatchBranchesRequiresReadAccess(t *testing.T) {
	h, _ := watchHarness(t)
	treeID := h.newTree("web")
	tests := []struct {
		name string
		user string
		ids  []string
		want codes.Code
	}{
		{"no trees", "alice", nil, codes.InvalidArgument},
		{"malformed id", "alice", []string{"not a valid id!"}, codes.InvalidArgument},
		{"unknown tree", "alice", []string{newID()}, codes.NotFound},
		{"non-member", "carol", []string{treeID}, codes.PermissionDenied},
		{"anonymous on private tree", "", []string{treeID}, codes.Unauthenticated},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			req := &syncpb.WatchBranchesRequest{}
			for _, id := range tc.ids {
				req.TreeIds = append(req.TreeIds, &syncpb.TreeId{Value: id})
			}
			stream, err := h.client.WatchBranches(h.as(tc.user), req)
			if err == nil {
				_, err = stream.Recv()
			}
			wantCode(t, err, tc.want)
		})
	}
}

func TestWatchBranchesEndsWhenAccessIsRevoked(t *testing.T) {
	h, bus := watchHarness(t)
	treeID := h.newTree("web")
	stream := watch(t, h.as("bob"), h, treeID)
	h.authz.setRole(tenantAcme, bob.AccountID, "")
	bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: tenantAcme, TreeID: treeID, Payload: map[string]any{"branch": "main", "new_tip": "t"}})
	_, err := stream.Recv()
	wantCode(t, err, codes.PermissionDenied)
}

func TestWatchBranchesEndsWhenTreesAreDeleted(t *testing.T) {
	h, bus := watchHarness(t)
	web := h.newTree("web")
	docs := h.newTree("docs")
	stream := watch(t, h.as("alice"), h, web, docs)
	bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: tenantAcme, TreeID: web})
	bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: tenantAcme, TreeID: docs, Payload: map[string]any{"branch": "main", "new_tip": "t"}})
	if ev := recv(t, stream); ev.GetTreeId().GetValue() != docs {
		t.Fatalf("event = %v", ev)
	}
	bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: tenantAcme, TreeID: docs})
	_, err := stream.Recv()
	wantCode(t, err, codes.NotFound)
}

func TestWatchBranchesClientCancellationUnsubscribes(t *testing.T) {
	h, bus := watchHarness(t)
	treeID := h.newTree("web")
	ctx, cancel := context.WithCancel(h.as("alice"))
	stream := watch(t, ctx, h, treeID)
	if n := bus.Stats().Subscribers; n != 1 {
		t.Fatalf("%d subscribers while watching, want 1", n)
	}
	cancel()
	_, err := stream.Recv()
	wantCode(t, err, codes.Canceled)
	deadline := time.Now().Add(10 * time.Second)
	for bus.Stats().Subscribers != 0 {
		if time.Now().After(deadline) {
			t.Fatalf("server did not unsubscribe after client cancellation")
		}
		time.Sleep(10 * time.Millisecond)
	}
}

func TestWatchBranchesEndsOnShutdown(t *testing.T) {
	h, _ := watchHarness(t)
	done := make(chan struct{})
	h.svc.d.Done = done
	treeID := h.newTree("web")
	stream := watch(t, h.as("alice"), h, treeID)
	close(done)
	_, err := stream.Recv()
	wantCode(t, err, codes.Unavailable)
}

func TestWatchBranchesWithoutBus(t *testing.T) {
	h := newHarness(t)
	h.svc.d.Bus = nil
	treeID := h.newTree("web")
	stream, err := h.client.WatchBranches(h.as("alice"), &syncpb.WatchBranchesRequest{TreeIds: []*syncpb.TreeId{{Value: treeID}}})
	if err == nil {
		_, err = stream.Recv()
	}
	wantCode(t, err, codes.Unimplemented)
}

func TestUnstage(t *testing.T) {
	h, bus := watchHarness(t)
	cleared, cancel := bus.Subscribe(domain.EventFilter{Types: []string{domain.EventStagedCleared}})
	defer cancel()
	treeID := h.newTree("web")
	stage := func(branch string) {
		t.Helper()
		resp, err := h.client.Stage(h.as("alice"), &syncpb.StageUploadRequest{
			TreeId: &syncpb.TreeId{Value: treeID}, BranchName: branch, SnapshotId: &syncpb.SnapshotId{Value: newID()},
		})
		if err != nil || !resp.GetAccepted() {
			t.Fatalf("stage: %v %v", resp, err)
		}
	}
	unstage := func(user, branch string) (*syncpb.UnstageResponse, error) {
		return h.client.Unstage(h.as(user), &syncpb.UnstageRequest{TreeId: &syncpb.TreeId{Value: treeID}, BranchName: branch})
	}
	stage("main")
	stage("feature")

	resp, err := unstage("alice", "main")
	if err != nil || !resp.GetRemoved() {
		t.Fatalf("unstage main: %v %v", resp, err)
	}
	select {
	case e := <-cleared:
		c, ok := e.Payload.(staged.Cleared)
		if !ok || c.Reason != staged.ReasonUnstaged || c.BranchName != "main" || c.AccountID != alice.AccountID {
			t.Fatalf("staged.cleared = %+v", e)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("no staged.cleared event")
	}
	if got := len(h.audit.actions("staged.unstage")); got != 1 {
		t.Fatalf("%d audit records, want 1", got)
	}

	// Already gone; bob has nothing staged.
	if resp, err := unstage("alice", "main"); err != nil || resp.GetRemoved() {
		t.Fatalf("second unstage: %v %v", resp, err)
	}
	if resp, err := unstage("bob", ""); err != nil || resp.GetRemoved() {
		t.Fatalf("bob unstage: %v %v", resp, err)
	}
	// Empty branch = every branch of the caller.
	if resp, err := unstage("alice", ""); err != nil || !resp.GetRemoved() {
		t.Fatalf("unstage all: %v %v", resp, err)
	}
	list, err := h.client.ListStaged(h.as("alice"), &syncpb.ListStagedRequest{TreeId: &syncpb.TreeId{Value: treeID}})
	if err != nil || len(list.GetStaged()) != 0 {
		t.Fatalf("list after unstage: %v %v", list, err)
	}

	_, err = unstage("", "main")
	wantCode(t, err, codes.Unauthenticated)
	_, err = unstage("carol", "main")
	wantCode(t, err, codes.PermissionDenied)
	_, err = unstage("alice", "bad branch")
	wantCode(t, err, codes.InvalidArgument)
	_, err = h.client.Unstage(h.as("alice"), &syncpb.UnstageRequest{TreeId: &syncpb.TreeId{Value: newID()}})
	wantCode(t, err, codes.NotFound)
}

func TestUnstageUnsupported(t *testing.T) {
	h := newHarness(t) // fakeStaged does not implement staged.Unstager
	treeID := h.newTree("web")
	_, err := h.client.Unstage(h.as("alice"), &syncpb.UnstageRequest{TreeId: &syncpb.TreeId{Value: treeID}})
	if status.Code(err) != codes.Unimplemented {
		t.Fatalf("err = %v", err)
	}
}
