package iam

import (
	"context"
	"fmt"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// fakeStore is a minimal in-memory domain.MetaStore covering the methods
// this package uses. Calling any other method panics (nil embedded value).
type fakeStore struct {
	domain.MetaStore

	mu          sync.Mutex
	tenants     map[string]domain.Tenant  // id -> tenant
	accounts    map[string]domain.Account // id -> account
	memberships map[string]domain.Membership
	tokens      map[string]domain.APIToken // id -> token
	touches     int
}

func newFakeStore() *fakeStore {
	return &fakeStore{
		tenants:     map[string]domain.Tenant{},
		accounts:    map[string]domain.Account{},
		memberships: map[string]domain.Membership{},
		tokens:      map[string]domain.APIToken{},
	}
}

func notFound(what string) error { return fmt.Errorf("%s: %w", what, domain.ErrNotFound) }

func (f *fakeStore) CreateTenant(_ context.Context, t *domain.Tenant) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, x := range f.tenants {
		if x.Slug == t.Slug {
			return fmt.Errorf("tenant slug: %w", domain.ErrAlreadyExists)
		}
	}
	f.tenants[t.ID] = *t
	return nil
}

func (f *fakeStore) GetTenant(_ context.Context, id string) (*domain.Tenant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	t, ok := f.tenants[id]
	if !ok {
		return nil, notFound("tenant")
	}
	return &t, nil
}

func (f *fakeStore) GetTenantBySlug(_ context.Context, slug string) (*domain.Tenant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, t := range f.tenants {
		if t.Slug == slug {
			return &t, nil
		}
	}
	return nil, notFound("tenant")
}

func (f *fakeStore) CreateAccount(_ context.Context, a *domain.Account) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, x := range f.accounts {
		if x.Username == a.Username {
			return fmt.Errorf("account: %w", domain.ErrAlreadyExists)
		}
	}
	f.accounts[a.ID] = *a
	return nil
}

func (f *fakeStore) GetAccount(_ context.Context, id string) (*domain.Account, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	a, ok := f.accounts[id]
	if !ok {
		return nil, notFound("account")
	}
	return &a, nil
}

func (f *fakeStore) GetAccountByUsername(_ context.Context, username string) (*domain.Account, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, a := range f.accounts {
		if a.Username == username {
			return &a, nil
		}
	}
	return nil, notFound("account")
}

func (f *fakeStore) ListAccounts(context.Context) ([]domain.Account, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := make([]domain.Account, 0, len(f.accounts))
	for _, a := range f.accounts {
		out = append(out, a)
	}
	return out, nil
}

func (f *fakeStore) UpdateAccount(_ context.Context, a *domain.Account) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if _, ok := f.accounts[a.ID]; !ok {
		return notFound("account")
	}
	f.accounts[a.ID] = *a
	return nil
}

func memberKey(tenantID, accountID string) string { return tenantID + "/" + accountID }

func (f *fakeStore) PutMembership(_ context.Context, m *domain.Membership) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.memberships[memberKey(m.TenantID, m.AccountID)] = *m
	return nil
}

func (f *fakeStore) GetMembership(_ context.Context, tenantID, accountID string) (*domain.Membership, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	m, ok := f.memberships[memberKey(tenantID, accountID)]
	if !ok {
		return nil, notFound("membership")
	}
	return &m, nil
}

func (f *fakeStore) DeleteMembership(_ context.Context, tenantID, accountID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	delete(f.memberships, memberKey(tenantID, accountID))
	return nil
}

func (f *fakeStore) CreateToken(_ context.Context, t *domain.APIToken) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if _, ok := f.tokens[t.ID]; ok {
		return fmt.Errorf("token: %w", domain.ErrAlreadyExists)
	}
	f.tokens[t.ID] = *t
	return nil
}

func (f *fakeStore) GetTokenByHash(_ context.Context, h string) (*domain.APIToken, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, t := range f.tokens {
		if t.TokenHash == h {
			return &t, nil
		}
	}
	return nil, notFound("token")
}

func (f *fakeStore) ListTokens(_ context.Context, accountID string) ([]domain.APIToken, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.APIToken{}
	for _, t := range f.tokens {
		if t.AccountID == accountID {
			out = append(out, t)
		}
	}
	return out, nil
}

func (f *fakeStore) DeleteToken(_ context.Context, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if _, ok := f.tokens[id]; !ok {
		return notFound("token")
	}
	delete(f.tokens, id)
	return nil
}

func (f *fakeStore) TouchToken(_ context.Context, id string, at time.Time) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	t, ok := f.tokens[id]
	if !ok {
		return notFound("token")
	}
	t.LastUsedAt = &at
	f.tokens[id] = t
	f.touches++
	return nil
}

func (f *fakeStore) touchCount() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.touches
}

// fakeClock is a manually advanced clock.
type fakeClock struct {
	mu sync.Mutex
	t  time.Time
}

func newFakeClock() *fakeClock {
	return &fakeClock{t: time.Date(2026, 1, 2, 3, 4, 5, 0, time.UTC)}
}

func (c *fakeClock) Now() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.t
}

func (c *fakeClock) Advance(d time.Duration) {
	c.mu.Lock()
	c.t = c.t.Add(d)
	c.mu.Unlock()
}

// fastParams keeps argon2 cheap in tests.
var fastParams = PasswordParams{Memory: 64, Time: 1, Threads: 1}

const testPassword = "correct horse battery"

type testEnv struct {
	store *fakeStore
	clock *fakeClock
	svc   *Service
}

func newTestEnv(t *testing.T, mod func(*Options)) *testEnv {
	t.Helper()
	st := newFakeStore()
	clk := newFakeClock()
	opts := Options{
		AllowRegistration: true,
		DisableRateLimit:  true,
		Password:          fastParams,
		Clock:             clk.Now,
	}
	if mod != nil {
		mod(&opts)
	}
	return &testEnv{store: st, clock: clk, svc: New(st, opts)}
}

func (e *testEnv) register(t *testing.T, username string) *domain.Account {
	t.Helper()
	a, err := e.svc.Register(context.Background(), username, username+"@example.com", testPassword)
	if err != nil {
		t.Fatalf("Register(%q): %v", username, err)
	}
	return a
}

func principalOf(a *domain.Account) *domain.Principal {
	return &domain.Principal{AccountID: a.ID, Username: a.Username, TenantID: a.TenantID, IsAdmin: a.IsAdmin}
}
