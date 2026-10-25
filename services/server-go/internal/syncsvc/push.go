package syncsvc

import (
	"context"
	"errors"
	"fmt"
	"path"
	"strings"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// maxPushSnapshots bounds the number of snapshot records in one push.
const maxPushSnapshots = 100_000

// maxMissingReported bounds how many missing objects are named in an
// error message.
const maxMissingReported = 5

// Push advances a branch to new_tip.
//
// Validation pipeline (in order; the first failure wins):
//
//  1. authentication, request shape (branch name, new_tip);
//  2. tree:write on the tree, then — when Authz is also a
//     domain.AccessEvaluator — branch:push on the branch (policies may deny
//     pushes per branch) — refusal → AccessDenied rejection;
//  3. Review.CheckPush (branch protection) — *PushDenied → matching
//     rejection, ErrForbidden → AccessDenied;
//  4. every snapshot record is well-formed, belongs to this tree, does not
//     contradict an already stored snapshot, has known parents (stored or
//     in this request), and is an ancestor of new_tip; snapshot_chain ids
//     without a record must already be stored;
//  5. every manifest and blob referenced by the new snapshots is present in
//     the object store — for chunked entries the chunk list (which must
//     decode) and every chunk (FailedPrecondition otherwise);
//  6. new_tip descends from the current server tip — else
//     ConflictDetected{server_tip};
//  7. License.CheckPush on the paths that differ between the server tip's
//     manifest and new_tip's manifest;
//  8. snapshots are stored (parents first) and the branch tip is moved with
//     compare-and-swap; losing a race yields ConflictDetected.
//
// The client's old_tip is advisory: the server compares against its own
// tip, so a stale old_tip still succeeds when new_tip is a fast-forward.
// Re-pushing the current tip is an idempotent success. On success the
// pusher's staged entry for the branch is cleared, EventBranchPushed is
// published and "branch.push" is audited.
func (s *Service) Push(ctx context.Context, req *syncpb.PushRequest) (*syncpb.PushResponse, error) {
	p, err := requireAuth(ctx)
	if err != nil {
		return nil, err
	}
	branch := req.GetBranchName()
	if branch == "" {
		branch = req.GetBranchId().GetValue()
	}
	if err := validateRefName("branch", branch); err != nil {
		return nil, err
	}
	newTip := req.GetNewTip().GetValue()
	if !validID(newTip) {
		return nil, invalidf("new_tip is missing or malformed")
	}
	if len(req.GetSnapshots()) > maxPushSnapshots {
		return nil, invalidf("too many snapshots in one push (max %d)", maxPushSnapshots)
	}
	t, err := s.loadTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}

	// 2. Authorization. The checks of this push (tree:write, branch:push and
	// branch protection) share one memoized read of the caller's IAM data.
	ctx = authz.WithRequestCache(ctx)
	// A token scoped to branch:push may push: its tree:write check is made
	// with that scope widened, and branch:push is then evaluated as usual.
	if err := s.d.Authz.Authorize(ctx, pushScoped(p), domain.ActTreeWrite, t.TenantID, t); err != nil {
		if errors.Is(err, domain.ErrForbidden) {
			s.auditRejected(ctx, p, t, branch, newTip, "access_denied")
			return rejected(accessDenied("you do not have write access to this tree")), nil
		}
		return nil, s.toStatus(ctx, err, "authorize")
	}
	if ev, ok := s.d.Authz.(domain.AccessEvaluator); ok {
		res := domain.Resource{TenantID: t.TenantID, Tree: t, Branch: branch}
		d, err := ev.Evaluate(ctx, p, domain.ActBranchPush, res, authz.RequestEnvFrom(ctx))
		if err != nil {
			return nil, s.toStatus(ctx, err, "authorize")
		}
		if !d.Allowed {
			s.auditRejected(ctx, p, t, branch, newTip, "access_denied")
			return rejected(accessDenied("pushing to branch " + branch + " is denied by an access policy")), nil
		}
	}

	// 3. Branch protection.
	if s.d.Review != nil {
		if err := s.d.Review.CheckPush(ctx, p, t, branch); err != nil {
			return s.policyRejection(ctx, p, t, branch, newTip, err, "branch protection check")
		}
	}

	// 4. Snapshot records.
	pl, err := s.planPush(ctx, t, branch, newTip, req)
	if err != nil {
		return nil, err
	}

	// 5. Referenced objects.
	if err := s.checkObjects(ctx, pl); err != nil {
		return nil, err
	}

	// 6. Fast-forward check against the server tip.
	serverTip, err := s.branchTip(ctx, t.ID, branch)
	if err != nil {
		return nil, s.toStatus(ctx, err, "branch")
	}
	if serverTip == newTip {
		return &syncpb.PushResponse{Accepted: true, NewTip: snapshotIDToPB(newTip)}, nil
	}
	if serverTip != "" {
		ok, err := s.descends(ctx, pl, serverTip)
		if err != nil {
			return nil, s.toStatus(ctx, err, "ancestry check")
		}
		if !ok {
			return rejected(conflict(serverTip)), nil
		}
	}

	// 7. License policy on changed paths, then the pluggable v0.5 checks
	// (path ACLs, quotas, signatures; ENGINEERING_PLAN §13.1).
	if s.d.License != nil || len(s.d.PushChecks) > 0 {
		changed, err := s.changedPaths(ctx, pl, serverTip)
		if err != nil {
			return nil, s.toStatus(ctx, err, "compute changed paths")
		}
		if s.d.License != nil {
			if err := s.d.License.CheckPush(ctx, p, t, changed); err != nil {
				return s.policyRejection(ctx, p, t, branch, newTip, err, "license check")
			}
		}
		if len(s.d.PushChecks) > 0 {
			plan := s.pushPlanFor(pl, branch, serverTip, changed)
			for _, c := range s.d.PushChecks {
				if err := c.CheckPushPlan(ctx, p, t, plan); err != nil {
					return s.policyRejection(ctx, p, t, branch, newTip, err, "push check")
				}
			}
		}
	}

	// 8. Persist snapshots, then move the tip.
	now := s.now()
	for _, snap := range pl.newSnaps {
		snap.PushedBy = p.AccountID
		snap.PushedAt = now
		if err := s.putSnapshot(ctx, snap); err != nil {
			return nil, err
		}
	}
	if err := s.d.Store.CompareAndSwapBranchTip(ctx, t.ID, branch, serverTip, newTip); err != nil {
		if !errors.Is(err, domain.ErrConflict) {
			return nil, s.toStatus(ctx, err, "update branch")
		}
		current, terr := s.branchTip(ctx, t.ID, branch)
		if terr != nil {
			return nil, s.toStatus(ctx, terr, "branch")
		}
		if current == newTip {
			// A concurrent push of the very same tip won; that is success.
			return &syncpb.PushResponse{Accepted: true, NewTip: snapshotIDToPB(newTip)}, nil
		}
		return rejected(conflict(current)), nil
	}

	if s.d.Staged != nil {
		if err := s.d.Staged.ClearOnPush(ctx, p, t, branch); err != nil && !errors.Is(err, domain.ErrNotFound) {
			s.log.WarnContext(ctx, "push: clearing staged snapshot failed", "tree", t.ID, "branch", branch, "err", err)
		}
	}
	s.publish(domain.Event{
		Type:     domain.EventBranchPushed,
		TenantID: t.TenantID,
		TreeID:   t.ID,
		Actor:    p.Username,
		Payload: map[string]any{
			"branch":  branch,
			"old_tip": serverTip,
			"new_tip": newTip,
			"count":   len(pl.newSnaps),
		},
	})
	s.audit(ctx, p, domain.AuditEvent{
		TenantID:     t.TenantID,
		Action:       "branch.push",
		ResourceType: "branch",
		ResourceID:   t.ID + "/" + branch,
		Details: map[string]string{
			"tree":    t.Name,
			"branch":  branch,
			"old_tip": serverTip,
			"new_tip": newTip,
			"count":   fmt.Sprint(len(pl.newSnaps)),
		},
	})
	return &syncpb.PushResponse{Accepted: true, NewTip: snapshotIDToPB(newTip)}, nil
}

