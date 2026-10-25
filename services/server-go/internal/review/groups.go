package review

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"hash/fnv"
	"slices"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// GroupCollection holds linked groups, keyed by DocKey(tenantID, id).
const GroupCollection = "linked_groups"

// MaxGroupName bounds linked group names; MaxGroupMembers bounds their size.
const (
	MaxGroupName    = 200
	MaxGroupMembers = 50
)

// GroupMember names one merge request of a new linked group.
type GroupMember struct {
	Tree   *domain.Tree
	Number int
}

func newGroupID() string {
	var b [12]byte
	_, _ = rand.Read(b[:])
	return hex.EncodeToString(b[:])
}

// lockTrees acquires the per-tree locks of every tree (each stripe once,
// in stripe order, so it never deadlocks with single-tree lockers) and
// returns the unlock function.
func (s *Service) lockTrees(treeIDs []string) func() {
	idx := make([]int, 0, len(treeIDs))
	for _, id := range treeIDs {
		h := fnv.New32a()
		_, _ = h.Write([]byte(id))
		idx = append(idx, int(h.Sum32()%lockStripes))
	}
	slices.Sort(idx)
	idx = slices.Compact(idx)
	for _, i := range idx {
		s.locks[i].Lock()
	}
	return func() {
		for j := len(idx) - 1; j >= 0; j-- {
			s.locks[idx[j]].Unlock()
		}
	}
}

// CreateGroup links open merge requests (at least two, possibly in several
// trees and tenants) into a group of tenantID that merges atomically. The
// caller needs domain.ActMRMerge on every member tree; a merge request can
// belong to one open group only (domain.ErrConflict).
func (s *Service) CreateGroup(ctx context.Context, p *domain.Principal, tenantID, name string, members []GroupMember) (*domain.LinkedGroup, error) {
	name = strings.TrimSpace(name)
	switch {
	case tenantID == "":
		return nil, fmt.Errorf("tenant is required: %w", domain.ErrInvalid)
	case name == "" || len(name) > MaxGroupName || !utf8.ValidString(name):
		return nil, fmt.Errorf("group name must be 1-%d bytes: %w", MaxGroupName, domain.ErrInvalid)
	case len(members) < 2:
		return nil, fmt.Errorf("a linked group needs at least two merge requests: %w", domain.ErrInvalid)
	case len(members) > MaxGroupMembers:
		return nil, fmt.Errorf("a linked group holds at most %d merge requests: %w", MaxGroupMembers, domain.ErrInvalid)
	}
	seen := map[string]bool{}
	treeIDs := make([]string, 0, len(members))
	for _, m := range members {
		if err := checkTree(m.Tree); err != nil {
			return nil, err
		}
		if m.Number < 1 {
			return nil, fmt.Errorf("merge request numbers must be positive: %w", domain.ErrInvalid)
		}
		k := m.Tree.ID + "!" + strconv.Itoa(m.Number)
		if seen[k] {
			return nil, fmt.Errorf("merge request %s!%d is listed twice: %w", m.Tree.Name, m.Number, domain.ErrInvalid)
		}
		seen[k] = true
		if err := s.authz.Authorize(ctx, p, domain.ActMRMerge, m.Tree.TenantID, m.Tree); err != nil {
			return nil, err
		}
		treeIDs = append(treeIDs, m.Tree.ID)
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}

	s.groupMu.Lock()
	defer s.groupMu.Unlock()
	unlock := s.lockTrees(treeIDs)
	defer unlock()

	mrs := make([]*domain.MergeRequest, len(members))
	for i, m := range members {
		mr, err := s.openMR(ctx, m.Tree.ID, m.Number)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", m.Tree.Name, err)
		}
		if mr.LinkedGroup != "" {
			g, err := s.groupOfMR(ctx, m.Tree, mr)
			if err != nil {
				return nil, err
			}
			if g != nil && g.State == domain.MRStateOpen {
				return nil, fmt.Errorf("merge request %s!%d already belongs to linked group %q: %w",
					m.Tree.Name, m.Number, g.Name, domain.ErrConflict)
			}
		}
		mrs[i] = mr
	}

	g := &domain.LinkedGroup{
		ID:        newGroupID(),
		TenantID:  tenantID,
		Name:      name,
		State:     domain.MRStateOpen,
		CreatedBy: p.Username,
		CreatedAt: s.now(),
	}
	for _, m := range members {
		g.Members = append(g.Members, domain.LinkedGroupMember{TreeID: m.Tree.ID, MRNumber: m.Number})
	}
	if err := s.store.CreateDoc(ctx, GroupCollection, domain.DocKey(tenantID, g.ID), g); err != nil {
		return nil, fmt.Errorf("review: store linked group: %w", err)
	}
	for i, mr := range mrs {
		mr.LinkedGroup = g.ID
		mr.UpdatedAt = g.CreatedAt
		if err := s.store.UpdateMergeRequest(ctx, mr); err != nil {
			return nil, fmt.Errorf("review: link merge request %s!%d: %w", members[i].Tree.Name, mr.Number, err)
		}
	}
	s.recordGroup(ctx, "group.create", p, g, nil)
	return g, nil
}

