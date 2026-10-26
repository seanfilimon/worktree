package fork

import (
	"context"
	"crypto/rand"
	"errors"
	"fmt"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork/snapcopy"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// Branch relations between a fork and its upstream (BranchStatus.Status).
const (
	StatusUpToDate     = "up_to_date"    // same content
	StatusAhead        = "ahead"         // the fork contains the upstream tip
	StatusBehind       = "behind"        // the fork can fast-forward to the upstream
	StatusDiverged     = "diverged"      // both have snapshots the other lacks
	StatusForkOnly     = "fork_only"     // the branch exists only in the fork
	StatusUpstreamOnly = "upstream_only" // the branch exists only in the upstream
)

// Sync outcomes (SyncBranch.Result).
const (
	ResultFastForwarded = "fast_forwarded"
	ResultCreated       = "created"
	ResultUpToDate      = "up_to_date"
	ResultAhead         = "ahead"
	ResultDiverged      = "diverged"
	ResultUpstreamOnly  = "upstream_only"
	ResultMoved         = "moved" // the fork branch moved during the sync; retry
)

// Fork is one fork of a tree as listed by List.
type Fork struct {
	TreeID      string     `json:"tree_id"`
	TenantID    string     `json:"tenant_id"`
	Tenant      string     `json:"tenant"` // tenant slug
	Name        string     `json:"name"`
	Description string     `json:"description"`
	Visibility  string     `json:"visibility"`
	Branches    []string   `json:"branches"`
	CreatedBy   string     `json:"created_by"`
	CreatedAt   time.Time  `json:"created_at"`
	LastSyncAt  *time.Time `json:"last_sync_at,omitempty"`
}

// List returns the forks of tree the caller may read, sorted by tenant and
// name. The caller needs tree:read on tree.
func (s *Service) List(ctx context.Context, p *domain.Principal, tree *domain.Tree) ([]Fork, error) {
	if err := s.authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	recs, err := meta.ListDocsT[Record](ctx, s.d.Store, Coll, meta.DocPrefix(tree.ID))
	if err != nil {
		return nil, fmt.Errorf("fork: list forks: %w", err)
	}
	out := []Fork{}
	for _, r := range recs {
		ft, err := s.d.Store.GetTree(ctx, r.TreeID)
		if errors.Is(err, domain.ErrNotFound) {
			continue // deleted; its record goes on tree.deleted
		}
		if err != nil {
			return nil, fmt.Errorf("fork: tree %s: %w", r.TreeID, err)
		}
		if !s.canRead(ctx, p, ft) {
			continue
		}
		out = append(out, Fork{
			TreeID: ft.ID, TenantID: ft.TenantID, Tenant: s.tenantSlug(ctx, ft.TenantID), Name: ft.Name,
			Description: ft.Description, Visibility: ft.Visibility, Branches: nonNil(r.Branches),
			CreatedBy: r.CreatedBy, CreatedAt: r.CreatedAt, LastSyncAt: r.LastSyncAt,
		})
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Tenant != out[j].Tenant {
			return out[i].Tenant < out[j].Tenant
		}
		return out[i].Name < out[j].Name
	})
	return out, nil
}

// Upstream describes the upstream of a tree (GET {tree}/fork).
type Upstream struct {
	// Fork is false for trees that are not forks (the other fields are
	// then empty).
	Fork           bool   `json:"fork"`
	UpstreamTreeID string `json:"upstream_tree_id,omitempty"`
	// UpstreamMissing: the upstream was deleted; the fork is intact.
	UpstreamMissing bool `json:"upstream_missing"`
	// UpstreamHidden: the caller may not read the upstream.
	UpstreamHidden bool           `json:"upstream_hidden"`
	Tenant         string         `json:"tenant,omitempty"` // upstream tenant slug
	Name           string         `json:"name,omitempty"`
	Visibility     string         `json:"visibility,omitempty"`
	DefaultBranch  string         `json:"default_branch,omitempty"`
	ForkedBy       string         `json:"forked_by,omitempty"`
	ForkedAt       *time.Time     `json:"forked_at,omitempty"`
	LastSyncAt     *time.Time     `json:"last_sync_at,omitempty"`
	Branches       []BranchStatus `json:"branches"`
}

// BranchStatus relates a fork branch to the upstream branch of that name.
type BranchStatus struct {
	Name        string `json:"name"`
	Tip         string `json:"tip"`          // fork tip ("" = absent or empty)
	UpstreamTip string `json:"upstream_tip"` // upstream tip (an upstream snapshot id)
	Status      string `json:"status"`
}

