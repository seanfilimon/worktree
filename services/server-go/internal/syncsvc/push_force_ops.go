package syncsvc

// v0.8 history-v2 (docs/ENGINEERING_PLAN.md §16.10): push idempotency
// (operation_id), the server reflog of branch moves and their retention.
//
// Doc collections:
//
//   - push_ops — DocKey(treeID, operationID) → pushOp: the first response
//     to a push with that operation id, for 24 h (PushOpTTL). A retried
//     push by the same account returns it without doing anything.
//   - branch_reflog — DocKey(treeID, branch, %020d unix-nano, random) →
//     ReflogEntry: every accepted push that moved a branch (force pushes
//     and protection bypasses flagged), kept 90 days (ReflogTTL). Served by
//     GET {tree}/reflog.
//
// Both are deleted with their tree (tree.deleted) by Start.

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"sort"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Doc collections of this file.
const (
	PushOpsCollection = "push_ops"
	ReflogCollection  = "branch_reflog"
)

// Retention.
const (
	PushOpTTL = 24 * time.Hour
	ReflogTTL = 90 * 24 * time.Hour
	// maxOperationID bounds PushRequest.operation_id.
	maxOperationID = 128
	// pruneInterval is how often Start prunes expired documents.
	pruneInterval = time.Hour
)

// pushOp is a stored push result (push_ops).
type pushOp struct {
	TreeID      string    `json:"tree_id"`
	OperationID string    `json:"operation_id"`
	AccountID   string    `json:"account_id"`
	Branch      string    `json:"branch"`
	Response    []byte    `json:"response"` // marshaled PushResponse
	At          time.Time `json:"at"`
}

// ReflogEntry is one branch move in the server reflog (branch_reflog).
type ReflogEntry struct {
	TreeID string `json:"tree_id"`
	Branch string `json:"branch"`
	// OldTip is the tip before the push ("" = the branch was created).
	OldTip string `json:"old_tip"`
	NewTip string `json:"new_tip"`
	// Kind is "push" or "force_push".
	Kind string `json:"kind"`
	// Forced is set for force pushes (the old tip is no longer an ancestor).
	Forced bool `json:"forced"`
	// Bypass is set when branch protection was bypassed; Bypassed lists
	// the refusals that were lifted.
	Bypass   bool      `json:"bypass"`
	Bypassed []string  `json:"bypassed"`
	Actor    string    `json:"actor"`
	ActorID  string    `json:"actor_id"`
	Count    int       `json:"count"`
	At       time.Time `json:"at"`
}

// Push implements the Push RPC: pushOnce, made idempotent by operation_id
// (a retried push of the same account with the same id within PushOpTTL
// returns the first response).
func (s *Service) Push(ctx context.Context, req *syncpb.PushRequest) (*syncpb.PushResponse, error) {
	opID := req.GetOperationId()
	if opID == "" {
		return s.pushOnce(ctx, req)
	}
	if err := validOperationID(opID); err != nil {
		return nil, err
	}
	p, err := requireAuth(ctx)
	if err != nil {
		return nil, err
	}
	treeID := req.GetTreeId().GetValue()
	if treeID == "" {
		return s.pushOnce(ctx, req) // reports the missing tree id
	}
	if resp := s.replayPush(ctx, p, treeID, opID); resp != nil {
		return resp, nil
	}
	resp, err := s.pushOnce(ctx, req)
	if err == nil && resp != nil {
		s.storePushOp(ctx, p, treeID, opID, req.GetBranchName(), resp)
	}
	return resp, err
}

func validOperationID(id string) error {
	if len(id) > maxOperationID {
		return invalidf("operation_id must be at most %d bytes", maxOperationID)
	}
	for _, r := range id {
		ok := r == '-' || r == '_' || r == '.' || r == ':' ||
			(r >= '0' && r <= '9') || (r >= 'a' && r <= 'z') || (r >= 'A' && r <= 'Z')
		if !ok {
			return invalidf("operation_id may only contain letters, digits and - _ . :")
		}
	}
	return nil
}

// replayPush returns the stored response of a previous push with the same
// operation id by the same account, nil when there is none (or it expired).
func (s *Service) replayPush(ctx context.Context, p *domain.Principal, treeID, opID string) *syncpb.PushResponse {
	op, err := meta.GetDocT[pushOp](ctx, s.d.Store, PushOpsCollection, domain.DocKey(treeID, opID))
	if err != nil {
		if !errors.Is(err, domain.ErrNotFound) {
			s.log.WarnContext(ctx, "push: reading push operation failed", "tree", treeID, "err", err)
		}
		return nil
	}
	if op.AccountID != p.AccountID || s.now().Sub(op.At) > PushOpTTL {
		return nil
	}
	resp := &syncpb.PushResponse{}
	if err := proto.Unmarshal(op.Response, resp); err != nil {
		return nil
	}
	return resp
}

