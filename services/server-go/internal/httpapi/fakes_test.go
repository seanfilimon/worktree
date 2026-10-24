package httpapi

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// ---- MetaStore ----

// fakeStore is an in-memory MetaStore covering the methods httpapi uses.
// Unused methods fall through to the nil embedded interface and panic.
type fakeStore struct {
	domain.MetaStore

	mu          sync.Mutex
	tenants     map[string]domain.Tenant
	accounts    map[string]domain.Account
	memberships map[[2]string]domain.Membership // tenant, account
	tokens      map[string]domain.APIToken
	trees       map[string]domain.Tree
	branches    map[[2]string]domain.Branch // tree, name
	snapshots   map[string]domain.Snapshot
	tags        map[[2]string]domain.Tag
	staged      []domain.StagedSnapshot
	protections map[[2]string]domain.BranchProtection
	mrs         map[[2]string]domain.MergeRequest // tree, number
	licenses    map[[2]string]domain.LicenseRule
	failList    error // injected failure for ListTrees
}

func newFakeStore() *fakeStore {
	return &fakeStore{
		tenants:     map[string]domain.Tenant{},
		accounts:    map[string]domain.Account{},
		memberships: map[[2]string]domain.Membership{},
		tokens:      map[string]domain.APIToken{},
		trees:       map[string]domain.Tree{},
		branches:    map[[2]string]domain.Branch{},
		snapshots:   map[string]domain.Snapshot{},
		tags:        map[[2]string]domain.Tag{},
		protections: map[[2]string]domain.BranchProtection{},
		mrs:         map[[2]string]domain.MergeRequest{},
		licenses:    map[[2]string]domain.LicenseRule{},
	}
}

func nf(what string) error { return fmt.Errorf("%s: %w", what, domain.ErrNotFound) }

func (s *fakeStore) CreateTenant(_ context.Context, t *domain.Tenant) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, x := range s.tenants {
		if x.Slug == t.Slug {
			return fmt.Errorf("tenant %s: %w", t.Slug, domain.ErrAlreadyExists)
		}
	}
	s.tenants[t.ID] = *t
	return nil
}

func (s *fakeStore) GetTenant(_ context.Context, id string) (*domain.Tenant, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	t, ok := s.tenants[id]
	if !ok {
		return nil, nf("tenant")
	}
	return &t, nil
}

func (s *fakeStore) GetTenantBySlug(_ context.Context, slug string) (*domain.Tenant, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, t := range s.tenants {
		if t.Slug == slug {
			return &t, nil
		}
	}
	return nil, nf("tenant")
}

func (s *fakeStore) ListTenants(context.Context) ([]domain.Tenant, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.Tenant{}
	for _, t := range s.tenants {
		out = append(out, t)
	}
	return out, nil
}

func (s *fakeStore) CreateAccount(_ context.Context, a *domain.Account) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, x := range s.accounts {
		if x.Username == a.Username {
			return fmt.Errorf("account: %w", domain.ErrAlreadyExists)
		}
	}
	s.accounts[a.ID] = *a
	return nil
}

func (s *fakeStore) GetAccount(_ context.Context, id string) (*domain.Account, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	a, ok := s.accounts[id]
	if !ok {
		return nil, nf("account")
	}
	return &a, nil
}

func (s *fakeStore) GetAccountByUsername(_ context.Context, u string) (*domain.Account, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, a := range s.accounts {
		if a.Username == u {
			return &a, nil
		}
	}
	return nil, nf("account")
}

func (s *fakeStore) ListAccounts(context.Context) ([]domain.Account, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.Account{}
	for _, a := range s.accounts {
		out = append(out, a)
	}
	return out, nil
}

func (s *fakeStore) UpdateAccount(_ context.Context, a *domain.Account) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.accounts[a.ID]; !ok {
		return nf("account")
	}
	s.accounts[a.ID] = *a
	return nil
}

func (s *fakeStore) PutMembership(_ context.Context, m *domain.Membership) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.memberships[[2]string{m.TenantID, m.AccountID}] = *m
	return nil
}

func (s *fakeStore) GetMembership(_ context.Context, t, a string) (*domain.Membership, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	m, ok := s.memberships[[2]string{t, a}]
	if !ok {
		return nil, nf("membership")
	}
	return &m, nil
}

