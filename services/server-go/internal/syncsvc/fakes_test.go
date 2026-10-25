package syncsvc

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strings"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// This file holds small in-package fakes of the domain interfaces so the
// sync service can be tested without the real meta/iam/review/... packages.
// Each fake embeds its interface: calling a method that is not overridden
// panics, which flags accidental dependencies in tests.

// ---------------------------------------------------------------- MetaStore

type fakeStore struct {
	domain.MetaStore

	mu          sync.Mutex
	tenants     map[string]*domain.Tenant
	trees       map[string]*domain.Tree
	branches    map[string]*domain.Branch // treeID + "\x00" + name
	snapshots   map[string]*domain.Snapshot
	tags        map[string]*domain.Tag // treeID + "\x00" + name
	configs     map[string][]byte      // tenantID + "\x00" + kind
	protections []domain.BranchProtection
}

func newFakeStore() *fakeStore {
	return &fakeStore{
		tenants:   map[string]*domain.Tenant{},
		trees:     map[string]*domain.Tree{},
		branches:  map[string]*domain.Branch{},
		snapshots: map[string]*domain.Snapshot{},
		tags:      map[string]*domain.Tag{},
		configs:   map[string][]byte{},
	}
}

func key(a, b string) string { return a + "\x00" + b }

func (f *fakeStore) GetTenant(_ context.Context, id string) (*domain.Tenant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	t, ok := f.tenants[id]
	if !ok {
		return nil, fmt.Errorf("tenant %s: %w", id, domain.ErrNotFound)
	}
	c := *t
	return &c, nil
}

func (f *fakeStore) GetTenantBySlug(_ context.Context, slug string) (*domain.Tenant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, t := range f.tenants {
		if t.Slug == slug {
			c := *t
			return &c, nil
		}
	}
	return nil, fmt.Errorf("tenant %s: %w", slug, domain.ErrNotFound)
}

func (f *fakeStore) CreateTree(_ context.Context, t *domain.Tree) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, x := range f.trees {
		if x.TenantID == t.TenantID && x.Name == t.Name {
			return fmt.Errorf("tree %s: %w", t.Name, domain.ErrAlreadyExists)
		}
	}
	c := *t
	f.trees[t.ID] = &c
	return nil
}

func (f *fakeStore) GetTree(_ context.Context, id string) (*domain.Tree, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	t, ok := f.trees[id]
	if !ok {
		return nil, fmt.Errorf("tree %s: %w", id, domain.ErrNotFound)
	}
	c := *t
	return &c, nil
}

func (f *fakeStore) GetTreeByName(_ context.Context, tenantID, name string) (*domain.Tree, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, t := range f.trees {
		if t.TenantID == tenantID && t.Name == name {
			c := *t
			return &c, nil
		}
	}
	return nil, fmt.Errorf("tree %s: %w", name, domain.ErrNotFound)
}

func (f *fakeStore) setVisibility(id, v string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.trees[id].Visibility = v
}

func (f *fakeStore) GetBranch(_ context.Context, treeID, name string) (*domain.Branch, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	b, ok := f.branches[key(treeID, name)]
	if !ok {
		return nil, fmt.Errorf("branch %s: %w", name, domain.ErrNotFound)
	}
	c := *b
	return &c, nil
}

func (f *fakeStore) ListBranches(_ context.Context, treeID string) ([]domain.Branch, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.Branch{}
	for _, b := range f.branches {
		if b.TreeID == treeID {
			out = append(out, *b)
		}
	}
	return out, nil
}

func (f *fakeStore) CompareAndSwapBranchTip(_ context.Context, treeID, name, oldTip, newTip string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	k := key(treeID, name)
	b, ok := f.branches[k]
	cur := ""
	if ok {
		cur = b.Tip
	}
	if cur != oldTip {
		return fmt.Errorf("branch %s: %w", name, domain.ErrConflict)
	}
	if !ok {
		b = &domain.Branch{TreeID: treeID, Name: name}
		f.branches[k] = b
	}
	b.Tip = newTip
	return nil
}

func (f *fakeStore) PutSnapshot(_ context.Context, s *domain.Snapshot) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if old, ok := f.snapshots[s.ID]; ok {
		if old.PushedAt.Equal(s.PushedAt) && sameSnapshotContent(old, s) {
			return nil
		}
		return fmt.Errorf("snapshot %s: %w", s.ID, domain.ErrConflict)
	}
	c := *s
	c.Parents = append([]string(nil), s.Parents...)
	f.snapshots[s.ID] = &c
	return nil
}

func (f *fakeStore) GetSnapshot(_ context.Context, id string) (*domain.Snapshot, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	s, ok := f.snapshots[id]
	if !ok {
		return nil, fmt.Errorf("snapshot %s: %w", id, domain.ErrNotFound)
	}
	c := *s
	return &c, nil
}