// pushScoped returns p with a "tree:write" scope added when one of its
// token scopes names branch:push explicitly ("branch:push" or "branch:*"),
// so branch-push tokens pass the tree-level write check of Push.
func pushScoped(p *domain.Principal) *domain.Principal {
	if p == nil || len(p.Scopes) == 0 {
		return p
	}
	for _, sc := range p.Scopes {
		if sc == string(domain.ActBranchPush) || sc == "branch:*" {
			q := *p
			q.Scopes = append(append([]string(nil), p.Scopes...), string(domain.ActTreeWrite))
			return &q
		}
	}
	return p
}

// pushPlan is the validated content of a push request.
type pushPlan struct {
	tree   *domain.Tree
	newTip string
	// records holds every snapshot supplied in the request, by id.
	records map[string]*domain.Snapshot
	// newSnaps are the records not yet stored, parents before children.
	newSnaps []*domain.Snapshot
	// frontier are stored snapshot ids directly referenced as parents by
	// new snapshots (or new_tip itself when it is already stored).
	frontier []string
	// manifests caches decoded manifests by hash.
	manifests map[hash.Hash]*syncpb.ManifestRecord
	// chunkObjects are the chunks of chunked entries found by checkObjects.
	chunkObjects []hash.Hash
}

// planPush validates the snapshot records of req (step 4 of Push).
func (s *Service) planPush(ctx context.Context, t *domain.Tree, branch, newTip string, req *syncpb.PushRequest) (*pushPlan, error) {
	pl := &pushPlan{
		tree:      t,
		newTip:    newTip,
		records:   make(map[string]*domain.Snapshot, len(req.GetSnapshots())),
		manifests: make(map[hash.Hash]*syncpb.ManifestRecord),
	}
	now := s.now()
	for _, rec := range req.GetSnapshots() {
		snap, err := snapshotFromPB(rec)
		if err != nil {
			return nil, invalidf("%v", err)
		}
		if snap.TreeID != t.ID {
			return nil, invalidf("snapshot %s has tree_id %q, expected %q", snap.ID, snap.TreeID, t.ID)
		}
		if _, dup := pl.records[snap.ID]; dup {
			return nil, invalidf("snapshot %s is listed twice", snap.ID)
		}
		if snap.BranchName == "" {
			snap.BranchName = branch
		}
		if snap.CreatedAt.IsZero() {
			snap.CreatedAt = now
		}
		pl.records[snap.ID] = &snap
	}

	// stored caches lookups of snapshots on the server: nil = not stored.
	stored := make(map[string]*domain.Snapshot)
	lookup := func(id string) (*domain.Snapshot, error) {
		if v, ok := stored[id]; ok {
			return v, nil
		}
		got, err := s.d.Store.GetSnapshot(ctx, id)
		if errors.Is(err, domain.ErrNotFound) {
			stored[id] = nil
			return nil, nil
		}
		if err != nil {
			return nil, s.toStatus(ctx, err, "snapshot")
		}
		stored[id] = got
		return got, nil
	}
	// knownStored reports whether id is stored and belongs to this tree.
	knownStored := func(id string) (bool, error) {
		got, err := lookup(id)
		if err != nil {
			return false, err
		}
		return got != nil && got.TreeID == t.ID, nil
	}

	// Records that already exist must match the stored content.
	isNew := make(map[string]bool, len(pl.records))
	for id, rec := range pl.records {
		got, err := lookup(id)
		if err != nil {
			return nil, err
		}
		if got == nil {
			isNew[id] = true
			continue
		}
		if !sameSnapshotContent(got, rec) {
			return nil, status.Errorf(codes.FailedPrecondition, "snapshot %s already exists with different content", id)
		}
	}

	for _, c := range req.GetSnapshotChain() {
		id := c.GetValue()
		if !validID(id) {
			return nil, invalidf("snapshot_chain contains a malformed id")
		}
		if _, ok := pl.records[id]; ok {
			continue
		}
		ok, err := knownStored(id)
		if err != nil {
			return nil, err
		}
		if !ok {
			return nil, status.Errorf(codes.FailedPrecondition, "snapshot %s in snapshot_chain is unknown and no record was supplied", id)
		}
	}

	// new_tip must be known.
	if _, ok := pl.records[newTip]; !ok {
		ok, err := knownStored(newTip)
		if err != nil {
			return nil, err
		}
		if !ok {
			return nil, status.Errorf(codes.FailedPrecondition, "new_tip %s is unknown: supply its snapshot record", newTip)
		}
	}

	// Walk from new_tip through new records (DFS post-order → parents
	// first), checking parents and detecting cycles.
	const (
		visiting = 1
		done     = 2
	)
	state := make(map[string]int, len(isNew))
	frontier := make(map[string]struct{})
	var visit func(id, child string) error
	visit = func(id, child string) error {
		if !isNew[id] {
			if _, inReq := pl.records[id]; !inReq {
				ok, err := knownStored(id)
				if err != nil {
					return err
				}
				if !ok {
					return status.Errorf(codes.FailedPrecondition, "parent %s of snapshot %s is unknown: push its ancestors first", id, child)
				}
			}
			frontier[id] = struct{}{}
			return nil
		}
		switch state[id] {
		case done:
			return nil
		case visiting:
			return invalidf("snapshot ancestry contains a cycle through %s", id)
		}
		state[id] = visiting
		rec := pl.records[id]
		for _, parent := range rec.Parents {
			if err := visit(parent, id); err != nil {
				return err
			}
		}
		state[id] = done
		pl.newSnaps = append(pl.newSnaps, rec)
		return nil
	}
	if err := visit(newTip, newTip); err != nil {
		return nil, err
	}
	for id := range isNew {
		if state[id] != done {
			return nil, invalidf("snapshot %s is not an ancestor of new_tip %s", id, newTip)
		}
	}
	for id := range frontier {
		pl.frontier = append(pl.frontier, id)
	}
	return pl, nil
}

