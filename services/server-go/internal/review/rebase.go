package review

// v0.8 rebase merges and source-branch deletion (docs/ENGINEERING_PLAN.md
// §16.9, specs/server/Merging.md §1): merge method "rebase" replays the
// source snapshots the target lacks, oldest first, onto the target tip as
// new snapshots; MergeOptions.DeleteSourceBranch deletes the source branch
// after a successful merge.

import (
	"context"
	"errors"
	"fmt"
	"sync"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// MergeRebase replays the source snapshots onto the target tip (linear
// history; a fast-forward when the target is an ancestor of the source).
const MergeRebase = "rebase"

// MaxRebaseSnapshots bounds the snapshots one rebase merge replays.
const MaxRebaseSnapshots = 1000

// rebaseInfo is the (cached) outcome of replaying a source onto a target.
type rebaseInfo struct {
	incoming  int        // snapshots to replay
	merge     string     // a merge snapshot among them ("" = none)
	conflicts []Conflict // conflicts of the first snapshot that does not apply
	stoppedAt string     // that snapshot
}

// rebaseCache is a small FIFO cache of rebase outcomes keyed by (tree,
// source tip, target tip).
type rebaseCache struct {
	mu    sync.Mutex
	m     map[string]*rebaseInfo
	order []string
}

func (c *rebaseCache) get(key string) (*rebaseInfo, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	o, ok := c.m[key]
	return o, ok
}

func (c *rebaseCache) put(key string, o *rebaseInfo) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.m == nil {
		c.m = map[string]*rebaseInfo{}
	}
	if _, ok := c.m[key]; !ok {
		if len(c.order) >= mergeCacheSize {
			delete(c.m, c.order[0])
			c.order = c.order[1:]
		}
		c.order = append(c.order, key)
	}
	c.m[key] = o
}

// analyzeRebase fills the method state of MergeRebase for ev (called by
// analyzeMethods once protection allows the method and the target is not up
// to date).
func (s *Service) analyzeRebase(ctx context.Context, tree *domain.Tree, ev *evaluation, st *methodState) error {
	info, err := s.rebaseOutcome(ctx, tree, ev)
	if err != nil {
		return err
	}
	switch {
	case info.merge != "":
		st.kind, st.err = BlockerMergeMethod, fmt.Errorf(
			"merge request !%d: rebase replays linear histories only, but snapshot %s of '%s' is a merge snapshot (use squash or merge): %w",
			ev.mr.Number, shortID(info.merge), ev.mr.SourceBranch, domain.ErrConflict)
	case info.incoming > MaxRebaseSnapshots:
		st.kind, st.err = BlockerMergeMethod, fmt.Errorf(
			"merge request !%d: a rebase replays at most %d snapshots (this one has %d; use squash or merge): %w",
			ev.mr.Number, MaxRebaseSnapshots, info.incoming, domain.ErrConflict)
	case !ev.diverged:
		st.possible = true // the target is an ancestor: a fast-forward
	case s.objects == nil:
		st.kind, st.err = BlockerMergeMethod, fmt.Errorf(
			"merge request !%d: server-side rebase merges are not available on this server: %w", ev.mr.Number, domain.ErrConflict)
	case len(info.conflicts) > 0:
		st.kind, st.err = BlockerConflicts, s.rebaseConflictError(ev.mr, info)
	default:
		st.possible = true
	}
	return nil
}

func (s *Service) rebaseConflictError(mr *domain.MergeRequest, info *rebaseInfo) *ConflictError {
	return &ConflictError{
		Message: fmt.Sprintf("merge request !%d cannot be rebased: snapshot %s does not apply on '%s': %s; merge instead, or rebase '%s' locally and push",
			mr.Number, shortID(info.stoppedAt), mr.TargetBranch, conflictSummary(info.conflicts), mr.SourceBranch),
		Conflicts: append([]Conflict(nil), info.conflicts...),
	}
}

// rebaseOutcome returns the cached outcome of rebasing ev's source onto its
// target.
func (s *Service) rebaseOutcome(ctx context.Context, tree *domain.Tree, ev *evaluation) (*rebaseInfo, error) {
	key := tree.ID + "\x00" + ev.sourceTip + "\x00" + ev.targetTip
	if info, ok := s.rebases.get(key); ok {
		return info, nil
	}
	incoming, err := s.incoming(ctx, ev.sourceTip, ev.targetTip)
	if err != nil {
		return nil, err
	}
	info := &rebaseInfo{incoming: len(incoming)}
	for _, sn := range incoming {
		if len(sn.Parents) > 1 {
			info.merge = sn.ID
			break
		}
	}
	if info.merge == "" && info.incoming <= MaxRebaseSnapshots && ev.diverged && s.objects != nil {
		rr, err := s.replay(ctx, incoming, ev.targetTip)
		if err != nil {
			return nil, err
		}
		info.conflicts, info.stoppedAt = rr.conflicts, rr.stoppedAt
	}
	s.rebases.put(key, info)
	return info, nil
}