// GetGroup returns linked group id of tenantID (no authorization: callers
// check tenant:read).
func (s *Service) GetGroup(ctx context.Context, tenantID, id string) (*domain.LinkedGroup, error) {
	if id == "" || strings.ContainsRune(id, 0) {
		return nil, fmt.Errorf("linked group id is required: %w", domain.ErrInvalid)
	}
	g, err := meta.GetDocT[domain.LinkedGroup](ctx, s.store, GroupCollection, domain.DocKey(tenantID, id))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("linked group %s not found: %w", id, domain.ErrNotFound)
		}
		return nil, fmt.Errorf("review: get linked group: %w", err)
	}
	return g, nil
}

// ListGroups returns the linked groups of tenantID, newest first.
func (s *Service) ListGroups(ctx context.Context, tenantID string) ([]domain.LinkedGroup, error) {
	gs, err := meta.ListDocsT[domain.LinkedGroup](ctx, s.store, GroupCollection, meta.DocPrefix(tenantID))
	if err != nil {
		return nil, fmt.Errorf("review: list linked groups: %w", err)
	}
	slices.SortStableFunc(gs, func(a, b domain.LinkedGroup) int { return b.CreatedAt.Compare(a.CreatedAt) })
	return gs, nil
}

// groupOfMR finds the linked group mr belongs to (nil when it no longer
// exists). Groups usually live in the tree's tenant; others are searched.
func (s *Service) groupOfMR(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest) (*domain.LinkedGroup, error) {
	if mr.LinkedGroup == "" {
		return nil, nil
	}
	g, err := s.GetGroup(ctx, tree.TenantID, mr.LinkedGroup)
	if err == nil {
		return g, nil
	}
	if !errors.Is(err, domain.ErrNotFound) {
		return nil, err
	}
	all, err := meta.ListDocsT[domain.LinkedGroup](ctx, s.store, GroupCollection, "")
	if err != nil {
		return nil, fmt.Errorf("review: list linked groups: %w", err)
	}
	for i := range all {
		if all[i].ID == mr.LinkedGroup {
			return &all[i], nil
		}
	}
	return nil, nil
}

// groupTrees loads the member trees of g and authorizes act on each.
func (s *Service) groupTrees(ctx context.Context, p *domain.Principal, g *domain.LinkedGroup, act domain.Action) ([]*domain.Tree, error) {
	trees := make([]*domain.Tree, len(g.Members))
	for i, m := range g.Members {
		t, err := s.store.GetTree(ctx, m.TreeID)
		if err != nil {
			if isNotFound(err) {
				return nil, fmt.Errorf("a member tree of linked group %q no longer exists: %w", g.Name, domain.ErrConflict)
			}
			return nil, fmt.Errorf("review: get tree: %w", err)
		}
		if act != "" {
			if err := s.authz.Authorize(ctx, p, act, t.TenantID, t); err != nil {
				return nil, err
			}
		}
		trees[i] = t
	}
	return trees, nil
}

// MergeGroup merges every member of an open linked group with the default
// merge method of each member; see MergeGroupWith.
func (s *Service) MergeGroup(ctx context.Context, p *domain.Principal, tenantID, id string) (*domain.LinkedGroup, error) {
	return s.MergeGroupWith(ctx, p, tenantID, id, "")
}