// Upstream returns the upstream of tree with the relation of every branch
// (when the caller may read the upstream). The caller needs tree:read on
// tree.
func (s *Service) Upstream(ctx context.Context, p *domain.Principal, tree *domain.Tree) (*Upstream, error) {
	if err := s.authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	out := &Upstream{Branches: []BranchStatus{}}
	if tree.ForkOf == "" {
		return out, nil
	}
	out.Fork, out.UpstreamTreeID = true, tree.ForkOf
	up, err := s.d.Store.GetTree(ctx, tree.ForkOf)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		out.UpstreamMissing = true
		return out, nil
	case err != nil:
		return nil, fmt.Errorf("fork: upstream: %w", err)
	}
	if !s.canRead(ctx, p, up) {
		out.UpstreamHidden = true
		return out, nil
	}
	out.Tenant, out.Name, out.Visibility, out.DefaultBranch = s.tenantSlug(ctx, up.TenantID), up.Name, up.Visibility, up.DefaultBranch
	var rec Record
	if err := s.d.Store.GetDoc(ctx, Coll, domain.DocKey(up.ID, tree.ID), &rec); err == nil {
		at := rec.CreatedAt
		out.ForkedBy, out.ForkedAt, out.LastSyncAt = rec.CreatedBy, &at, rec.LastSyncAt
	} else if !errors.Is(err, domain.ErrNotFound) {
		return nil, fmt.Errorf("fork: record: %w", err)
	}
	out.Branches, err = s.statuses(ctx, up, tree)
	if err != nil {
		return nil, err
	}
	return out, nil
}