// incoming lists the snapshots of sourceTip that targetTip lacks, oldest
// first.
func (s *Service) incoming(ctx context.Context, sourceTip, targetTip string) ([]domain.Snapshot, error) {
	var haves []string
	if targetTip != "" {
		haves = []string{targetTip}
	}
	out, err := s.hist.Missing(ctx, sourceTip, haves)
	if err != nil {
		return nil, fmt.Errorf("review: list incoming snapshots: %w", err)
	}
	return out, nil
}

// replayStep is one replayed snapshot.
type replayStep struct {
	src      *domain.Snapshot
	manifest *syncpb.ManifestRecord
}

// replayResult is the outcome of replay.
type replayResult struct {
	steps     []replayStep // snapshots that change the tree (empty ones are dropped)
	blobs     map[hash.Hash][]byte
	conflicts []Conflict
	stoppedAt string
}

// replay applies each incoming snapshot's changes (against its first
// parent) on top of targetTip with three-way merges. It stops at the first
// snapshot that conflicts. Nothing is written.
func (s *Service) replay(ctx context.Context, incoming []domain.Snapshot, targetTip string) (*replayResult, error) {
	cur, err := s.manifestOf(ctx, targetTip)
	if err != nil {
		return nil, err
	}
	rr := &replayResult{blobs: map[hash.Hash][]byte{}}
	for i := range incoming {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		sn, err := s.store.GetSnapshot(ctx, incoming[i].ID)
		if err != nil {
			return nil, fmt.Errorf("review: snapshot %s: %w", shortID(incoming[i].ID), err)
		}
		parent := ""
		if len(sn.Parents) > 0 {
			parent = sn.Parents[0]
		}
		base, err := s.manifestOf(ctx, parent)
		if err != nil {
			return nil, err
		}
		theirs, err := s.loadManifest(ctx, sn.ManifestHash)
		if err != nil {
			return nil, err
		}
		res, err := merge3.Merge(ctx, s.objects, base, cur, theirs)
		if err != nil {
			return nil, fmt.Errorf("review: replay %s: %w", shortID(sn.ID), err)
		}
		if !res.Clean() {
			rr.conflicts, rr.stoppedAt = toConflicts(res.Conflicts), sn.ID
			return rr, nil
		}
		if proto.Equal(res.Manifest, cur) {
			continue // already on the target
		}
		for h, data := range res.Blobs {
			rr.blobs[h] = data
		}
		rr.steps = append(rr.steps, replayStep{src: sn, manifest: res.Manifest})
		cur = res.Manifest
	}
	return rr, nil
}

// prepareRebase writes the snapshots of a rebase merge and returns its plan
// (the target is not moved). A target that is an ancestor of the source is
// fast-forwarded. Replayed snapshots keep the author, message and creation
// time of the originals, are pushed by the merger and carry no signature;
// snapshots whose changes the target already has are dropped.
func (s *Service) prepareRebase(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, ev *evaluation) (*mergePlan, error) {
	if !ev.diverged {
		return &mergePlan{method: MergeRebase, newTip: ev.sourceTip, merged: ev.sourceTip}, nil
	}
	incoming, err := s.incoming(ctx, ev.sourceTip, ev.targetTip)
	if err != nil {
		return nil, err
	}
	rr, err := s.replay(ctx, incoming, ev.targetTip)
	if err != nil {
		return nil, err
	}
	if len(rr.conflicts) > 0 {
		return nil, s.rebaseConflictError(mr, &rebaseInfo{conflicts: rr.conflicts, stoppedAt: rr.stoppedAt})
	}
	for h, data := range rr.blobs {
		if err := s.objects.Put(ctx, h, data); err != nil {
			return nil, fmt.Errorf("review: store rebased blob: %w", err)
		}
	}
	now := s.now()
	prev := ev.targetTip
	snaps := make([]*domain.Snapshot, 0, len(rr.steps))
	for _, step := range rr.steps {
		raw, err := proto.MarshalOptions{Deterministic: true}.Marshal(step.manifest)
		if err != nil {
			return nil, fmt.Errorf("review: encode rebased manifest: %w", err)
		}
		mh := hash.Sum(raw)
		if err := s.objects.Put(ctx, mh, raw); err != nil {
			return nil, fmt.Errorf("review: store rebased manifest: %w", err)
		}
		snap := &domain.Snapshot{
			ID:           s.newID(),
			TreeID:       tree.ID,
			BranchName:   mr.TargetBranch,
			Parents:      []string{prev},
			ManifestHash: mh,
			Author:       step.src.Author,
			Message:      step.src.Message,
			CreatedAt:    step.src.CreatedAt,
			PushedBy:     p.AccountID,
			PushedAt:     now,
		}
		snaps = append(snaps, snap)
		prev = snap.ID
	}
	if len(snaps) == 0 {
		// Every change is already on the target.
		return &mergePlan{method: MergeRebase, newTip: ev.targetTip, merged: ev.targetTip}, nil
	}
	if err := s.checkRebaseQuota(ctx, p, tree, mr.TargetBranch, ev, snaps); err != nil {
		return nil, err
	}
	for _, snap := range snaps {
		if err := s.store.PutSnapshot(ctx, snap); err != nil {
			return nil, fmt.Errorf("review: store rebased snapshot: %w", err)
		}
	}
	last := snaps[len(snaps)-1].ID
	return &mergePlan{method: MergeRebase, newTip: last, merged: last}, nil
}