func (s *fakeStore) ListTenantMembers(_ context.Context, t string) ([]domain.Membership, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.Membership{}
	for k, m := range s.memberships {
		if k[0] == t {
			out = append(out, m)
		}
	}
	return out, nil
}

func (s *fakeStore) ListAccountMemberships(_ context.Context, a string) ([]domain.Membership, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.Membership{}
	for k, m := range s.memberships {
		if k[1] == a {
			out = append(out, m)
		}
	}
	return out, nil
}

func (s *fakeStore) DeleteMembership(_ context.Context, t, a string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := [2]string{t, a}
	if _, ok := s.memberships[k]; !ok {
		return nf("membership")
	}
	delete(s.memberships, k)
	return nil
}

func (s *fakeStore) CreateToken(_ context.Context, t *domain.APIToken) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.tokens[t.ID] = *t
	return nil
}

func (s *fakeStore) GetTokenByHash(_ context.Context, h string) (*domain.APIToken, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, t := range s.tokens {
		if t.TokenHash == h {
			return &t, nil
		}
	}
	return nil, nf("token")
}

func (s *fakeStore) ListTokens(_ context.Context, a string) ([]domain.APIToken, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.APIToken{}
	for _, t := range s.tokens {
		if t.AccountID == a {
			out = append(out, t)
		}
	}
	return out, nil
}

func (s *fakeStore) DeleteToken(_ context.Context, id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.tokens[id]; !ok {
		return nf("token")
	}
	delete(s.tokens, id)
	return nil
}

func (s *fakeStore) CreateTree(_ context.Context, t *domain.Tree) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, x := range s.trees {
		if x.TenantID == t.TenantID && x.Name == t.Name {
			return fmt.Errorf("tree: %w", domain.ErrAlreadyExists)
		}
	}
	s.trees[t.ID] = *t
	return nil
}

func (s *fakeStore) GetTreeByName(_ context.Context, tenantID, name string) (*domain.Tree, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, t := range s.trees {
		if t.TenantID == tenantID && t.Name == name {
			return &t, nil
		}
	}
	return nil, nf("tree")
}

func (s *fakeStore) ListTrees(_ context.Context, tenantID string) ([]domain.Tree, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.failList != nil {
		return nil, s.failList
	}
	out := []domain.Tree{}
	for _, t := range s.trees {
		if tenantID == "" || t.TenantID == tenantID {
			out = append(out, t)
		}
	}
	return out, nil
}

func (s *fakeStore) UpdateTree(_ context.Context, t *domain.Tree) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.trees[t.ID] = *t
	return nil
}

func (s *fakeStore) DeleteTree(_ context.Context, id string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, ok := s.trees[id]; !ok {
		return nf("tree")
	}
	delete(s.trees, id)
	return nil
}

func (s *fakeStore) GetBranch(_ context.Context, tree, name string) (*domain.Branch, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	b, ok := s.branches[[2]string{tree, name}]
	if !ok {
		return nil, nf("branch")
	}
	return &b, nil
}

func (s *fakeStore) ListBranches(_ context.Context, tree string) ([]domain.Branch, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.Branch{}
	for k, b := range s.branches {
		if k[0] == tree {
			out = append(out, b)
		}
	}
	return out, nil
}

func (s *fakeStore) PutBranch(_ context.Context, b *domain.Branch) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.branches[[2]string{b.TreeID, b.Name}] = *b
	return nil
}

func (s *fakeStore) GetSnapshot(_ context.Context, id string) (*domain.Snapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	sn, ok := s.snapshots[id]
	if !ok {
		return nil, nf("snapshot")
	}
	sn.Parents = slices.Clone(sn.Parents)
	return &sn, nil
}

func (s *fakeStore) CountSnapshots(_ context.Context, tree string) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	n := 0
	for _, sn := range s.snapshots {
		if tree == "" || sn.TreeID == tree {
			n++
		}
	}
	return n, nil
}

func (s *fakeStore) ListTags(_ context.Context, tree string) ([]domain.Tag, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.Tag{}
	for k, t := range s.tags {
		if k[0] == tree {
			out = append(out, t)
		}
	}
	return out, nil
}