// statuses compares every branch of fork with up, sorted by name.
func (s *Service) statuses(ctx context.Context, up, fork *domain.Tree) ([]BranchStatus, error) {
	ub, err := s.d.Store.ListBranches(ctx, up.ID)
	if err != nil {
		return nil, fmt.Errorf("fork: list upstream branches: %w", err)
	}
	fb, err := s.d.Store.ListBranches(ctx, fork.ID)
	if err != nil {
		return nil, fmt.Errorf("fork: list branches: %w", err)
	}
	upTips := map[string]string{}
	for _, b := range ub {
		upTips[b.Name] = b.Tip
	}
	out := []BranchStatus{}
	for _, b := range fb {
		ut, ok := upTips[b.Name]
		st := BranchStatus{Name: b.Name, Tip: b.Tip, UpstreamTip: ut, Status: StatusForkOnly}
		if ok {
			if st.Status, err = s.relation(ctx, up, fork, ut, b.Tip); err != nil {
				return nil, err
			}
			delete(upTips, b.Name)
		}
		out = append(out, st)
	}
	for name, ut := range upTips {
		out = append(out, BranchStatus{Name: name, UpstreamTip: ut, Status: StatusUpstreamOnly})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out, nil
}

// relation classifies fork tip ft against upstream tip ut without copying.
func (s *Service) relation(ctx context.Context, up, fork *domain.Tree, ut, ft string) (string, error) {
	switch {
	case ut == "" && ft == "":
		return StatusUpToDate, nil
	case ut == "":
		return StatusAhead, nil
	case ft == "":
		return StatusBehind, nil
	}
	mapped, present, err := snapcopy.Existing(ctx, s.d.Store, up.ID, fork.ID, ut)
	if err != nil {
		return "", err
	}
	if present {
		if mapped == ft {
			return StatusUpToDate, nil
		}
		if ok, err := s.isAncestor(ctx, mapped, ft); err != nil || ok {
			return StatusAhead, err
		}
		if ok, err := s.isAncestor(ctx, ft, mapped); err != nil || ok {
			return StatusBehind, err
		}
		return StatusDiverged, nil
	}
	// The upstream tip is not in the fork yet: behind when the fork tip's
	// upstream counterpart is an ancestor of it.
	back, ok, err := snapcopy.Existing(ctx, s.d.Store, fork.ID, up.ID, ft)
	if err != nil {
		return "", err
	}
	if ok {
		if anc, err := s.isAncestor(ctx, back, ut); err != nil || anc {
			return StatusBehind, err
		}
	}
	return StatusDiverged, nil
}

func (s *Service) isAncestor(ctx context.Context, ancestor, descendant string) (bool, error) {
	if s.d.History == nil {
		return ancestor == descendant, nil
	}
	ok, err := s.d.History.IsAncestor(ctx, ancestor, descendant)
	if err != nil {
		return false, fmt.Errorf("fork: ancestry: %w", err)
	}
	return ok, nil
}

// SyncResult is the outcome of Sync.
type SyncResult struct {
	Branches []SyncBranch `json:"branches"`
	// Updated counts fast-forwarded and created branches.
	Updated int `json:"updated"`
	// Diverged names the branches that could not be fast-forwarded.
	Diverged []string `json:"diverged"`
}

// SyncBranch is the outcome of one branch.
type SyncBranch struct {
	Name   string `json:"name"`
	Result string `json:"result"`
	OldTip string `json:"old_tip,omitempty"`
	NewTip string `json:"new_tip,omitempty"`
}

// Sync fast-forwards the branches of fork to its upstream where possible
// and reports the others. Without branches it syncs every branch both
// trees have (upstream-only branches are reported, not created); named
// branches that exist only upstream are created. The caller needs
// tree:write on fork and tree:read on the upstream. Branch protections of
// the fork do not apply: a sync only fast-forwards to upstream content.
func (s *Service) Sync(ctx context.Context, p *domain.Principal, fork *domain.Tree, branches []string) (*SyncResult, error) {
	if p == nil {
		return nil, fmt.Errorf("syncing a fork requires a signed-in caller: %w", domain.ErrUnauthenticated)
	}
	if err := s.authorize(ctx, p, domain.ActTreeWrite, fork.TenantID, fork); err != nil {
		return nil, err
	}
	if fork.ForkOf == "" {
		return nil, invalidf("tree %s is not a fork", fork.Name)
	}
	up, err := s.d.Store.GetTree(ctx, fork.ForkOf)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, fmt.Errorf("the upstream of %s no longer exists (upstream_missing): %w", fork.Name, domain.ErrConflict)
	}
	if err != nil {
		return nil, fmt.Errorf("fork: upstream: %w", err)
	}
	if err := s.authorize(ctx, p, domain.ActTreeRead, up.TenantID, up); err != nil {
		return nil, err
	}

	ub, err := s.d.Store.ListBranches(ctx, up.ID)
	if err != nil {
		return nil, fmt.Errorf("fork: list upstream branches: %w", err)
	}
	fb, err := s.d.Store.ListBranches(ctx, fork.ID)
	if err != nil {
		return nil, fmt.Errorf("fork: list branches: %w", err)
	}
	upTips := map[string]string{}
	for _, b := range ub {
		upTips[b.Name] = b.Tip
	}
	forkTips := map[string]string{}
	for _, b := range fb {
		forkTips[b.Name] = b.Tip
	}
	explicit := len(branches) > 0
	var names []string
	if explicit {
		for _, n := range branches {
			n = strings.TrimSpace(n)
			if _, ok := upTips[n]; !ok {
				return nil, fmt.Errorf("branch '%s' not found in the upstream: %w", n, domain.ErrNotFound)
			}
			if !slices.Contains(names, n) {
				names = append(names, n)
			}
		}
	} else {
		for n := range upTips {
			names = append(names, n)
		}
	}
	sort.Strings(names)

	res := &SyncResult{Branches: []SyncBranch{}, Diverged: []string{}}
	for _, name := range names {
		ft, inFork := forkTips[name]
		if !inFork && !explicit {
			res.Branches = append(res.Branches, SyncBranch{Name: name, Result: ResultUpstreamOnly})
			continue
		}
		sb, err := s.syncBranch(ctx, p, up, fork, name, upTips[name], ft, inFork)
		if err != nil {
			return nil, err
		}
		switch sb.Result {
		case ResultFastForwarded, ResultCreated:
			res.Updated++
		case ResultDiverged:
			res.Diverged = append(res.Diverged, name)
		}
		res.Branches = append(res.Branches, sb)
	}

	now := s.now()
	key := domain.DocKey(up.ID, fork.ID)
	if err := meta.UpdateDocT(ctx, s.d.Store, Coll, key, func(cur *Record) (*Record, error) {
		if cur == nil {
			cur = &Record{UpstreamTreeID: up.ID, UpstreamTenantID: up.TenantID, TreeID: fork.ID, TenantID: fork.TenantID, CreatedAt: fork.CreatedAt}
		}
		cur.LastSyncAt = &now
		return cur, nil
	}); err != nil {
		return nil, fmt.Errorf("fork: record sync: %w", err)
	}
	s.audit(ctx, p, domain.AuditEvent{
		TenantID: fork.TenantID, Action: "fork.sync", ResourceType: "tree", ResourceID: fork.ID,
		Details: map[string]string{"upstream_tree_id": up.ID, "updated": fmt.Sprint(res.Updated), "diverged": strings.Join(res.Diverged, ",")},
	})
	return res, nil
}

