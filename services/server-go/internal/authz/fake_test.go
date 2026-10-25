package authz

import (
	"context"
	"fmt"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// fakeStore is an in-memory domain.MetaStore covering the lookups the
// authorizer performs. Any other method panics (nil embedded interface).
type fakeStore struct {
	domain.MetaStore

	mu          sync.Mutex
	tenants     map[string]domain.Tenant
	accounts    map[string]domain.Account
	memberships map[string]domain.Membership // tenant/account
	teams       map[string]domain.Team
	teamMembers []domain.TeamMember
	roles       map[string]domain.CustomRole // tenant/name
	policies    map[string][]domain.Policy   // tenant -> policies
	grants      map[string][]domain.Grant    // tree -> grants
	sas         map[string]domain.ServiceAccount
	calls       map[string]int
	fail        map[string]error // method -> injected error
}

func newFakeStore() *fakeStore {
	return &fakeStore{
		tenants:     map[string]domain.Tenant{},
		accounts:    map[string]domain.Account{},
		memberships: map[string]domain.Membership{},
		teams:       map[string]domain.Team{},
		roles:       map[string]domain.CustomRole{},
		policies:    map[string][]domain.Policy{},
		grants:      map[string][]domain.Grant{},
		sas:         map[string]domain.ServiceAccount{},
		calls:       map[string]int{},
		fail:        map[string]error{},
	}
}

func notFound(what string) error { return fmt.Errorf("%s: %w", what, domain.ErrNotFound) }

// enter records a call and returns the injected error for method.
func (f *fakeStore) enter(method string) error {
	f.calls[method]++
	return f.fail[method]
}

func (f *fakeStore) callCount(method string) int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return f.calls[method]
}

func (f *fakeStore) GetTenant(_ context.Context, id string) (*domain.Tenant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("GetTenant"); err != nil {
		return nil, err
	}
	t, ok := f.tenants[id]
	if !ok {
		return nil, notFound("tenant")
	}
	return &t, nil
}

func (f *fakeStore) GetAccount(_ context.Context, id string) (*domain.Account, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("GetAccount"); err != nil {
		return nil, err
	}
	a, ok := f.accounts[id]
	if !ok {
		return nil, notFound("account")
	}
	return &a, nil
}

func (f *fakeStore) GetMembership(_ context.Context, tenantID, accountID string) (*domain.Membership, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("GetMembership"); err != nil {
		return nil, err
	}
	m, ok := f.memberships[tenantID+"/"+accountID]
	if !ok {
		return nil, notFound("membership")
	}
	return &m, nil
}

func (f *fakeStore) ListAccountTeams(_ context.Context, accountID string) ([]domain.TeamMember, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("ListAccountTeams"); err != nil {
		return nil, err
	}
	out := []domain.TeamMember{}
	for _, m := range f.teamMembers {
		if m.AccountID == accountID {
			out = append(out, m)
		}
	}
	return out, nil
}

func (f *fakeStore) GetTeam(_ context.Context, id string) (*domain.Team, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("GetTeam"); err != nil {
		return nil, err
	}
	t, ok := f.teams[id]
	if !ok {
		return nil, notFound("team")
	}
	return &t, nil
}

func (f *fakeStore) GetRole(_ context.Context, tenantID, name string) (*domain.CustomRole, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("GetRole"); err != nil {
		return nil, err
	}
	r, ok := f.roles[tenantID+"/"+name]
	if !ok {
		return nil, notFound("role")
	}
	return &r, nil
}

func (f *fakeStore) ListPolicies(_ context.Context, tenantID string) ([]domain.Policy, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("ListPolicies"); err != nil {
		return nil, err
	}
	return append([]domain.Policy{}, f.policies[tenantID]...), nil
}

func (f *fakeStore) ListGrants(_ context.Context, treeID string) ([]domain.Grant, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("ListGrants"); err != nil {
		return nil, err
	}
	return append([]domain.Grant{}, f.grants[treeID]...), nil
}