func (s *fakeStore) ListStaged(_ context.Context, tree, branch string) ([]domain.StagedSnapshot, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.StagedSnapshot{}
	for _, st := range s.staged {
		if st.TreeID == tree && (branch == "" || st.BranchName == branch) {
			out = append(out, st)
		}
	}
	return out, nil
}

func (s *fakeStore) CountStaged(context.Context) (int, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	return len(s.staged), nil
}

func (s *fakeStore) PutProtection(_ context.Context, p *domain.BranchProtection) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.protections[[2]string{p.TreeID, p.Pattern}] = *p
	return nil
}

func (s *fakeStore) ListProtections(_ context.Context, tree string) ([]domain.BranchProtection, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.BranchProtection{}
	for k, p := range s.protections {
		if k[0] == tree {
			out = append(out, p)
		}
	}
	return out, nil
}

func (s *fakeStore) DeleteProtection(_ context.Context, tree, pattern string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := [2]string{tree, pattern}
	if _, ok := s.protections[k]; !ok {
		return nf("protection")
	}
	delete(s.protections, k)
	return nil
}

func (s *fakeStore) CreateMergeRequest(_ context.Context, mr *domain.MergeRequest) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	n := 1
	for k := range s.mrs {
		if k[0] == mr.TreeID {
			n++
		}
	}
	mr.Number = n
	if mr.ID == "" {
		mr.ID = newID()
	}
	s.mrs[[2]string{mr.TreeID, fmt.Sprint(n)}] = *mr
	return nil
}

func (s *fakeStore) GetMergeRequest(_ context.Context, tree string, n int) (*domain.MergeRequest, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	mr, ok := s.mrs[[2]string{tree, fmt.Sprint(n)}]
	if !ok {
		return nil, nf("merge request")
	}
	return &mr, nil
}

func (s *fakeStore) ListMergeRequests(_ context.Context, tree, state string) ([]domain.MergeRequest, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.MergeRequest{}
	for k, mr := range s.mrs {
		if k[0] == tree && (state == "" || mr.State == state) {
			out = append(out, mr)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Number < out[j].Number })
	return out, nil
}

func (s *fakeStore) UpdateMergeRequest(_ context.Context, mr *domain.MergeRequest) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.mrs[[2]string{mr.TreeID, fmt.Sprint(mr.Number)}] = *mr
	return nil
}

func (s *fakeStore) PutLicenseRule(_ context.Context, r *domain.LicenseRule) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.licenses[[2]string{r.TreeID, r.PathGlob}] = *r
	return nil
}

func (s *fakeStore) ListLicenseRules(_ context.Context, tree string) ([]domain.LicenseRule, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	out := []domain.LicenseRule{}
	for k, r := range s.licenses {
		if k[0] == tree {
			out = append(out, r)
		}
	}
	return out, nil
}

func (s *fakeStore) DeleteLicenseRule(_ context.Context, tree, glob string) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := [2]string{tree, glob}
	if _, ok := s.licenses[k]; !ok {
		return nf("license rule")
	}
	delete(s.licenses, k)
	return nil
}

// ---- Authenticator ----

type fakeAuth struct {
	store *fakeStore
	// resolve maps a raw bearer token to a principal (set by the fixture).
	resolve   func(raw string) *domain.Principal
	mu        sync.Mutex
	passwords map[string]string // username -> password
	regOpen   bool
}

func hashToken(raw string) string {
	sum := sha256.Sum256([]byte(raw))
	return hex.EncodeToString(sum[:])
}

func (f *fakeAuth) Register(ctx context.Context, username, email, password string) (*domain.Account, error) {
	if !f.regOpen {
		return nil, fmt.Errorf("registration disabled: %w", domain.ErrForbidden)
	}
	t := &domain.Tenant{ID: newID(), Slug: username, Name: username, Kind: domain.TenantUser, CreatedAt: time.Now()}
	if err := f.store.CreateTenant(ctx, t); err != nil {
		return nil, err
	}
	a := &domain.Account{ID: newID(), Username: username, Email: email, TenantID: t.ID, CreatedAt: time.Now()}
	if err := f.store.CreateAccount(ctx, a); err != nil {
		return nil, err
	}
	_ = f.store.PutMembership(ctx, &domain.Membership{TenantID: t.ID, AccountID: a.ID, Role: domain.RoleOwner})
	f.mu.Lock()
	f.passwords[username] = password
	f.mu.Unlock()
	return a, nil
}