// checkObjects verifies that every manifest and blob referenced by the new
// snapshots is stored (step 5 of Push).
func (s *Service) checkObjects(ctx context.Context, pl *pushPlan) error {
	var missing []string
	nMissing := 0
	note := func(h hash.Hash) {
		nMissing++
		if len(missing) < maxMissingReported {
			missing = append(missing, h.Hex())
		}
	}
	checked := make(map[hash.Hash]struct{})
	for _, snap := range pl.newSnaps {
		mh := snap.ManifestHash
		if _, ok := pl.manifests[mh]; ok {
			continue
		}
		m, err := s.loadManifest(ctx, mh)
		if errors.Is(err, objstore.ErrNotFound) {
			if _, seen := checked[mh]; !seen {
				checked[mh] = struct{}{}
				note(mh)
			}
			continue
		}
		if err != nil {
			if errors.Is(err, errInvalidManifest) {
				return status.Errorf(codes.FailedPrecondition, "manifest %s of snapshot %s is not a valid ManifestRecord", mh.Hex(), snap.ID)
			}
			return s.toStatus(ctx, err, "read manifest")
		}
		pl.manifests[mh] = m
		for _, e := range m.GetEntries() {
			bh := hash.Hash(e.GetBlobHash().GetValue())
			if _, seen := checked[bh]; seen {
				continue
			}
			checked[bh] = struct{}{}
			ok, err := s.d.Objects.Has(ctx, bh)
			if err != nil {
				return s.toStatus(ctx, err, "object store")
			}
			if !ok {
				note(bh)
				continue
			}
			if !e.GetChunked() {
				continue
			}
			// Chunked large file (ENGINEERING_PLAN §13.2): the chunk list
			// must decode and every chunk must be stored.
			objs, err := content.EntryObjects(ctx, s.d.Objects, e)
			if err != nil {
				if errors.Is(err, content.ErrInvalidChunkList) {
					return status.Errorf(codes.FailedPrecondition, "chunk list %s of %s (snapshot %s) is not a valid ChunkList", bh.Hex(), e.GetPath(), snap.ID)
				}
				return s.toStatus(ctx, err, "read chunk list")
			}
			for _, ch := range objs[1:] {
				pl.chunkObjects = append(pl.chunkObjects, ch)
				if _, seen := checked[ch]; seen {
					continue
				}
				checked[ch] = struct{}{}
				ok, err := s.d.Objects.Has(ctx, ch)
				if err != nil {
					return s.toStatus(ctx, err, "object store")
				}
				if !ok {
					note(ch)
				}
			}
		}
	}
	if nMissing > 0 {
		return status.Errorf(codes.FailedPrecondition,
			"%d referenced object(s) are missing; upload them first (e.g. %s)", nMissing, strings.Join(missing, ", "))
	}
	return nil
}