// syncBranch fast-forwards (or creates) one fork branch.
func (s *Service) syncBranch(ctx context.Context, p *domain.Principal, up, fork *domain.Tree, name, ut, ft string, inFork bool) (SyncBranch, error) {
	sb := SyncBranch{Name: name, OldTip: ft}
	rel := StatusBehind
	if inFork {
		var err error
		if rel, err = s.relation(ctx, up, fork, ut, ft); err != nil {
			return sb, err
		}
	}
	switch rel {
	case StatusUpToDate:
		sb.Result, sb.NewTip = ResultUpToDate, ft
		return sb, nil
	case StatusAhead:
		sb.Result, sb.NewTip = ResultAhead, ft
		return sb, nil
	case StatusDiverged:
		sb.Result = ResultDiverged
		return sb, nil
	}
	newTip, err := snapcopy.Copy(ctx, s.d.Store, up.ID, fork.ID, ut)
	if err != nil {
		return sb, fmt.Errorf("fork: copy branch '%s': %w", name, err)
	}
	if ft != "" {
		// Re-check on the copied tip: only fast-forwards move the branch.
		if ok, err := s.isAncestor(ctx, ft, newTip); err != nil {
			return sb, err
		} else if !ok {
			sb.Result = ResultDiverged
			return sb, nil
		}
	}
	if err := s.d.Store.CompareAndSwapBranchTip(ctx, fork.ID, name, ft, newTip); err != nil {
		if errors.Is(err, domain.ErrConflict) {
			sb.Result = ResultMoved
			return sb, nil
		}
		return sb, fmt.Errorf("fork: update branch '%s': %w", name, err)
	}
	sb.NewTip = newTip
	sb.Result = ResultFastForwarded
	if !inFork {
		sb.Result = ResultCreated
		s.publish(domain.Event{Type: domain.EventBranchCreated, TenantID: fork.TenantID, TreeID: fork.ID, Actor: p.Username,
			Payload: map[string]any{"branch": name, "tip": newTip, "via": "fork_sync"}})
	}
	s.publish(domain.Event{Type: domain.EventBranchPushed, TenantID: fork.TenantID, TreeID: fork.ID, Actor: p.Username,
		Payload: map[string]any{"branch": name, "old_tip": ft, "new_tip": newTip, "via": "fork_sync"}})
	return sb, nil
}

// ---- tree deletion ----

// Start subscribes to tree.deleted: the fork records of a deleted tree
// (as upstream or as fork) and its snapshot origins are dropped. Forks of a
// deleted upstream keep Tree.ForkOf (upstream_missing).
func (s *Service) Start(ctx context.Context) {
	if s.d.Bus == nil {
		return
	}
	ch, cancel := s.d.Bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeDeleted}})
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-ch:
				if !ok {
					return
				}
				if e.TreeID == "" {
					continue
				}
				if err := s.DeleteTree(context.WithoutCancel(ctx), e.TreeID); err != nil {
					s.d.Logger.Warn("fork: tree.deleted cleanup failed", "tree", e.TreeID, "err", err)
				}
			}
		}
	}()
}

// DeleteTree drops the documents of a deleted tree.
func (s *Service) DeleteTree(ctx context.Context, treeID string) error {
	if _, err := s.d.Store.DeleteDocs(ctx, Coll, meta.DocPrefix(treeID)); err != nil {
		return err
	}
	recs, err := meta.ListDocsT[Record](ctx, s.d.Store, Coll, "")
	if err != nil {
		return err
	}
	for _, r := range recs {
		if r.TreeID == treeID {
			if err := s.d.Store.DeleteDoc(ctx, Coll, domain.DocKey(r.UpstreamTreeID, r.TreeID)); err != nil && !errors.Is(err, domain.ErrNotFound) {
				return err
			}
		}
	}
	_, err = s.d.Store.DeleteDocs(ctx, snapcopy.Coll, meta.DocPrefix(treeID))
	return err
}

func nonNil[T any](s []T) []T {
	if s == nil {
		return []T{}
	}
	return s
}

// newID returns a random RFC 4122 version 4 UUID string.
func newID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	b[6] = b[6]&0x0f | 0x40
	b[8] = b[8]&0x3f | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}