func (f *fakeAuth) Login(ctx context.Context, username, password string) (string, *domain.Account, error) {
	f.mu.Lock()
	pw, ok := f.passwords[username]
	f.mu.Unlock()
	if !ok || pw != password {
		return "", nil, fmt.Errorf("bad credentials: %w", domain.ErrUnauthenticated)
	}
	a, err := f.store.GetAccountByUsername(ctx, username)
	if err != nil {
		return "", nil, err
	}
	tok, _, err := f.CreateToken(ctx, a.ID, "session", 0)
	return tok, a, err
}

func (f *fakeAuth) Authenticate(_ context.Context, raw string) (*domain.Principal, error) {
	if f.resolve != nil {
		if p := f.resolve(raw); p != nil {
			return p, nil
		}
	}
	return nil, domain.ErrUnauthenticated
}

func (f *fakeAuth) CreateToken(ctx context.Context, accountID, name string, ttl time.Duration) (string, *domain.APIToken, error) {
	raw := "wt_" + newID()
	t := &domain.APIToken{ID: newID(), AccountID: accountID, Name: name, TokenHash: hashToken(raw), CreatedAt: time.Now()}
	if ttl > 0 {
		exp := t.CreatedAt.Add(ttl)
		t.ExpiresAt = &exp
	}
	return raw, t, f.store.CreateToken(ctx, t)
}

func (f *fakeAuth) RevokeToken(ctx context.Context, accountID, tokenID string) error {
	f.store.mu.Lock()
	t, ok := f.store.tokens[tokenID]
	f.store.mu.Unlock()
	if !ok || t.AccountID != accountID {
		return nf("token")
	}
	return f.store.DeleteToken(ctx, tokenID)
}

func (f *fakeAuth) SetPassword(context.Context, string, string) error { return nil }

// ---- Authorizer ----

type fakeAuthz struct{ store *fakeStore }

var roleActions = map[string][]domain.Action{
	domain.RoleViewer:     {domain.ActTreeRead, domain.ActTenantRead},
	domain.RoleDeveloper:  {domain.ActTreeRead, domain.ActTenantRead, domain.ActTreeWrite, domain.ActTreeCreate, domain.ActMRCreate, domain.ActMRApprove, domain.ActBranchCreate},
	domain.RoleMaintainer: {domain.ActTreeRead, domain.ActTenantRead, domain.ActTreeWrite, domain.ActTreeCreate, domain.ActMRCreate, domain.ActMRApprove, domain.ActMRMerge, domain.ActBranchCreate, domain.ActBranchDelete},
}

func (z *fakeAuthz) Authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	if p != nil && p.IsAdmin {
		return nil
	}
	if act == domain.ActTreeRead && tree != nil {
		if tree.Visibility == domain.VisibilityPublic || (tree.Visibility == domain.VisibilityInternal && p != nil) {
			return nil
		}
	}
	if p == nil {
		return fmt.Errorf("login required: %w", domain.ErrUnauthenticated)
	}
	if act == domain.ActServerAdmin {
		return fmt.Errorf("server admin required: %w", domain.ErrForbidden)
	}
	role, _ := z.RoleOf(ctx, p.AccountID, tenantID)
	switch role {
	case domain.RoleOwner, domain.RoleAdmin:
		return nil
	case "":
		return fmt.Errorf("not a member: %w", domain.ErrForbidden)
	}
	if slices.Contains(roleActions[role], act) {
		return nil
	}
	return fmt.Errorf("%s may not %s: %w", role, act, domain.ErrForbidden)
}

func (z *fakeAuthz) RoleOf(ctx context.Context, accountID, tenantID string) (string, error) {
	m, err := z.store.GetMembership(ctx, tenantID, accountID)
	if err != nil {
		return "", nil
	}
	return m.Role, nil
}

// ---- History ----

type fakeHistory struct{ store *fakeStore }

func (h *fakeHistory) IsAncestor(context.Context, string, string) (bool, error) { return false, nil }

func (h *fakeHistory) Missing(context.Context, string, []string) ([]domain.Snapshot, error) {
	return nil, nil
}