// pushPlanFor builds the domain view of a validated push for PushCheckers.
func (s *Service) pushPlanFor(pl *pushPlan, branch, serverTip string, changed []string) *domain.PushPlan {
	seen := make(map[hash.Hash]struct{})
	var objs []hash.Hash
	add := func(h hash.Hash) {
		if _, ok := seen[h]; !ok {
			seen[h] = struct{}{}
			objs = append(objs, h)
		}
	}
	for _, snap := range pl.newSnaps {
		add(snap.ManifestHash)
		if m, ok := pl.manifests[snap.ManifestHash]; ok {
			for _, e := range m.GetEntries() {
				add(hash.Hash(e.GetBlobHash().GetValue()))
			}
		}
	}
	for _, ch := range pl.chunkObjects {
		add(ch)
	}
	return &domain.PushPlan{
		Branch:       branch,
		OldTip:       serverTip,
		NewTip:       pl.newTip,
		NewSnapshots: pl.newSnaps,
		ChangedPaths: changed,
		NewObjects:   objs,
		ObjectSize: func(ctx context.Context, h hash.Hash) int64 {
			n, err := objstore.ObjectSize(ctx, s.d.Objects, h)
			if err != nil {
				return 0
			}
			return n
		},
	}
}

// descends reports whether the push's new tip descends from serverTip
// (step 6 of Push). The new snapshots are not stored yet, so ancestry is
// resolved through the frontier of stored snapshots they build on.
func (s *Service) descends(ctx context.Context, pl *pushPlan, serverTip string) (bool, error) {
	for _, f := range pl.frontier {
		ok, err := s.d.History.IsAncestor(ctx, serverTip, f)
		if err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				continue
			}
			return false, err
		}
		if ok {
			return true, nil
		}
	}
	return false, nil
}