func (f *fakeStore) PutTag(_ context.Context, t *domain.Tag) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	k := key(t.TreeID, t.Name)
	if _, ok := f.tags[k]; ok {
		return fmt.Errorf("tag %s: %w", t.Name, domain.ErrAlreadyExists)
	}
	c := *t
	f.tags[k] = &c
	return nil
}

func (f *fakeStore) ListTags(_ context.Context, treeID string) ([]domain.Tag, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.Tag{}
	for _, t := range f.tags {
		if t.TreeID == treeID {
			out = append(out, *t)
		}
	}
	return out, nil
}

func (f *fakeStore) PutAccessConfig(_ context.Context, tenantID, kind string, content []byte) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.configs[key(tenantID, kind)] = append([]byte(nil), content...)
	return nil
}

func (f *fakeStore) ListProtections(_ context.Context, treeID string) ([]domain.BranchProtection, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.BranchProtection{}
	for _, p := range f.protections {
		if p.TreeID == treeID {
			out = append(out, p)
		}
	}
	return out, nil
}

// ------------------------------------------------------------------ History

// fakeHistory walks the fakeStore's snapshot graph.
type fakeHistory struct{ st *fakeStore }

func (h fakeHistory) parents(id string) ([]string, bool) {
	h.st.mu.Lock()
	defer h.st.mu.Unlock()
	s, ok := h.st.snapshots[id]
	if !ok {
		return nil, false
	}
	return append([]string(nil), s.Parents...), true
}

// reach returns the ids reachable from tips, in DFS post-order (parents
// before children).
func (h fakeHistory) reach(tips []string) ([]string, error) {
	var order []string
	seen := map[string]bool{}
	var visit func(id string) error
	visit = func(id string) error {
		if seen[id] {
			return nil
		}
		seen[id] = true
		ps, ok := h.parents(id)
		if !ok {
			return fmt.Errorf("snapshot %s: %w", id, domain.ErrNotFound)
		}
		for _, p := range ps {
			if err := visit(p); err != nil {
				return err
			}
		}
		order = append(order, id)
		return nil
	}
	for _, t := range tips {
		if err := visit(t); err != nil {
			return nil, err
		}
	}
	return order, nil
}

func (h fakeHistory) IsAncestor(_ context.Context, ancestor, descendant string) (bool, error) {
	ids, err := h.reach([]string{descendant})
	if err != nil {
		return false, err
	}
	for _, id := range ids {
		if id == ancestor {
			return true, nil
		}
	}
	return false, nil
}

func (h fakeHistory) Log(ctx context.Context, tip string, limit int) ([]domain.Snapshot, error) {
	ids, err := h.reach([]string{tip})
	if err != nil {
		return nil, err
	}
	out := make([]domain.Snapshot, 0, len(ids))
	for _, id := range ids {
		s, _ := h.st.GetSnapshot(ctx, id)
		out = append(out, *s)
	}
	sort.Slice(out, func(i, j int) bool {
		if !out[i].CreatedAt.Equal(out[j].CreatedAt) {
			return out[i].CreatedAt.After(out[j].CreatedAt)
		}
		return out[i].ID > out[j].ID
	})
	if limit > 0 && len(out) > limit {
		out = out[:limit]
	}
	return out, nil
}

func (h fakeHistory) Missing(ctx context.Context, tip string, haves []string) ([]domain.Snapshot, error) {
	have, err := h.reach(haves)
	if err != nil {
		return nil, err
	}
	excluded := map[string]bool{}
	for _, id := range have {
		excluded[id] = true
	}
	ids, err := h.reach([]string{tip})
	if err != nil {
		return nil, err
	}
	out := []domain.Snapshot{}
	for _, id := range ids {
		if !excluded[id] {
			s, _ := h.st.GetSnapshot(ctx, id)
			out = append(out, *s)
		}
	}
	return out, nil
}

// --------------------------------------------------------------- Authorizer

// fakeAuthz implements a simplified role model: members[tenantID][accountID]
// = role. Server admins may do anything.
type fakeAuthz struct {
	mu      sync.Mutex
	members map[string]map[string]string
}

func (a *fakeAuthz) setRole(tenantID, accountID, role string) {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.members == nil {
		a.members = map[string]map[string]string{}
	}
	if a.members[tenantID] == nil {
		a.members[tenantID] = map[string]string{}
	}
	a.members[tenantID][accountID] = role
}

func (a *fakeAuthz) RoleOf(_ context.Context, accountID, tenantID string) (string, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.members[tenantID][accountID], nil
}