func (h *fakeHistory) Log(ctx context.Context, tip string, limit int) ([]domain.Snapshot, error) {
	seen := map[string]bool{}
	var out []domain.Snapshot
	queue := []string{tip}
	for len(queue) > 0 {
		id := queue[0]
		queue = queue[1:]
		if seen[id] {
			continue
		}
		seen[id] = true
		s, err := h.store.GetSnapshot(ctx, id)
		if err != nil {
			return nil, err
		}
		out = append(out, *s)
		queue = append(queue, s.Parents...)
	}
	sort.Slice(out, func(i, j int) bool {
		if !out[i].CreatedAt.Equal(out[j].CreatedAt) {
			return out[i].CreatedAt.After(out[j].CreatedAt)
		}
		return out[i].ID < out[j].ID
	})
	if limit > 0 && len(out) > limit {
		out = out[:limit]
	}
	return out, nil
}

// ---- ReviewService ----

type fakeReview struct {
	store *fakeStore
	authz *fakeAuthz
}

func (r *fakeReview) CheckPush(context.Context, *domain.Principal, *domain.Tree, string) error {
	return nil
}

func (r *fakeReview) CheckDeleteBranch(context.Context, *domain.Principal, *domain.Tree, string) error {
	return nil
}

func (r *fakeReview) Open(ctx context.Context, p *domain.Principal, tree *domain.Tree, title, desc, src, dst string) (*domain.MergeRequest, error) {
	if err := r.authz.Authorize(ctx, p, domain.ActMRCreate, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if _, err := r.store.GetBranch(ctx, tree.ID, src); err != nil {
		return nil, fmt.Errorf("source branch: %w", domain.ErrInvalid)
	}
	mr := &domain.MergeRequest{TreeID: tree.ID, Title: title, Description: desc, SourceBranch: src, TargetBranch: dst,
		AuthorID: p.AccountID, Author: p.Username, State: domain.MRStateOpen, CreatedAt: time.Now()}
	return mr, r.store.CreateMergeRequest(ctx, mr)
}

func (r *fakeReview) update(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int, act domain.Action, fn func(*domain.MergeRequest) error) (*domain.MergeRequest, error) {
	if err := r.authz.Authorize(ctx, p, act, tree.TenantID, tree); err != nil {
		return nil, err
	}
	mr, err := r.store.GetMergeRequest(ctx, tree.ID, n)
	if err != nil {
		return nil, err
	}
	if err := fn(mr); err != nil {
		return nil, err
	}
	return mr, r.store.UpdateMergeRequest(ctx, mr)
}

func (r *fakeReview) Approve(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int) (*domain.MergeRequest, error) {
	return r.update(ctx, p, tree, n, domain.ActMRApprove, func(mr *domain.MergeRequest) error {
		mr.Approvals = append(mr.Approvals, domain.Approval{AccountID: p.AccountID, Username: p.Username, At: time.Now()})
		return nil
	})
}

func (r *fakeReview) Close(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int) (*domain.MergeRequest, error) {
	return r.update(ctx, p, tree, n, domain.ActMRCreate, func(mr *domain.MergeRequest) error {
		if mr.State != domain.MRStateOpen {
			return fmt.Errorf("not open: %w", domain.ErrConflict)
		}
		mr.State = domain.MRStateClosed
		return nil
	})
}

func (r *fakeReview) Merge(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int) (*domain.MergeRequest, error) {
	return r.update(ctx, p, tree, n, domain.ActMRMerge, func(mr *domain.MergeRequest) error {
		if len(mr.Approvals) == 0 {
			return &domain.PushDenied{Kind: "review_required", Reason: "1 approval required"}
		}
		mr.State = domain.MRStateMerged
		return nil
	})
}

// ---- LicenseChecker ----

type fakeLicense struct{}

func (fakeLicense) CheckPush(context.Context, *domain.Principal, *domain.Tree, []string) error {
	return nil
}

func (fakeLicense) CheckRead(_ context.Context, _ *domain.Principal, _ *domain.Tree, paths []string) error {
	for _, p := range paths {
		if strings.HasPrefix(p, "secret/") {
			return &domain.PushDenied{Kind: "license_violation", Reason: "proprietary path", Path: p, License: "LicenseRef-Proprietary"}
		}
	}
	return nil
}

// ---- StagedService ----

type fakeStaged struct{ store *fakeStore }

func (f *fakeStaged) Stage(context.Context, *domain.Principal, *domain.Tree, *domain.StagedSnapshot) error {
	return nil
}

func (f *fakeStaged) List(ctx context.Context, _ *domain.Principal, tree *domain.Tree, branch string) ([]domain.StagedSnapshot, error) {
	return f.store.ListStaged(ctx, tree.ID, branch)
}

func (f *fakeStaged) ClearOnPush(context.Context, *domain.Principal, *domain.Tree, string) error {
	return nil
}

// ---- EventBus ----

type fakeBus struct {
	mu   sync.Mutex
	subs map[int]fakeSub
	next int
}

type fakeSub struct {
	f  domain.EventFilter
	ch chan domain.Event
}

func newFakeBus() *fakeBus { return &fakeBus{subs: map[int]fakeSub{}} }

func (b *fakeBus) Publish(e domain.Event) {
	b.mu.Lock()
	defer b.mu.Unlock()
	for _, s := range b.subs {
		if s.f.TenantID != "" && s.f.TenantID != e.TenantID {
			continue
		}
		if s.f.TreeID != "" && s.f.TreeID != e.TreeID {
			continue
		}
		if len(s.f.Types) > 0 && !slices.Contains(s.f.Types, e.Type) {
			continue
		}
		select {
		case s.ch <- e:
		default:
		}
	}
}

func (b *fakeBus) Subscribe(f domain.EventFilter) (<-chan domain.Event, func()) {
	b.mu.Lock()
	defer b.mu.Unlock()
	id := b.next
	b.next++
	ch := make(chan domain.Event, 16)
	b.subs[id] = fakeSub{f: f, ch: ch}
	var once sync.Once
	return ch, func() {
		once.Do(func() {
			b.mu.Lock()
			delete(b.subs, id)
			b.mu.Unlock()
		})
	}
}

func (b *fakeBus) subscribers() int {
	b.mu.Lock()
	defer b.mu.Unlock()
	return len(b.subs)
}

// ---- AuditLog ----

type fakeAudit struct {
	mu     sync.Mutex
	events []domain.AuditEvent
}

func (a *fakeAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, e)
}