// changedPaths returns the sorted paths that differ (added, removed,
// content or mode changed) between the server tip's manifest and the new
// tip's manifest (step 7 of Push).
func (s *Service) changedPaths(ctx context.Context, pl *pushPlan, serverTip string) ([]string, error) {
	newM, err := s.manifestOf(ctx, pl, pl.newTip)
	if err != nil {
		return nil, err
	}
	oldM := &syncpb.ManifestRecord{}
	if serverTip != "" {
		if oldM, err = s.manifestOf(ctx, pl, serverTip); err != nil {
			return nil, err
		}
	}
	return diffManifests(oldM, newM), nil
}

// manifestOf returns the decoded manifest of snapshot id (from the request
// or the store).
func (s *Service) manifestOf(ctx context.Context, pl *pushPlan, id string) (*syncpb.ManifestRecord, error) {
	snap, ok := pl.records[id]
	if !ok {
		var err error
		if snap, err = s.d.Store.GetSnapshot(ctx, id); err != nil {
			return nil, err
		}
	}
	if m, ok := pl.manifests[snap.ManifestHash]; ok {
		return m, nil
	}
	m, err := s.loadManifest(ctx, snap.ManifestHash)
	if err != nil {
		return nil, fmt.Errorf("manifest of snapshot %s: %w", id, err)
	}
	pl.manifests[snap.ManifestHash] = m
	return m, nil
}

// diffManifests merges two path-sorted manifests and returns the paths
// whose entry was added, removed or changed.
func diffManifests(a, b *syncpb.ManifestRecord) []string {
	ae, be := a.GetEntries(), b.GetEntries()
	out := make([]string, 0)
	i, j := 0, 0
	for i < len(ae) || j < len(be) {
		switch {
		case j >= len(be) || (i < len(ae) && ae[i].GetPath() < be[j].GetPath()):
			out = append(out, ae[i].GetPath())
			i++
		case i >= len(ae) || be[j].GetPath() < ae[i].GetPath():
			out = append(out, be[j].GetPath())
			j++
		default:
			x, y := ae[i], be[j]
			if string(x.GetBlobHash().GetValue()) != string(y.GetBlobHash().GetValue()) || x.GetExecutable() != y.GetExecutable() {
				out = append(out, x.GetPath())
			}
			i++
			j++
		}
	}
	return out
}

// putSnapshot stores snap, treating a concurrent identical write (which
// differs only in push metadata) as success.
func (s *Service) putSnapshot(ctx context.Context, snap *domain.Snapshot) error {
	err := s.d.Store.PutSnapshot(ctx, snap)
	if err == nil {
		return nil
	}
	if errors.Is(err, domain.ErrConflict) || errors.Is(err, domain.ErrAlreadyExists) {
		got, gerr := s.d.Store.GetSnapshot(ctx, snap.ID)
		if gerr == nil && sameSnapshotContent(got, snap) {
			return nil
		}
		return status.Errorf(codes.FailedPrecondition, "snapshot %s already exists with different content", snap.ID)
	}
	return s.toStatus(ctx, err, "store snapshot")
}