// storePushOp remembers resp for retries (best effort).
func (s *Service) storePushOp(ctx context.Context, p *domain.Principal, treeID, opID, branch string, resp *syncpb.PushResponse) {
	data, err := proto.Marshal(resp)
	if err != nil {
		return
	}
	op := pushOp{TreeID: treeID, OperationID: opID, AccountID: p.AccountID, Branch: branch, Response: data, At: s.now()}
	if err := s.d.Store.PutDoc(ctx, PushOpsCollection, domain.DocKey(treeID, opID), &op); err != nil {
		s.log.WarnContext(ctx, "push: storing push operation failed", "tree", treeID, "err", err)
	}
}

// recordReflog appends e to the server reflog (best effort: a failure is
// logged and never fails the push, which already moved the branch).
func (s *Service) recordReflog(ctx context.Context, p *domain.Principal, t *domain.Tree, e ReflogEntry) {
	e.TreeID = t.ID
	e.At = s.now()
	e.Kind = "push"
	if e.Forced {
		e.Kind = "force_push"
	}
	if e.Bypassed == nil {
		e.Bypassed = []string{}
	}
	if p != nil {
		e.Actor, e.ActorID = p.Username, p.AccountID
	}
	var nonce [4]byte
	_, _ = rand.Read(nonce[:])
	key := domain.DocKey(t.ID, e.Branch, fmt.Sprintf("%020d", e.At.UnixNano()), hex.EncodeToString(nonce[:]))
	if err := s.d.Store.PutDoc(ctx, ReflogCollection, key, &e); err != nil {
		s.log.WarnContext(ctx, "push: recording the reflog failed", "tree", t.ID, "branch", e.Branch, "err", err)
	}
}

// ListReflog returns the server reflog of a tree, newest first: every
// branch when branch is "", at most limit entries (limit <= 0 = all).
// Callers authorize tree:read.
func ListReflog(ctx context.Context, store domain.DocStore, treeID, branch string, limit int) ([]ReflogEntry, error) {
	prefix := meta.DocPrefix(treeID)
	if branch != "" {
		prefix = meta.DocPrefix(treeID, branch)
	}
	out, err := meta.ListDocsT[ReflogEntry](ctx, store, ReflogCollection, prefix)
	if err != nil {
		return nil, err
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].At.After(out[j].At) })
	if limit > 0 && len(out) > limit {
		out = out[:limit]
	}
	for i := range out {
		if out[i].Bypassed == nil {
			out[i].Bypassed = []string{}
		}
	}
	return out, nil
}

// Start deletes the push_ops and branch_reflog documents of deleted trees
// and prunes expired documents hourly, until ctx ends.
func (s *Service) Start(ctx context.Context) {
	var events <-chan domain.Event
	cancel := func() {}
	if s.d.Bus != nil {
		events, cancel = s.d.Bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeDeleted}})
	}
	go func() {
		defer cancel()
		tick := time.NewTicker(pruneInterval)
		defer tick.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-events:
				if !ok {
					events = nil
					continue
				}
				s.deleteTreeDocs(context.WithoutCancel(ctx), e.TreeID)
			case <-tick.C:
				s.pruneExpired(context.WithoutCancel(ctx))
			}
		}
	}()
}

// deleteTreeDocs removes the push_ops and branch_reflog documents of a tree.
func (s *Service) deleteTreeDocs(ctx context.Context, treeID string) {
	if treeID == "" {
		return
	}
	for _, coll := range []string{PushOpsCollection, ReflogCollection} {
		if _, err := s.d.Store.DeleteDocs(ctx, coll, meta.DocPrefix(treeID)); err != nil {
			s.log.WarnContext(ctx, "syncsvc: delete documents of deleted tree", slog.String("collection", coll), slog.String("tree_id", treeID), slog.Any("error", err))
		}
	}
}

// pruneExpired deletes push operations older than PushOpTTL and reflog
// entries older than ReflogTTL.
func (s *Service) pruneExpired(ctx context.Context) {
	now := s.now()
	if ops, err := s.d.Store.ListDocs(ctx, PushOpsCollection, ""); err == nil {
		for _, raw := range ops {
			var op pushOp
			if json.Unmarshal(raw, &op) != nil || now.Sub(op.At) <= PushOpTTL {
				continue
			}
			_ = s.d.Store.DeleteDoc(ctx, PushOpsCollection, domain.DocKey(op.TreeID, op.OperationID))
		}
	}
	cutoff := now.Add(-ReflogTTL)
	if entries, err := s.d.Store.ListDocs(ctx, ReflogCollection, ""); err == nil {
		for _, raw := range entries {
			var e ReflogEntry
			if json.Unmarshal(raw, &e) != nil || !e.At.Before(cutoff) {
				continue
			}
			// The random key suffix is unknown here: delete by prefix.
			_, _ = s.d.Store.DeleteDocs(ctx, ReflogCollection, meta.DocPrefix(e.TreeID, e.Branch, fmt.Sprintf("%020d", e.At.UnixNano())))
		}
	}
}