func (a *fakeAuthz) Authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	if act == domain.ActTreeRead && tree != nil && tree.Visibility == domain.VisibilityPublic {
		return nil
	}
	if p == nil {
		return domain.ErrUnauthenticated
	}
	if p.IsAdmin {
		return nil
	}
	role, _ := a.RoleOf(ctx, p.AccountID, tenantID)
	var ok bool
	switch act {
	case domain.ActTreeRead:
		ok = role != "" || (tree != nil && tree.Visibility == domain.VisibilityInternal)
	case domain.ActTreeWrite, domain.ActTreeCreate:
		ok = role == domain.RoleOwner || role == domain.RoleAdmin || role == domain.RoleMaintainer || role == domain.RoleDeveloper
	case domain.ActTenantAdmin:
		ok = role == domain.RoleOwner || role == domain.RoleAdmin
	}
	if !ok {
		return fmt.Errorf("%s: %w", act, domain.ErrForbidden)
	}
	return nil
}

// ------------------------------------------------------- Review / License

type fakeReview struct {
	domain.ReviewService
	mu sync.Mutex
	fn func(branch string) error
}

func (r *fakeReview) set(fn func(branch string) error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.fn = fn
}

func (r *fakeReview) CheckPush(_ context.Context, _ *domain.Principal, _ *domain.Tree, branch string) error {
	r.mu.Lock()
	fn := r.fn
	r.mu.Unlock()
	if fn == nil {
		return nil
	}
	return fn(branch)
}

type fakeLicense struct {
	domain.LicenseChecker
	mu    sync.Mutex
	fn    func(paths []string) error
	calls [][]string
}

func (l *fakeLicense) set(fn func(paths []string) error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	l.fn = fn
}

func (l *fakeLicense) lastCall() []string {
	l.mu.Lock()
	defer l.mu.Unlock()
	if len(l.calls) == 0 {
		return nil
	}
	return l.calls[len(l.calls)-1]
}

func (l *fakeLicense) CheckPush(_ context.Context, _ *domain.Principal, _ *domain.Tree, paths []string) error {
	l.mu.Lock()
	l.calls = append(l.calls, append([]string(nil), paths...))
	fn := l.fn
	l.mu.Unlock()
	if fn == nil {
		return nil
	}
	return fn(paths)
}

// CheckRead allows every read. (v0.5 sec-paths, §13.1: Pull consults the
// license checker for stubbed paths; pull_paths_test.go covers denials with
// the real checker.)
func (l *fakeLicense) CheckRead(context.Context, *domain.Principal, *domain.Tree, []string) error {
	return nil
}

// ------------------------------------------------------------------ Staged

type fakeStaged struct {
	mu      sync.Mutex
	entries map[string]domain.StagedSnapshot // tree + account + branch
	cleared []string                         // "account/branch"
}

func (f *fakeStaged) Stage(_ context.Context, p *domain.Principal, tree *domain.Tree, s *domain.StagedSnapshot) error {
	if strings.HasPrefix(s.Message, "invalid") {
		return fmt.Errorf("%w: message rejected", domain.ErrInvalid)
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.entries == nil {
		f.entries = map[string]domain.StagedSnapshot{}
	}
	f.entries[tree.ID+"/"+p.AccountID+"/"+s.BranchName] = *s
	return nil
}

func (f *fakeStaged) List(_ context.Context, _ *domain.Principal, tree *domain.Tree, branch string) ([]domain.StagedSnapshot, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.StagedSnapshot{}
	for _, s := range f.entries {
		if s.TreeID == tree.ID && (branch == "" || s.BranchName == branch) {
			out = append(out, s)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].AccountID < out[j].AccountID })
	return out, nil
}

func (f *fakeStaged) ClearOnPush(_ context.Context, p *domain.Principal, tree *domain.Tree, branch string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	k := tree.ID + "/" + p.AccountID + "/" + branch
	if _, ok := f.entries[k]; !ok {
		return fmt.Errorf("staged: %w", domain.ErrNotFound)
	}
	delete(f.entries, k)
	f.cleared = append(f.cleared, p.AccountID+"/"+branch)
	return nil
}

// ------------------------------------------------------------- Bus / Audit

type fakeBus struct {
	mu     sync.Mutex
	events []domain.Event
}

func (b *fakeBus) Publish(e domain.Event) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.events = append(b.events, e)
}

func (b *fakeBus) Subscribe(domain.EventFilter) (<-chan domain.Event, func()) {
	ch := make(chan domain.Event)
	return ch, func() {}
}

func (b *fakeBus) ofType(typ string) []domain.Event {
	b.mu.Lock()
	defer b.mu.Unlock()
	var out []domain.Event
	for _, e := range b.events {
		if e.Type == typ {
			out = append(out, e)
		}
	}
	return out
}

type fakeAudit struct {
	mu     sync.Mutex
	events []domain.AuditEvent
}

func (a *fakeAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, e)
}

func (a *fakeAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, errors.New("not implemented")
}

func (a *fakeAudit) actions(action string) []domain.AuditEvent {
	a.mu.Lock()
	defer a.mu.Unlock()
	var out []domain.AuditEvent
	for _, e := range a.events {
		if e.Action == action {
			out = append(out, e)
		}
	}
	return out
}