func (a *fakeAudit) Query(_ context.Context, f domain.AuditFilter) ([]domain.AuditEvent, error) {
	a.mu.Lock()
	defer a.mu.Unlock()
	out := []domain.AuditEvent{}
	for i := len(a.events) - 1; i >= 0; i-- {
		e := a.events[i]
		if (f.TenantID == "" || e.TenantID == f.TenantID) && (f.Action == "" || e.Action == f.Action) {
			out = append(out, e)
		}
		if f.Limit > 0 && len(out) == f.Limit {
			break
		}
	}
	return out, nil
}

func (a *fakeAudit) actions() []string {
	a.mu.Lock()
	defer a.mu.Unlock()
	var out []string
	for _, e := range a.events {
		out = append(out, e.Action)
	}
	return out
}

// ---- object store ----

type fakeObjects struct {
	mu   sync.Mutex
	objs map[hash.Hash][]byte
}

func newFakeObjects() *fakeObjects { return &fakeObjects{objs: map[hash.Hash][]byte{}} }

func (o *fakeObjects) put(data []byte) hash.Hash {
	h := hash.Sum(data)
	o.mu.Lock()
	o.objs[h] = data
	o.mu.Unlock()
	return h
}

func (o *fakeObjects) Put(_ context.Context, h hash.Hash, data []byte) error {
	if hash.Sum(data) != h {
		return objstore.ErrHashMismatch
	}
	o.mu.Lock()
	o.objs[h] = data
	o.mu.Unlock()
	return nil
}

func (o *fakeObjects) Get(_ context.Context, h hash.Hash) ([]byte, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	d, ok := o.objs[h]
	if !ok {
		return nil, objstore.ErrNotFound
	}
	return d, nil
}

func (o *fakeObjects) Has(_ context.Context, h hash.Hash) (bool, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	_, ok := o.objs[h]
	return ok, nil
}

func (o *fakeObjects) Delete(_ context.Context, h hash.Hash) error {
	o.mu.Lock()
	defer o.mu.Unlock()
	delete(o.objs, h)
	return nil
}

func (o *fakeObjects) Stats(context.Context) (int64, int64, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	var n int64
	for _, d := range o.objs {
		n += int64(len(d))
	}
	return int64(len(o.objs)), n, nil
}