// branchTip returns the tip of a branch, "" when it does not exist.
func (s *Service) branchTip(ctx context.Context, treeID, branch string) (string, error) {
	b, err := s.d.Store.GetBranch(ctx, treeID, branch)
	if errors.Is(err, domain.ErrNotFound) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	return b.Tip, nil
}

// policyRejection converts a policy-check error into a rejected
// PushResponse, or a status error when err is not a policy refusal.
func (s *Service) policyRejection(ctx context.Context, p *domain.Principal, t *domain.Tree, branch, newTip string, err error, what string) (*syncpb.PushResponse, error) {
	var denied *domain.PushDenied
	switch {
	case errors.As(err, &denied):
		s.auditRejected(ctx, p, t, branch, newTip, denied.Kind)
		return rejected(s.rejectionFor(ctx, t, branch, denied)), nil
	case errors.Is(err, domain.ErrForbidden):
		s.auditRejected(ctx, p, t, branch, newTip, "access_denied")
		return rejected(accessDenied("push to " + branch + " is not allowed")), nil
	}
	return nil, s.toStatus(ctx, err, what)
}

// rejectionFor maps a *PushDenied to the matching PushRejection oneof.
// Unknown kinds become AccessDenied.
func (s *Service) rejectionFor(ctx context.Context, t *domain.Tree, branch string, d *domain.PushDenied) *syncpb.PushRejection {
	switch d.Kind {
	case "branch_protection":
		return &syncpb.PushRejection{Reason: &syncpb.PushRejection_BranchProtection_{
			BranchProtection: &syncpb.PushRejection_BranchProtection{Rule: d.Reason},
		}}
	case "license_violation":
		return &syncpb.PushRejection{Reason: &syncpb.PushRejection_LicenseViolation_{
			LicenseViolation: &syncpb.PushRejection_LicenseViolation{Path: d.Path, License: d.License},
		}}
	case "review_required":
		return &syncpb.PushRejection{Reason: &syncpb.PushRejection_ReviewRequired_{
			ReviewRequired: &syncpb.PushRejection_ReviewRequired{Required: s.requiredApprovals(ctx, t, branch)},
		}}
	case "quota_exceeded":
		limit := d.Reason
		if limit == "" {
			limit = "quota"
		}
		return &syncpb.PushRejection{Reason: &syncpb.PushRejection_QuotaExceeded_{
			QuotaExceeded: &syncpb.PushRejection_QuotaExceeded{Limit: limit},
		}}
	default: // "access_denied" and anything unknown
		reason := d.Reason
		if reason == "" {
			reason = "access denied"
		}
		return accessDenied(reason)
	}
}

// requiredApprovals returns the largest RequiredApprovals of the
// protections matching branch (best effort; 0 when unknown). A direct push
// carries no approvals, so `current` is always 0.
func (s *Service) requiredApprovals(ctx context.Context, t *domain.Tree, branch string) uint32 {
	prots, err := s.d.Store.ListProtections(ctx, t.ID)
	if err != nil {
		return 0
	}
	var n int
	for _, pr := range prots {
		if ok, _ := path.Match(pr.Pattern, branch); ok && pr.RequiredApprovals > n {
			n = pr.RequiredApprovals
		}
	}
	return uint32(n)
}

// auditRejected records a refused push.
func (s *Service) auditRejected(ctx context.Context, p *domain.Principal, t *domain.Tree, branch, newTip, kind string) {
	s.audit(ctx, p, domain.AuditEvent{
		TenantID:     t.TenantID,
		Action:       "branch.push_rejected",
		ResourceType: "branch",
		ResourceID:   t.ID + "/" + branch,
		Details:      map[string]string{"tree": t.Name, "branch": branch, "new_tip": newTip, "reason": kind},
	})
}

func rejected(r *syncpb.PushRejection) *syncpb.PushResponse {
	return &syncpb.PushResponse{Accepted: false, RejectionReason: r}
}

func accessDenied(reason string) *syncpb.PushRejection {
	return &syncpb.PushRejection{Reason: &syncpb.PushRejection_AccessDenied_{
		AccessDenied: &syncpb.PushRejection_AccessDenied{Reason: reason},
	}}
}

func conflict(serverTip string) *syncpb.PushRejection {
	return &syncpb.PushRejection{Reason: &syncpb.PushRejection_ConflictDetected_{
		ConflictDetected: &syncpb.PushRejection_ConflictDetected{ServerTip: snapshotIDToPB(serverTip)},
	}}
}
