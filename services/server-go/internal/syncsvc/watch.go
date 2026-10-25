package syncsvc

// v0.5 (docs/ENGINEERING_PLAN.md §13.11, workstream daemon): WatchBranches
// streams branch changes to clients (worktree-bg's sync loop) and Unstage
// removes the caller's staged snapshot.

import (
	"context"
	"encoding/json"
	"errors"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
)

// maxWatchedTrees bounds the tree_ids of one WatchBranches call.
const maxWatchedTrees = 1000

// watchEventTypes are the bus events WatchBranches subscribes to.
var watchEventTypes = []string{
	domain.EventBranchPushed,
	domain.EventBranchCreated,
	domain.EventBranchDeleted,
	domain.EventTreeDeleted,
}

// WatchBranches streams a BranchEvent for every push to, creation of or
// deletion of a branch in the requested trees, until the client cancels.
//
// Every tree must exist and be readable by the caller when the call starts
// (NotFound / PermissionDenied / Unauthenticated otherwise). Access is
// re-checked for each event: if the caller loses read access to a watched
// tree the stream ends with PermissionDenied; a deleted tree is dropped from
// the watch, and the stream ends with NotFound once none remain. Response
// headers are sent as soon as the bus subscription is in place, so a client
// that has received them will not miss later events. Events are delivered
// best effort: a client too slow to drain its stream misses events (the bus
// drops them) and should fall back to periodic fetches.
func (s *Service) WatchBranches(req *syncpb.WatchBranchesRequest, stream grpc.ServerStreamingServer[syncpb.BranchEvent]) error {
	ctx := stream.Context()
	if s.d.Bus == nil {
		return status.Error(codes.Unimplemented, "branch events are not enabled on this server")
	}
	ids := req.GetTreeIds()
	if len(ids) == 0 {
		return invalidf("tree_ids is required")
	}
	if len(ids) > maxWatchedTrees {
		return invalidf("at most %d trees may be watched per call", maxWatchedTrees)
	}
	p := iam.PrincipalFrom(ctx)
	watched := make(map[string]bool, len(ids))
	tenants := make(map[string]bool, 1)
	for _, id := range ids {
		t, err := s.loadTree(ctx, id)
		if err != nil {
			return err
		}
		if err := s.authorize(ctx, p, domain.ActTreeRead, t.TenantID, t); err != nil {
			return err
		}
		watched[t.ID] = true
		tenants[t.TenantID] = true
	}

	filter := domain.EventFilter{Types: watchEventTypes}
	if len(watched) == 1 {
		for id := range watched {
			filter.TreeID = id
		}
	} else if len(tenants) == 1 {
		for id := range tenants {
			filter.TenantID = id
		}
	}
	events, cancel := s.d.Bus.Subscribe(filter)
	defer cancel()
	if err := stream.SendHeader(metadata.MD{}); err != nil {
		return err
	}

	for {
		select {
		case <-ctx.Done():
			if errors.Is(ctx.Err(), context.DeadlineExceeded) {
				return status.Error(codes.DeadlineExceeded, "deadline exceeded")
			}
			return status.Error(codes.Canceled, "watch canceled")
		case <-s.d.Done: // nil channel (no Done configured) never fires
			return status.Error(codes.Unavailable, "server is shutting down")
		case e, ok := <-events:
			if !ok {
				return status.Error(codes.Unavailable, "server is shutting down")
			}
			if !watched[e.TreeID] {
				continue
			}
			if e.Type == domain.EventTreeDeleted {
				delete(watched, e.TreeID)
				if len(watched) == 0 {
					return status.Error(codes.NotFound, "every watched tree was deleted")
				}
				continue
			}
			ev, ok := branchEventFromDomain(e)
			if !ok {
				s.log.DebugContext(ctx, "watch: undecodable branch event", "type", e.Type, "tree", e.TreeID)
				continue
			}
			t, err := s.d.Store.GetTree(ctx, e.TreeID)
			if errors.Is(err, domain.ErrNotFound) {
				delete(watched, e.TreeID)
				if len(watched) == 0 {
					return status.Error(codes.NotFound, "every watched tree was deleted")
				}
				continue
			}
			if err != nil {
				return s.toStatus(ctx, err, "tree")
			}
			if err := s.authorize(ctx, p, domain.ActTreeRead, t.TenantID, t); err != nil {
				return err
			}
			if err := stream.Send(ev); err != nil {
				return err
			}
		}
	}
}

// branchEventPayload is the union of the branch event payload shapes
// published on the bus: map{"branch","old_tip","new_tip"} for pushes and
// domain.Branch for creations and deletions.
type branchEventPayload struct {
	Branch string `json:"branch"`
	Name   string `json:"name"`
	OldTip string `json:"old_tip"`
	NewTip string `json:"new_tip"`
	Tip    string `json:"tip"`
}

// branchEventFromDomain converts a branch.* bus event to the wire form.
func branchEventFromDomain(e domain.Event) (*syncpb.BranchEvent, bool) {
	var pl branchEventPayload
	switch v := e.Payload.(type) {
	case domain.Branch:
		pl = branchEventPayload{Name: v.Name, Tip: v.Tip}
	case *domain.Branch:
		if v == nil {
			return nil, false
		}
		pl = branchEventPayload{Name: v.Name, Tip: v.Tip}
	default:
		raw, err := json.Marshal(v)
		if err != nil || json.Unmarshal(raw, &pl) != nil {
			return nil, false
		}
	}
	name := pl.Branch
	if name == "" {
		name = pl.Name
	}
	if name == "" {
		return nil, false
	}
	oldTip, newTip := pl.OldTip, pl.NewTip
	switch e.Type {
	case domain.EventBranchCreated:
		if newTip == "" {
			newTip = pl.Tip
		}
	case domain.EventBranchDeleted:
		if oldTip == "" {
			oldTip = pl.Tip
		}
		newTip = ""
	}
	return &syncpb.BranchEvent{
		TreeId:     &syncpb.TreeId{Value: e.TreeID},
		BranchName: name,
		OldTip:     snapshotIDToPB(oldTip),
		NewTip:     snapshotIDToPB(newTip),
		Actor:      e.Actor,
		At:         tsToPB(e.At),
	}, true
}

// Unstage removes the caller's staged snapshot for branch_name in a tree
// (every branch when branch_name is empty). removed reports whether an
// entry existed. Requires authentication and read access to the tree.
func (s *Service) Unstage(ctx context.Context, req *syncpb.UnstageRequest) (*syncpb.UnstageResponse, error) {
	u, ok := s.d.Staged.(staged.Unstager)
	if s.d.Staged == nil || !ok {
		return nil, status.Error(codes.Unimplemented, "unstaging is not enabled on this server")
	}
	p, err := requireAuth(ctx)
	if err != nil {
		return nil, err
	}
	t, err := s.loadTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	branch := req.GetBranchName()
	if branch != "" {
		if err := validateRefName("branch", branch); err != nil {
			return nil, err
		}
	}
	removed, err := u.Unstage(ctx, p, t, "", branch)
	if err != nil {
		return nil, s.toStatus(ctx, err, "unstage")
	}
	for _, r := range removed {
		s.audit(ctx, p, domain.AuditEvent{
			TenantID:     t.TenantID,
			Action:       "staged.unstage",
			ResourceType: "staged",
			ResourceID:   t.ID + "/" + r.BranchName,
			Details: map[string]string{
				"tree":     t.Name,
				"branch":   r.BranchName,
				"snapshot": r.SnapshotID,
			},
		})
	}
	return &syncpb.UnstageResponse{Removed: len(removed) > 0}, nil
}