// MergeGroupWith merges every member of an open linked group atomically:
// all must be mergeable with method ("" = each member's default method, see
// MergeWith), otherwise *BlockedError (code merge_blocked), or
// *ConflictError (code merge_conflict, conflicts carry their tree) when
// three-way conflicts are the only problem. Merge and squash snapshots are
// written first; then the target tips are moved with compare-and-swap in
// member order; if one move fails the earlier ones are rolled back and the
// merge fails with domain.ErrConflict. Requires domain.ActMRMerge on every
// member tree.
func (s *Service) MergeGroupWith(ctx context.Context, p *domain.Principal, tenantID, id, method string) (*domain.LinkedGroup, error) {
	if err := requirePrincipalOrAuth(p); err != nil {
		return nil, err
	}
	opts := MergeOptions{Method: method}
	if err := validateMergeOptions(&opts); err != nil {
		return nil, err
	}
	s.groupMu.Lock()
	defer s.groupMu.Unlock()

	g, err := s.GetGroup(ctx, tenantID, id)
	if err != nil {
		return nil, err
	}
	trees, err := s.groupTrees(ctx, p, g, domain.ActMRMerge)
	if err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	if g.State != domain.MRStateOpen {
		return nil, fmt.Errorf("linked group %q is %s: %w", g.Name, g.State, domain.ErrConflict)
	}
	ids := make([]string, len(trees))
	for i, t := range trees {
		ids[i] = t.ID
	}
	unlock := s.lockTrees(ids)
	defer unlock()

	// 1. Every member must be mergeable with the requested method.
	mrs := make([]*domain.MergeRequest, len(trees))
	evs := make([]*evaluation, len(trees))
	methods := make([]string, len(trees))
	var problems []string
	var blockers []Blocker
	var conflicts []Conflict
	onlyConflicts := true
	for i, m := range g.Members {
		tree := trees[i]
		mr, err := s.getMR(ctx, tree.ID, m.MRNumber)
		if err != nil {
			return nil, fmt.Errorf("%s: %w", tree.Name, err)
		}
		if mr.LinkedGroup != g.ID {
			problems = append(problems, fmt.Sprintf("%s!%d is no longer linked to the group", tree.Name, mr.Number))
			onlyConflicts = false
			continue
		}
		ev, err := s.evaluate(ctx, tree, mr, true)
		if err != nil {
			return nil, err
		}
		merr := ev.mergeError()
		if merr == nil {
			methods[i], merr = ev.selectMethod(opts.Method)
		}
		if merr != nil {
			problems = append(problems, fmt.Sprintf("%s!%d: %v", tree.Name, mr.Number, merr))
			blockers = append(blockers, ev.checks.Blockers...)
			var ce *ConflictError
			if errors.As(merr, &ce) {
				conflicts = append(conflicts, inTree(ce.Conflicts, s.treeLabel(ctx, tree))...)
			} else {
				onlyConflicts = false
			}
			continue
		}
		mrs[i], evs[i] = mr, ev
	}
	if len(problems) > 0 {
		msg := fmt.Sprintf("linked group %q cannot be merged: %s", g.Name, strings.Join(problems, "; "))
		if onlyConflicts {
			return nil, &ConflictError{Message: msg, Conflicts: conflicts}
		}
		return nil, &BlockedError{Code: CodeMergeBlocked, Message: msg, Blockers: blockers}
	}

	// 2. Write the merge/squash snapshots (no branch moves yet).
	plans := make([]*mergePlan, len(trees))
	for i, tree := range trees {
		plan, err := s.prepareMerge(ctx, p, tree, mrs[i], evs[i], methods[i], "")
		if err != nil {
			return nil, fmt.Errorf("linked group %q: %s!%d: %w (no branch was changed)", g.Name, tree.Name, mrs[i].Number, err)
		}
		plans[i] = plan
	}

	// 3. Move the tips; roll back on failure.
	moved := make([]bool, len(trees))
	for i, tree := range trees {
		ok, err := s.moveTarget(ctx, tree, mrs[i], evs[i], plans[i])
		if err != nil {
			s.rollback(ctx, trees, mrs, evs, plans, moved)
			return nil, fmt.Errorf("linked group %q: %s!%d: %w (no branch was changed)", g.Name, tree.Name, mrs[i].Number, err)
		}
		moved[i] = ok
	}

	// 4. Record the result.
	for i, mr := range mrs {
		if err := s.markMerged(ctx, mr, plans[i]); err != nil {
			return nil, err
		}
	}
	now := s.now()
	g.State = domain.MRStateMerged
	g.MergedAt = &now
	if err := s.store.PutDoc(ctx, GroupCollection, domain.DocKey(g.TenantID, g.ID), g); err != nil {
		return nil, fmt.Errorf("review: branches updated but linked group %q could not be saved: %w", g.Name, err)
	}
	for i, mr := range mrs {
		s.announceMerge(ctx, p, trees[i], mr, evs[i], plans[i], moved[i], map[string]string{"linked_group": g.ID})
	}
	if s.bus != nil {
		s.bus.Publish(domain.Event{
			Type: domain.EventGroupMerged, TenantID: g.TenantID, Actor: username(p), Payload: cloneGroup(g), At: now,
		})
	}
	s.recordGroup(ctx, "group.merge", p, g, nil)
	return g, nil
}

