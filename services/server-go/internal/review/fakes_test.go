package review

import (
	"context"
	"fmt"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// fakeStore implements the subset of domain.MetaStore used by the review
// service. Unused methods panic via the nil embedded interface.
type fakeStore struct {
	domain.MetaStore

	mu          sync.Mutex
	branches    map[string]domain.Branch // key: tree + "/" + name
	protections map[string][]domain.BranchProtection
	mrs         map[string][]domain.MergeRequest // index = number-1
	nextID      int

	beforeCAS func()   // called (without the lock) before every CAS
	updateErr error    // returned by UpdateMergeRequest when set
	casCalls  int      // number of CompareAndSwapBranchTip calls
	updates   []string // "tree!number:state" for each successful update
}

func newFakeStore() *fakeStore {
	return &fakeStore{
		branches:    map[string]domain.Branch{},
		protections: map[string][]domain.BranchProtection{},
		mrs:         map[string][]domain.MergeRequest{},
	}
}

func bkey(tree, name string) string { return tree + "/" + name }

func cloneMR(mr domain.MergeRequest) *domain.MergeRequest {
	c := mr
	c.Approvals = slices.Clone(mr.Approvals)
	if mr.MergedAt != nil {
		t := *mr.MergedAt
		c.MergedAt = &t
	}
	return &c
}

func (f *fakeStore) setBranch(tree, name, tip string) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.branches[bkey(tree, name)] = domain.Branch{TreeID: tree, Name: name, Tip: tip}
}

func (f *fakeStore) tip(tree, name string) (string, bool) {
	f.mu.Lock()
	defer f.mu.Unlock()
	b, ok := f.branches[bkey(tree, name)]
	return b.Tip, ok
}

func (f *fakeStore) GetBranch(_ context.Context, tree, name string) (*domain.Branch, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	b, ok := f.branches[bkey(tree, name)]
	if !ok {
		return nil, fmt.Errorf("branch %s: %w", name, domain.ErrNotFound)
	}
	return &b, nil
}

func (f *fakeStore) CompareAndSwapBranchTip(_ context.Context, tree, name, oldTip, newTip string) error {
	if f.beforeCAS != nil {
		f.beforeCAS()
	}
	f.mu.Lock()
	defer f.mu.Unlock()
	f.casCalls++
	b, ok := f.branches[bkey(tree, name)]
	if (ok && b.Tip != oldTip) || (!ok && oldTip != "") {
		return fmt.Errorf("branch %s tip is %q, not %q: %w", name, b.Tip, oldTip, domain.ErrConflict)
	}
	b.TreeID, b.Name, b.Tip = tree, name, newTip
	f.branches[bkey(tree, name)] = b
	return nil
}

func (f *fakeStore) PutProtection(_ context.Context, p *domain.BranchProtection) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.protections[p.TreeID] = append(f.protections[p.TreeID], *p)
	return nil
}

func (f *fakeStore) ListProtections(_ context.Context, tree string) ([]domain.BranchProtection, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	return append([]domain.BranchProtection{}, f.protections[tree]...), nil
}

func (f *fakeStore) CreateMergeRequest(_ context.Context, mr *domain.MergeRequest) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.nextID++
	if mr.ID == "" {
		mr.ID = fmt.Sprintf("mr-%d", f.nextID)
	}
	mr.Number = len(f.mrs[mr.TreeID]) + 1
	f.mrs[mr.TreeID] = append(f.mrs[mr.TreeID], *cloneMR(*mr))
	return nil
}

func (f *fakeStore) GetMergeRequest(_ context.Context, tree string, number int) (*domain.MergeRequest, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	list := f.mrs[tree]
	if number < 1 || number > len(list) {
		return nil, fmt.Errorf("merge request %d: %w", number, domain.ErrNotFound)
	}
	return cloneMR(list[number-1]), nil
}

func (f *fakeStore) ListMergeRequests(_ context.Context, tree, state string) ([]domain.MergeRequest, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.MergeRequest{}
	for _, mr := range f.mrs[tree] {
		if state == "" || mr.State == state {
			out = append(out, *cloneMR(mr))
		}
	}
	return out, nil
}

func (f *fakeStore) UpdateMergeRequest(_ context.Context, mr *domain.MergeRequest) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.updateErr != nil {
		return f.updateErr
	}
	list := f.mrs[mr.TreeID]
	if mr.Number < 1 || mr.Number > len(list) {
		return fmt.Errorf("merge request %d: %w", mr.Number, domain.ErrNotFound)
	}
	list[mr.Number-1] = *cloneMR(*mr)
	f.updates = append(f.updates, fmt.Sprintf("%s!%d:%s", mr.TreeID, mr.Number, mr.State))
	return nil
}

// mr returns the stored merge request.
func (f *fakeStore) mr(t *testing.T, tree string, number int) *domain.MergeRequest {
	t.Helper()
	m, err := f.GetMergeRequest(context.Background(), tree, number)
	if err != nil {
		t.Fatalf("stored merge request: %v", err)
	}
	return m
}

// fakeHistory resolves ancestry from an explicit parent map.
type fakeHistory struct {
	domain.History
	parents map[string][]string
}