// checkRebaseQuota sizes the objects the replayed snapshots add (on top of
// what the target and source tips reference) against the tenant quota.
func (s *Service) checkRebaseQuota(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string, ev *evaluation, snaps []*domain.Snapshot) error {
	if s.quota == nil {
		return nil
	}
	sizing := make([]*domain.Snapshot, len(snaps))
	for i, sn := range snaps {
		c := *sn
		sizing[i] = &c
	}
	sizing[0].Parents = []string{ev.targetTip, ev.sourceTip}
	plan := &domain.PushPlan{Branch: branch, OldTip: ev.targetTip, NewTip: snaps[len(snaps)-1].ID, NewSnapshots: sizing}
	if err := s.quota.CheckPushPlan(ctx, p, tree, plan); err != nil {
		return quotaError(err)
	}
	return nil
}

// checkDeleteSource validates MergeOptions.DeleteSourceBranch before a
// merge: the caller may delete the source branch (domain.ActBranchDelete,
// not the default branch, no NoDelete protection) and no other open merge
// request uses it.
func (s *Service) checkDeleteSource(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest) error {
	if mr.SourceTreeID != "" && mr.SourceTreeID != tree.ID {
		return fmt.Errorf("merge request !%d: the source branch lives in a fork and cannot be deleted by this merge: %w", mr.Number, domain.ErrInvalid)
	}
	if err := s.CheckDeleteBranch(ctx, p, tree, mr.SourceBranch); err != nil {
		return err
	}
	open, err := s.store.ListMergeRequests(ctx, tree.ID, domain.MRStateOpen)
	if err != nil {
		return fmt.Errorf("review: list merge requests: %w", err)
	}
	for i := range open {
		o := &open[i]
		if o.Number != mr.Number && (o.SourceBranch == mr.SourceBranch || o.TargetBranch == mr.SourceBranch) {
			return fmt.Errorf("source branch '%s' is also used by open merge request !%d; merge without deleting it: %w",
				mr.SourceBranch, o.Number, domain.ErrConflict)
		}
	}
	return nil
}

// deleteSource deletes the source branch of a merged merge request and
// publishes branch.deleted. Failures are logged: the merge itself is done.
func (s *Service) deleteSource(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest) {
	b, err := s.store.GetBranch(ctx, tree.ID, mr.SourceBranch)
	if err == nil {
		err = s.store.DeleteBranch(ctx, tree.ID, mr.SourceBranch)
	}
	if err != nil {
		if !errors.Is(err, domain.ErrNotFound) {
			s.logger().Error("review: delete source branch after merge", "tree", tree.ID, "branch", mr.SourceBranch, "error", err)
		}
		return
	}
	s.publish(domain.EventBranchDeleted, p, tree, b)
	if s.audit != nil {
		ev := domain.AuditEvent{
			TenantID: tree.TenantID, ActorID: p.AccountID, Actor: p.Username,
			Action: "branch.delete", ResourceType: "branch", ResourceID: tree.ID + "/" + mr.SourceBranch,
			Details: map[string]string{"branch": mr.SourceBranch, "tip": b.Tip, "merge_request": fmt.Sprint(mr.Number)},
			At:      s.now(),
		}
		s.audit.Record(ctx, ev)
	}
}