func (f *fakeStore) GetServiceAccount(_ context.Context, id string) (*domain.ServiceAccount, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if err := f.enter("GetServiceAccount"); err != nil {
		return nil, err
	}
	sa, ok := f.sas[id]
	if !ok {
		return nil, notFound("service account")
	}
	return &sa, nil
}

// ---- fixture helpers ----

// env is a populated store plus authorizer.
type env struct {
	t     *testing.T
	store *fakeStore
	az    *Authorizer
}

func newEnv(t *testing.T) *env {
	t.Helper()
	st := newFakeStore()
	return &env{t: t, store: st, az: New(st, Options{})}
}

func (e *env) tenant(id, slug string) *domain.Tenant {
	t := domain.Tenant{ID: id, Slug: slug, Name: slug, Kind: domain.TenantOrg}
	e.store.tenants[id] = t
	return &t
}

// user creates an account (with a personal tenant) and returns its
// principal.
func (e *env) user(name string) *domain.Principal {
	a := domain.Account{ID: "acct-" + name, Username: name, TenantID: "tenant-" + name}
	e.store.accounts[a.ID] = a
	e.store.tenants[a.TenantID] = domain.Tenant{ID: a.TenantID, Slug: name, Kind: domain.TenantUser}
	e.store.memberships[a.TenantID+"/"+a.ID] = domain.Membership{TenantID: a.TenantID, AccountID: a.ID, Role: domain.RoleOwner}
	return &domain.Principal{AccountID: a.ID, Username: name, TenantID: a.TenantID}
}

func (e *env) updateAccount(p *domain.Principal, fn func(*domain.Account)) {
	a := e.store.accounts[p.AccountID]
	fn(&a)
	e.store.accounts[p.AccountID] = a
}

func (e *env) member(tenantID string, p *domain.Principal, role string) {
	e.store.memberships[tenantID+"/"+p.AccountID] = domain.Membership{TenantID: tenantID, AccountID: p.AccountID, Role: role}
}

func (e *env) team(id, tenantID, slug string, members ...*domain.Principal) {
	e.store.teams[id] = domain.Team{ID: id, TenantID: tenantID, Slug: slug, Name: slug}
	for _, m := range members {
		e.store.teamMembers = append(e.store.teamMembers, domain.TeamMember{TeamID: id, AccountID: m.AccountID, Role: domain.TeamRoleMember})
	}
}

func (e *env) grant(tree *domain.Tree, subjectType, subjectID, subject, role string) {
	e.store.grants[tree.ID] = append(e.store.grants[tree.ID], domain.Grant{
		TreeID: tree.ID, SubjectType: subjectType, SubjectID: subjectID, Subject: subject, Role: role,
	})
}

func (e *env) role(tenantID, name string, perms ...domain.Action) {
	e.store.roles[tenantID+"/"+name] = domain.CustomRole{TenantID: tenantID, Name: name, Permissions: perms}
}

// serviceAccount creates a service account and returns its principal.
func (e *env) serviceAccount(id, tenantID, name, role string) *domain.Principal {
	e.store.sas[id] = domain.ServiceAccount{ID: id, TenantID: tenantID, Name: name, Role: role}
	slug := e.store.tenants[tenantID].Slug
	return &domain.Principal{Kind: domain.PrincipalServiceAccount, ServiceAccountID: id, Username: "sa:" + slug + "/" + name, TenantID: tenantID}
}

func (e *env) policy(tenantID, name string, enabled bool, sts ...domain.PolicyStatement) {
	e.store.policies[tenantID] = append(e.store.policies[tenantID], domain.Policy{
		ID: "pol-" + name, TenantID: tenantID, Name: name, Enabled: enabled, Statements: sts, Version: 1,
	})
}

func tree(id, tenantID, name, vis string) *domain.Tree {
	return &domain.Tree{ID: id, TenantID: tenantID, Name: name, Visibility: vis, DefaultBranch: "main"}
}

func stmt(effect string, actions, resources, principals []string) domain.PolicyStatement {
	return domain.PolicyStatement{Effect: effect, Actions: actions, Resources: resources, Principals: principals}
}

func strs(s ...string) []string { return s }