func (h *fakeHistory) IsAncestor(_ context.Context, ancestor, descendant string) (bool, error) {
	if _, ok := h.parents[descendant]; !ok {
		return false, fmt.Errorf("snapshot %s: %w", descendant, domain.ErrNotFound)
	}
	seen := map[string]bool{}
	queue := []string{descendant}
	for len(queue) > 0 {
		id := queue[0]
		queue = queue[1:]
		if id == ancestor {
			return true, nil
		}
		if seen[id] {
			continue
		}
		seen[id] = true
		queue = append(queue, h.parents[id]...)
	}
	return false, nil
}

// fakeAuthz grants actions per username. Denials return a distinct error
// value per call so tests can check it is passed through unchanged.
type fakeAuthz struct {
	grants map[string][]domain.Action

	mu      sync.Mutex
	lastErr error
}

func (a *fakeAuthz) Authorize(_ context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	var err error
	switch {
	case p == nil:
		err = fmt.Errorf("authz: %s requires sign-in: %w", act, domain.ErrUnauthenticated)
	case tree == nil || tenantID != tree.TenantID:
		err = fmt.Errorf("authz: wrong tenant %q: %w", tenantID, domain.ErrForbidden)
	case !slices.Contains(a.grants[p.Username], act):
		err = fmt.Errorf("authz: %s may not %s: %w", p.Username, act, domain.ErrForbidden)
	}
	a.mu.Lock()
	a.lastErr = err
	a.mu.Unlock()
	return err
}

func (a *fakeAuthz) RoleOf(context.Context, string, string) (string, error) { return "", nil }

func (a *fakeAuthz) last() error {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.lastErr
}

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
	return make(chan domain.Event), func() {}
}

func (b *fakeBus) types() []string {
	b.mu.Lock()
	defer b.mu.Unlock()
	out := []string{}
	for _, e := range b.events {
		out = append(out, e.Type)
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
	return nil, nil
}

func (a *fakeAudit) actions() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	out := []string{}
	for _, e := range a.events {
		out = append(out, e.Action)
	}
	return out
}

// Snapshot graph used by the tests:
//
//	s1 ── s2 ── s3        (feature: s3)
//	  └── s4              (main may be moved here to diverge)
//	s1 ── s2 ── s5        (other: s5, sibling of s3)
const (
	s1 = "s1"
	s2 = "s2"
	s3 = "s3"
	s4 = "s4"
	s5 = "s5"
)

var (
	alice = &domain.Principal{AccountID: "acc-alice", Username: "alice", TenantID: "ten-alice"}
	bob   = &domain.Principal{AccountID: "acc-bob", Username: "bob", TenantID: "ten-bob"}
	dave  = &domain.Principal{AccountID: "acc-dave", Username: "dave", TenantID: "ten-dave"}
	carol = &domain.Principal{AccountID: "acc-carol", Username: "carol", TenantID: "ten-carol"}
	root  = &domain.Principal{AccountID: "acc-root", Username: "root", TenantID: "ten-root", IsAdmin: true}
	eve   = &domain.Principal{AccountID: "acc-eve", Username: "eve", TenantID: "ten-eve"}
)

var (
	developer  = []domain.Action{domain.ActTreeRead, domain.ActTreeWrite, domain.ActBranchCreate, domain.ActMRCreate, domain.ActMRApprove}
	maintainer = append(slices.Clone(developer), domain.ActBranchDelete, domain.ActMRMerge)
)

type env struct {
	svc   *Service
	store *fakeStore
	hist  *fakeHistory
	authz *fakeAuthz
	bus   *fakeBus
	audit *fakeAudit
	tree  *domain.Tree
}

var fixedNow = time.Date(2026, 10, 24, 12, 0, 0, 0, time.UTC)

func newEnv(t *testing.T) *env {
	t.Helper()
	e := &env{
		store: newFakeStore(),
		hist: &fakeHistory{parents: map[string][]string{
			s1: nil, s2: {s1}, s3: {s2}, s4: {s1}, s5: {s2},
		}},
		authz: &fakeAuthz{grants: map[string][]domain.Action{
			"alice": developer,
			"bob":   developer,
			"dave":  developer,
			"carol": maintainer,
			"root":  maintainer,
			"eve":   {domain.ActTreeRead},
		}},
		bus:   &fakeBus{},
		audit: &fakeAudit{},
		tree:  &domain.Tree{ID: "tree-1", TenantID: "ten-acme", Name: "proj", DefaultBranch: "main"},
	}
	e.svc = New(e.store, e.hist, e.authz, e.bus, e.audit)
	e.svc.now = func() time.Time { return fixedNow }
	e.store.setBranch(e.tree.ID, "main", s2)
	e.store.setBranch(e.tree.ID, "feature", s3)
	e.store.setBranch(e.tree.ID, "other", s5)
	e.store.setBranch(e.tree.ID, "empty", "")
	return e
}

func (e *env) protect(rules ...domain.BranchProtection) {
	for i := range rules {
		rules[i].TreeID = e.tree.ID
		_ = e.store.PutProtection(context.Background(), &rules[i])
	}
}

// open creates a feature→main merge request as alice.
func (e *env) open(t *testing.T) *domain.MergeRequest {
	t.Helper()
	mr, err := e.svc.Open(context.Background(), alice, e.tree, "Add feature", "desc", "feature", "main")
	if err != nil {
		t.Fatalf("Open: %v", err)
	}
	return mr
}