// rollback restores the target tips moved so far (best effort: a tip that
// moved again meanwhile is left alone).
func (s *Service) rollback(ctx context.Context, trees []*domain.Tree, mrs []*domain.MergeRequest, evs []*evaluation, plans []*mergePlan, moved []bool) {
	for j := len(trees) - 1; j >= 0; j-- {
		if !moved[j] {
			continue
		}
		ev := evs[j]
		if err := s.store.CompareAndSwapBranchTip(context.WithoutCancel(ctx), trees[j].ID, mrs[j].TargetBranch, plans[j].newTip, ev.targetTip); err != nil {
			s.logger().Error("review: linked group rollback failed",
				"tree", trees[j].ID, "branch", mrs[j].TargetBranch, "error", err)
		}
	}
}

// CloseGroup closes an open linked group and unlinks its open members.
// Its creator may close it; anyone else needs domain.ActMRMerge on every
// member tree.
func (s *Service) CloseGroup(ctx context.Context, p *domain.Principal, tenantID, id string) (*domain.LinkedGroup, error) {
	if err := requirePrincipalOrAuth(p); err != nil {
		return nil, err
	}
	s.groupMu.Lock()
	defer s.groupMu.Unlock()
	g, err := s.GetGroup(ctx, tenantID, id)
	if err != nil {
		return nil, err
	}
	act := domain.ActMRMerge
	if !p.Anonymous() && p.Username == g.CreatedBy {
		act = ""
	}
	trees, err := s.groupTrees(ctx, p, g, act)
	if err != nil && !errors.Is(err, domain.ErrConflict) {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	if g.State != domain.MRStateOpen {
		return nil, fmt.Errorf("linked group %q is %s: %w", g.Name, g.State, domain.ErrConflict)
	}
	for i, m := range g.Members {
		if i >= len(trees) || trees[i] == nil {
			continue
		}
		unlock := s.lockTree(trees[i].ID)
		mr, err := s.getMR(ctx, trees[i].ID, m.MRNumber)
		if err == nil && mr.LinkedGroup == g.ID {
			mr.LinkedGroup = ""
			mr.UpdatedAt = s.now()
			err = s.store.UpdateMergeRequest(ctx, mr)
		}
		unlock()
		if err != nil && !isNotFound(err) {
			return nil, fmt.Errorf("review: unlink merge request: %w", err)
		}
	}
	g.State = domain.MRStateClosed
	if err := s.store.PutDoc(ctx, GroupCollection, domain.DocKey(g.TenantID, g.ID), g); err != nil {
		return nil, fmt.Errorf("review: store linked group: %w", err)
	}
	s.recordGroup(ctx, "group.close", p, g, nil)
	return g, nil
}

// treeLabel names a tree "tenant/tree" (the tree name alone when the tenant
// cannot be read).
func (s *Service) treeLabel(ctx context.Context, tree *domain.Tree) string {
	if t, err := s.store.GetTenant(ctx, tree.TenantID); err == nil && t.Slug != "" {
		return t.Slug + "/" + tree.Name
	}
	return tree.Name
}

// inTree returns cs with Tree set to label.
func inTree(cs []Conflict, label string) []Conflict {
	out := make([]Conflict, len(cs))
	for i, c := range cs {
		c.Tree = label
		out[i] = c
	}
	return out
}

// requirePrincipalOrAuth rejects anonymous callers before any lookup.
func requirePrincipalOrAuth(p *domain.Principal) error {
	if p.Anonymous() {
		return fmt.Errorf("review: sign-in required: %w", domain.ErrUnauthenticated)
	}
	return nil
}

func cloneGroup(g *domain.LinkedGroup) domain.LinkedGroup {
	c := *g
	c.Members = slices.Clone(g.Members)
	if g.MergedAt != nil {
		t := *g.MergedAt
		c.MergedAt = &t
	}
	return c
}

func (s *Service) recordGroup(ctx context.Context, action string, p *domain.Principal, g *domain.LinkedGroup, extra map[string]string) {
	if s.audit == nil {
		return
	}
	members := make([]string, len(g.Members))
	for i, m := range g.Members {
		members[i] = m.TreeID + "!" + strconv.Itoa(m.MRNumber)
	}
	details := map[string]string{"name": g.Name, "members": strings.Join(members, ",")}
	for k, v := range extra {
		details[k] = v
	}
	ev := domain.AuditEvent{
		TenantID:     g.TenantID,
		Actor:        "anonymous",
		Action:       action,
		ResourceType: "linked_group",
		ResourceID:   g.ID,
		Details:      details,
		At:           s.now(),
	}
	if !p.Anonymous() {
		ev.ActorID, ev.Actor = p.AccountID, p.Username
	}
	s.audit.Record(ctx, ev)
}
