package meta

import (
	"cmp"
	"context"
	"fmt"
	"maps"
	"slices"
	"strconv"
	"sync"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Memory is an in-memory [domain.MetaStore] with the same semantics as
// [Bolt]. It is safe for concurrent use; every method runs under a single
// RWMutex, so each call is atomic and linearizable.
type Memory struct {
	mu     sync.RWMutex
	closed bool

	tenants       map[string]domain.Tenant
	tenantBySlug  map[string]string
	accounts      map[string]domain.Account
	accountByName map[string]string
	members       map[string]domain.Membership // key(tenantID, accountID)
	tokens        map[string]domain.APIToken
	tokenByHash   map[string]string
	trees         map[string]domain.Tree
	treeByName    map[string]string                           // key(tenantID, name) -> tree id
	branches      map[string]map[string]domain.Branch         // tree -> name
	snapshots     map[string]domain.Snapshot                  // id
	snapsByTree   map[string]map[string]struct{}              // tree -> snapshot ids
	tags          map[string]map[string]domain.Tag            // tree -> name
	staged        map[string]map[string]domain.StagedSnapshot // tree -> key(accountID, branch)
	protections   map[string]map[string]domain.BranchProtection
	mrs           map[string]map[int]domain.MergeRequest
	mrSeq         map[string]int
	audit         []memAudit // sorted by (At, seq)
	auditSeq      uint64
	licenses      map[string]map[string]domain.LicenseRule
	access        map[string][]byte // key(tenantID, kind)

	iam  memIAM  // v0.4 IAM (iam_memory.go)
	docs memDocs // v0.5 documents (docs.go)
}

type memAudit struct {
	seq uint64
	ev  domain.AuditEvent
}

// NewMemory returns an empty in-memory store.
func NewMemory() *Memory {
	return &Memory{
		tenants:       map[string]domain.Tenant{},
		tenantBySlug:  map[string]string{},
		accounts:      map[string]domain.Account{},
		accountByName: map[string]string{},
		members:       map[string]domain.Membership{},
		tokens:        map[string]domain.APIToken{},
		tokenByHash:   map[string]string{},
		trees:         map[string]domain.Tree{},
		treeByName:    map[string]string{},
		branches:      map[string]map[string]domain.Branch{},
		snapshots:     map[string]domain.Snapshot{},
		snapsByTree:   map[string]map[string]struct{}{},
		tags:          map[string]map[string]domain.Tag{},
		staged:        map[string]map[string]domain.StagedSnapshot{},
		protections:   map[string]map[string]domain.BranchProtection{},
		mrs:           map[string]map[int]domain.MergeRequest{},
		mrSeq:         map[string]int{},
		licenses:      map[string]map[string]domain.LicenseRule{},
		access:        map[string][]byte{},
		iam:           newMemIAM(),
		docs:          newMemDocs(),
	}
}

// read runs fn under the read lock.
func (m *Memory) read(ctx context.Context, fn func() error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	m.mu.RLock()
	defer m.mu.RUnlock()
	if m.closed {
		return ErrClosed
	}
	return fn()
}

// write runs fn under the write lock.
func (m *Memory) write(ctx context.Context, fn func() error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	m.mu.Lock()
	defer m.mu.Unlock()
	if m.closed {
		return ErrClosed
	}
	return fn()
}

// inner returns the nested map for k, creating it if needed.
func inner[K comparable, V any](outer map[string]map[K]V, k string) map[K]V {
	in, ok := outer[k]
	if !ok {
		in = map[K]V{}
		outer[k] = in
	}
	return in
}

// valuesByKey returns the values of mp ordered by key, passed through clone.
func valuesByKey[K cmp.Ordered, V any](mp map[K]V, clone func(V) V) []V {
	out := make([]V, 0, len(mp))
	for _, k := range slices.Sorted(maps.Keys(mp)) {
		out = append(out, clone(mp[k]))
	}
	return out
}

// ---- tenants ----------------------------------------------------------

// CreateTenant implements domain.MetaStore.
func (m *Memory) CreateTenant(ctx context.Context, t *domain.Tenant) error {
	if t == nil {
		return errNil("tenant")
	}
	return m.write(ctx, func() error {
		if err := prepareTenant(t); err != nil {
			return err
		}
		if _, ok := m.tenants[t.ID]; ok {
			return alreadyExists("tenant", t.ID)
		}
		if _, ok := m.tenantBySlug[t.Slug]; ok {
			return alreadyExists("tenant slug", t.Slug)
		}
		m.tenants[t.ID] = normTenant(*t)
		m.tenantBySlug[t.Slug] = t.ID
		return nil
	})
}

// GetTenant implements domain.MetaStore.
func (m *Memory) GetTenant(ctx context.Context, id string) (*domain.Tenant, error) {
	var out domain.Tenant
	err := m.read(ctx, func() error {
		t, ok := m.tenants[id]
		if !ok {
			return notFound("tenant", id)
		}
		out = t
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// GetTenantBySlug implements domain.MetaStore.
func (m *Memory) GetTenantBySlug(ctx context.Context, slug string) (*domain.Tenant, error) {
	var out domain.Tenant
	err := m.read(ctx, func() error {
		id, ok := m.tenantBySlug[slug]
		if !ok {
			return notFound("tenant slug", slug)
		}
		out = m.tenants[id]
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTenants implements domain.MetaStore (ordered by slug).
func (m *Memory) ListTenants(ctx context.Context) ([]domain.Tenant, error) {
	var out []domain.Tenant
	err := m.read(ctx, func() error {
		out = make([]domain.Tenant, 0, len(m.tenants))
		for _, slug := range slices.Sorted(maps.Keys(m.tenantBySlug)) {
			out = append(out, m.tenants[m.tenantBySlug[slug]])
		}
		return nil
	})
	return out, err
}

// UpdateTenant implements domain.MetaStore.
func (m *Memory) UpdateTenant(ctx context.Context, t *domain.Tenant) error {
	if t == nil {
		return errNil("tenant")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tenant id", t.ID, "tenant slug", t.Slug); err != nil {
			return err
		}
		old, ok := m.tenants[t.ID]
		if !ok {
			return notFound("tenant", t.ID)
		}
		if owner, ok := m.tenantBySlug[t.Slug]; ok && owner != t.ID {
			return alreadyExists("tenant slug", t.Slug)
		}
		if t.CreatedAt.IsZero() {
			t.CreatedAt = old.CreatedAt
		}
		delete(m.tenantBySlug, old.Slug)
		m.tenantBySlug[t.Slug] = t.ID
		m.tenants[t.ID] = normTenant(*t)
		return nil
	})
}

// ---- accounts ---------------------------------------------------------

// CreateAccount implements domain.MetaStore.
func (m *Memory) CreateAccount(ctx context.Context, a *domain.Account) error {
	if a == nil {
		return errNil("account")
	}
	return m.write(ctx, func() error {
		if err := prepareAccount(a); err != nil {
			return err
		}
		if _, ok := m.accounts[a.ID]; ok {
			return alreadyExists("account", a.ID)
		}
		if _, ok := m.accountByName[a.Username]; ok {
			return alreadyExists("account username", a.Username)
		}
		m.accounts[a.ID] = normAccount(*a)
		m.accountByName[a.Username] = a.ID
		return nil
	})
}

// GetAccount implements domain.MetaStore.
func (m *Memory) GetAccount(ctx context.Context, id string) (*domain.Account, error) {
	var out domain.Account
	err := m.read(ctx, func() error {
		a, ok := m.accounts[id]
		if !ok {
			return notFound("account", id)
		}
		out = a
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// GetAccountByUsername implements domain.MetaStore.
func (m *Memory) GetAccountByUsername(ctx context.Context, username string) (*domain.Account, error) {
	var out domain.Account
	err := m.read(ctx, func() error {
		id, ok := m.accountByName[username]
		if !ok {
			return notFound("account username", username)
		}
		out = m.accounts[id]
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListAccounts implements domain.MetaStore (ordered by username).
func (m *Memory) ListAccounts(ctx context.Context) ([]domain.Account, error) {
	var out []domain.Account
	err := m.read(ctx, func() error {
		out = make([]domain.Account, 0, len(m.accounts))
		for _, name := range slices.Sorted(maps.Keys(m.accountByName)) {
			out = append(out, m.accounts[m.accountByName[name]])
		}
		return nil
	})
	return out, err
}

// UpdateAccount implements domain.MetaStore.
func (m *Memory) UpdateAccount(ctx context.Context, a *domain.Account) error {
	if a == nil {
		return errNil("account")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("account id", a.ID, "account username", a.Username); err != nil {
			return err
		}
		old, ok := m.accounts[a.ID]
		if !ok {
			return notFound("account", a.ID)
		}
		if owner, ok := m.accountByName[a.Username]; ok && owner != a.ID {
			return alreadyExists("account username", a.Username)
		}
		if a.CreatedAt.IsZero() {
			a.CreatedAt = old.CreatedAt
		}
		delete(m.accountByName, old.Username)
		m.accountByName[a.Username] = a.ID
		m.accounts[a.ID] = normAccount(*a)
		return nil
	})
}

// ---- memberships ------------------------------------------------------

// PutMembership implements domain.MetaStore (insert or replace).
func (m *Memory) PutMembership(ctx context.Context, mb *domain.Membership) error {
	if mb == nil {
		return errNil("membership")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tenant id", mb.TenantID, "account id", mb.AccountID); err != nil {
			return err
		}
		k := key(mb.TenantID, mb.AccountID)
		if mb.CreatedAt.IsZero() {
			if old, ok := m.members[k]; ok {
				mb.CreatedAt = old.CreatedAt
			} else {
				mb.CreatedAt = now()
			}
		}
		m.members[k] = normMembership(*mb)
		return nil
	})
}

// GetMembership implements domain.MetaStore.
func (m *Memory) GetMembership(ctx context.Context, tenantID, accountID string) (*domain.Membership, error) {
	var out domain.Membership
	err := m.read(ctx, func() error {
		mb, ok := m.members[key(tenantID, accountID)]
		if !ok {
			return notFound("membership", tenantID, accountID)
		}
		out = mb
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTenantMembers implements domain.MetaStore (ordered by account id).
func (m *Memory) ListTenantMembers(ctx context.Context, tenantID string) ([]domain.Membership, error) {
	var out []domain.Membership
	err := m.read(ctx, func() error {
		out = []domain.Membership{}
		for _, mb := range m.members {
			if mb.TenantID == tenantID {
				out = append(out, mb)
			}
		}
		slices.SortFunc(out, func(a, b domain.Membership) int { return cmp.Compare(a.AccountID, b.AccountID) })
		return nil
	})
	return out, err
}

// ListAccountMemberships implements domain.MetaStore (ordered by tenant id).
func (m *Memory) ListAccountMemberships(ctx context.Context, accountID string) ([]domain.Membership, error) {
	var out []domain.Membership
	err := m.read(ctx, func() error {
		out = []domain.Membership{}
		for _, mb := range m.members {
			if mb.AccountID == accountID {
				out = append(out, mb)
			}
		}
		slices.SortFunc(out, func(a, b domain.Membership) int { return cmp.Compare(a.TenantID, b.TenantID) })
		return nil
	})
	return out, err
}

// DeleteMembership implements domain.MetaStore.
func (m *Memory) DeleteMembership(ctx context.Context, tenantID, accountID string) error {
	return m.write(ctx, func() error {
		k := key(tenantID, accountID)
		if _, ok := m.members[k]; !ok {
			return notFound("membership", tenantID, accountID)
		}
		delete(m.members, k)
		return nil
	})
}

// ---- tokens -----------------------------------------------------------

// CreateToken implements domain.MetaStore.
func (m *Memory) CreateToken(ctx context.Context, t *domain.APIToken) error {
	if t == nil {
		return errNil("token")
	}
	return m.write(ctx, func() error {
		if err := prepareToken(t); err != nil {
			return err
		}
		if _, ok := m.tokens[t.ID]; ok {
			return alreadyExists("token", t.ID)
		}
		if _, ok := m.tokenByHash[t.TokenHash]; ok {
			return alreadyExists("token hash", "")
		}
		if t.ServiceAccountID != "" && !hasKey(m.iam.sas, t.ServiceAccountID) {
			return notFound("service account", t.ServiceAccountID)
		}
		m.tokens[t.ID] = normToken(*t)
		m.tokenByHash[t.TokenHash] = t.ID
		return nil
	})
}

// GetTokenByHash implements domain.MetaStore.
func (m *Memory) GetTokenByHash(ctx context.Context, tokenHash string) (*domain.APIToken, error) {
	var out domain.APIToken
	err := m.read(ctx, func() error {
		id, ok := m.tokenByHash[tokenHash]
		if !ok {
			return notFound("token hash", "")
		}
		out = normToken(m.tokens[id])
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTokens implements domain.MetaStore (ordered by creation time).
// Service-account keys are never listed.
func (m *Memory) ListTokens(ctx context.Context, accountID string) ([]domain.APIToken, error) {
	var out []domain.APIToken
	err := m.read(ctx, func() error {
		out = []domain.APIToken{}
		for _, t := range m.tokens {
			if t.AccountID == accountID && t.ServiceAccountID == "" {
				out = append(out, normToken(t))
			}
		}
		sortTokens(out)
		return nil
	})
	return out, err
}

// DeleteToken implements domain.MetaStore.
func (m *Memory) DeleteToken(ctx context.Context, id string) error {
	return m.write(ctx, func() error {
		t, ok := m.tokens[id]
		if !ok {
			return notFound("token", id)
		}
		delete(m.tokens, id)
		delete(m.tokenByHash, t.TokenHash)
		return nil
	})
}

// TouchToken implements domain.MetaStore.
func (m *Memory) TouchToken(ctx context.Context, id string, at time.Time) error {
	return m.write(ctx, func() error {
		t, ok := m.tokens[id]
		if !ok {
			return notFound("token", id)
		}
		at = utc(at)
		t.LastUsedAt = &at
		m.tokens[id] = t
		return nil
	})
}

// TouchTokenMeta implements iam.TokenMetaToucher (see Bolt.TouchTokenMeta).
func (m *Memory) TouchTokenMeta(ctx context.Context, id string, at time.Time, lastIP, userAgent string) error {
	return m.write(ctx, func() error {
		t, ok := m.tokens[id]
		if !ok {
			return notFound("token", id)
		}
		touchTokenMeta(&t, at, lastIP, userAgent)
		m.tokens[id] = t
		return nil
	})
}

// ---- trees ------------------------------------------------------------

// CreateTree implements domain.MetaStore.
func (m *Memory) CreateTree(ctx context.Context, t *domain.Tree) error {
	if t == nil {
		return errNil("tree")
	}
	return m.write(ctx, func() error {
		if err := prepareTree(t); err != nil {
			return err
		}
		if _, ok := m.trees[t.ID]; ok {
			return alreadyExists("tree", t.ID)
		}
		nk := key(t.TenantID, t.Name)
		if _, ok := m.treeByName[nk]; ok {
			return alreadyExists("tree", t.TenantID, t.Name)
		}
		m.trees[t.ID] = normTree(*t)
		m.treeByName[nk] = t.ID
		return nil
	})
}

// GetTree implements domain.MetaStore.
func (m *Memory) GetTree(ctx context.Context, id string) (*domain.Tree, error) {
	var out domain.Tree
	err := m.read(ctx, func() error {
		t, ok := m.trees[id]
		if !ok {
			return notFound("tree", id)
		}
		out = t
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// GetTreeByName implements domain.MetaStore.
func (m *Memory) GetTreeByName(ctx context.Context, tenantID, name string) (*domain.Tree, error) {
	var out domain.Tree
	err := m.read(ctx, func() error {
		id, ok := m.treeByName[key(tenantID, name)]
		if !ok {
			return notFound("tree", tenantID, name)
		}
		out = m.trees[id]
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTrees implements domain.MetaStore (ordered by name, then tenant id).
func (m *Memory) ListTrees(ctx context.Context, tenantID string) ([]domain.Tree, error) {
	var out []domain.Tree
	err := m.read(ctx, func() error {
		out = []domain.Tree{}
		for _, t := range m.trees {
			if tenantID == "" || t.TenantID == tenantID {
				out = append(out, t)
			}
		}
		sortTrees(out)
		return nil
	})
	return out, err
}

// UpdateTree implements domain.MetaStore. UpdatedAt is set to now.
func (m *Memory) UpdateTree(ctx context.Context, t *domain.Tree) error {
	if t == nil {
		return errNil("tree")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tree id", t.ID, "tenant id", t.TenantID, "tree name", t.Name); err != nil {
			return err
		}
		old, ok := m.trees[t.ID]
		if !ok {
			return notFound("tree", t.ID)
		}
		nk := key(t.TenantID, t.Name)
		if owner, ok := m.treeByName[nk]; ok && owner != t.ID {
			return alreadyExists("tree", t.TenantID, t.Name)
		}
		if t.CreatedAt.IsZero() {
			t.CreatedAt = old.CreatedAt
		}
		t.UpdatedAt = now()
		delete(m.treeByName, key(old.TenantID, old.Name))
		m.treeByName[nk] = t.ID
		m.trees[t.ID] = normTree(*t)
		return nil
	})
}

// DeleteTree implements domain.MetaStore. It cascades to every per-tree
// record: branches, snapshots, tags, staged snapshots, protections, merge
// requests (and the MR counter), license rules and grants.
func (m *Memory) DeleteTree(ctx context.Context, id string) error {
	return m.write(ctx, func() error {
		t, ok := m.trees[id]
		if !ok {
			return notFound("tree", id)
		}
		delete(m.trees, id)
		delete(m.treeByName, key(t.TenantID, t.Name))
		delete(m.branches, id)
		for sid := range m.snapsByTree[id] {
			delete(m.snapshots, sid)
		}
		delete(m.snapsByTree, id)
		delete(m.tags, id)
		delete(m.staged, id)
		delete(m.protections, id)
		delete(m.mrs, id)
		delete(m.mrSeq, id)
		delete(m.licenses, id)
		delete(m.iam.grants, id)
		return nil
	})
}

// ---- branches ---------------------------------------------------------

// GetBranch implements domain.MetaStore.
func (m *Memory) GetBranch(ctx context.Context, treeID, name string) (*domain.Branch, error) {
	var out domain.Branch
	err := m.read(ctx, func() error {
		b, ok := m.branches[treeID][name]
		if !ok {
			return notFound("branch", treeID, name)
		}
		out = b
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListBranches implements domain.MetaStore (ordered by name).
func (m *Memory) ListBranches(ctx context.Context, treeID string) ([]domain.Branch, error) {
	var out []domain.Branch
	err := m.read(ctx, func() error {
		out = valuesByKey(m.branches[treeID], normBranch)
		return nil
	})
	return out, err
}

// PutBranch implements domain.MetaStore (insert or replace). UpdatedAt is
// set to now.
func (m *Memory) PutBranch(ctx context.Context, b *domain.Branch) error {
	if b == nil {
		return errNil("branch")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tree id", b.TreeID, "branch name", b.Name); err != nil {
			return err
		}
		in := inner(m.branches, b.TreeID)
		prepareBranchPut(b, in[b.Name], hasKey(in, b.Name))
		in[b.Name] = normBranch(*b)
		return nil
	})
}

// CompareAndSwapBranchTip implements domain.MetaStore.
func (m *Memory) CompareAndSwapBranchTip(ctx context.Context, treeID, name, oldTip, newTip string) error {
	return m.write(ctx, func() error {
		if err := checkKeys("tree id", treeID, "branch name", name); err != nil {
			return err
		}
		in := inner(m.branches, treeID)
		cur, exists := in[name]
		next, err := casBranch(treeID, name, cur, exists, oldTip, newTip)
		if err != nil {
			return err
		}
		in[name] = next
		return nil
	})
}

// DeleteBranch implements domain.MetaStore.
func (m *Memory) DeleteBranch(ctx context.Context, treeID, name string) error {
	return m.write(ctx, func() error {
		if _, ok := m.branches[treeID][name]; !ok {
			return notFound("branch", treeID, name)
		}
		delete(m.branches[treeID], name)
		return nil
	})
}

func hasKey[K comparable, V any](mp map[K]V, k K) bool {
	_, ok := mp[k]
	return ok
}

// ---- snapshots --------------------------------------------------------

// PutSnapshot implements domain.MetaStore.
func (m *Memory) PutSnapshot(ctx context.Context, s *domain.Snapshot) error {
	if s == nil {
		return errNil("snapshot")
	}
	return m.write(ctx, func() error {
		if err := prepareSnapshot(s); err != nil {
			return err
		}
		ns := normSnapshot(*s)
		if old, ok := m.snapshots[s.ID]; ok {
			if snapshotContentEqual(&old, &ns) {
				return nil
			}
			return fmt.Errorf("meta: snapshot %q exists with different content: %w", s.ID, domain.ErrConflict)
		}
		m.snapshots[s.ID] = ns
		inner(m.snapsByTree, s.TreeID)[s.ID] = struct{}{}
		return nil
	})
}

// GetSnapshot implements domain.MetaStore.
func (m *Memory) GetSnapshot(ctx context.Context, id string) (*domain.Snapshot, error) {
	var out *domain.Snapshot
	err := m.read(ctx, func() error {
		var err error
		out, err = m.getSnapshotLocked(id)
		return err
	})
	return out, err
}

func (m *Memory) getSnapshotLocked(id string) (*domain.Snapshot, error) {
	s, ok := m.snapshots[id]
	if !ok {
		return nil, notFound("snapshot", id)
	}
	s = normSnapshot(s)
	return &s, nil
}

// ViewSnapshots implements [SnapshotViewer]. The read lock is held while fn
// runs, so fn must not call other methods of m.
func (m *Memory) ViewSnapshots(ctx context.Context, fn func(get func(id string) (*domain.Snapshot, error)) error) error {
	return m.read(ctx, func() error { return fn(m.getSnapshotLocked) })
}

// HasSnapshot implements domain.MetaStore.
func (m *Memory) HasSnapshot(ctx context.Context, id string) (bool, error) {
	var ok bool
	err := m.read(ctx, func() error {
		_, ok = m.snapshots[id]
		return nil
	})
	return ok, err
}

// CountSnapshots implements domain.MetaStore ("" counts all trees).
func (m *Memory) CountSnapshots(ctx context.Context, treeID string) (int, error) {
	var n int
	err := m.read(ctx, func() error {
		if treeID == "" {
			n = len(m.snapshots)
		} else {
			n = len(m.snapsByTree[treeID])
		}
		return nil
	})
	return n, err
}

// ---- tags -------------------------------------------------------------

// PutTag implements domain.MetaStore.
func (m *Memory) PutTag(ctx context.Context, t *domain.Tag) error {
	if t == nil {
		return errNil("tag")
	}
	return m.write(ctx, func() error {
		if err := prepareTag(t); err != nil {
			return err
		}
		in := inner(m.tags, t.TreeID)
		if _, ok := in[t.Name]; ok {
			return alreadyExists("tag", t.TreeID, t.Name)
		}
		in[t.Name] = normTag(*t)
		return nil
	})
}

// GetTag implements domain.MetaStore.
func (m *Memory) GetTag(ctx context.Context, treeID, name string) (*domain.Tag, error) {
	var out domain.Tag
	err := m.read(ctx, func() error {
		t, ok := m.tags[treeID][name]
		if !ok {
			return notFound("tag", treeID, name)
		}
		out = t
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTags implements domain.MetaStore (ordered by name).
func (m *Memory) ListTags(ctx context.Context, treeID string) ([]domain.Tag, error) {
	var out []domain.Tag
	err := m.read(ctx, func() error {
		out = valuesByKey(m.tags[treeID], normTag)
		return nil
	})
	return out, err
}

// DeleteTag implements domain.MetaStore.
func (m *Memory) DeleteTag(ctx context.Context, treeID, name string) error {
	return m.write(ctx, func() error {
		if _, ok := m.tags[treeID][name]; !ok {
			return notFound("tag", treeID, name)
		}
		delete(m.tags[treeID], name)
		return nil
	})
}

// ---- staged -----------------------------------------------------------

// PutStaged implements domain.MetaStore (replaces the entry for the same
// tree, account and branch).
func (m *Memory) PutStaged(ctx context.Context, s *domain.StagedSnapshot) error {
	if s == nil {
		return errNil("staged snapshot")
	}
	return m.write(ctx, func() error {
		if err := prepareStaged(s); err != nil {
			return err
		}
		inner(m.staged, s.TreeID)[stagedKey(s.AccountID, s.BranchName, s.Label)] = normStaged(*s)
		return nil
	})
}

// ListStaged implements domain.MetaStore (newest staged_at first; branch
// "" lists all branches).
func (m *Memory) ListStaged(ctx context.Context, treeID, branch string) ([]domain.StagedSnapshot, error) {
	var out []domain.StagedSnapshot
	err := m.read(ctx, func() error {
		out = []domain.StagedSnapshot{}
		for _, s := range m.staged[treeID] {
			if branch == "" || s.BranchName == branch {
				out = append(out, normStaged(s))
			}
		}
		sortStaged(out)
		return nil
	})
	return out, err
}

// DeleteStaged implements domain.MetaStore (the default, unlabeled slot).
// Deleting a missing entry is a no-op.
func (m *Memory) DeleteStaged(ctx context.Context, treeID, accountID, branch string) error {
	return m.DeleteStagedLabel(ctx, treeID, accountID, branch, "")
}

// DeleteStagedLabel implements domain.MetaStore (v0.8, §16.8). Deleting a
// missing entry is a no-op.
func (m *Memory) DeleteStagedLabel(ctx context.Context, treeID, accountID, branch, label string) error {
	return m.write(ctx, func() error {
		delete(m.staged[treeID], stagedKey(accountID, branch, label))
		return nil
	})
}

// CountStaged implements domain.MetaStore.
func (m *Memory) CountStaged(ctx context.Context) (int, error) {
	var n int
	err := m.read(ctx, func() error {
		for _, in := range m.staged {
			n += len(in)
		}
		return nil
	})
	return n, err
}

// ---- protections ------------------------------------------------------

// PutProtection implements domain.MetaStore (insert or replace).
func (m *Memory) PutProtection(ctx context.Context, p *domain.BranchProtection) error {
	if p == nil {
		return errNil("branch protection")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tree id", p.TreeID, "pattern", p.Pattern); err != nil {
			return err
		}
		inner(m.protections, p.TreeID)[p.Pattern] = normProtection(*p)
		return nil
	})
}

// ListProtections implements domain.MetaStore (ordered by pattern).
func (m *Memory) ListProtections(ctx context.Context, treeID string) ([]domain.BranchProtection, error) {
	var out []domain.BranchProtection
	err := m.read(ctx, func() error {
		out = valuesByKey(m.protections[treeID], normProtection)
		return nil
	})
	return out, err
}

// DeleteProtection implements domain.MetaStore.
func (m *Memory) DeleteProtection(ctx context.Context, treeID, pattern string) error {
	return m.write(ctx, func() error {
		if _, ok := m.protections[treeID][pattern]; !ok {
			return notFound("branch protection", treeID, pattern)
		}
		delete(m.protections[treeID], pattern)
		return nil
	})
}

// ---- merge requests ---------------------------------------------------

// CreateMergeRequest implements domain.MetaStore.
func (m *Memory) CreateMergeRequest(ctx context.Context, mr *domain.MergeRequest) error {
	if mr == nil {
		return errNil("merge request")
	}
	return m.write(ctx, func() error {
		if err := checkKey("tree id", mr.TreeID); err != nil {
			return err
		}
		n := m.mrSeq[mr.TreeID] + 1
		prepareMR(mr, n)
		m.mrSeq[mr.TreeID] = n
		inner(m.mrs, mr.TreeID)[n] = normMR(*mr)
		return nil
	})
}

// GetMergeRequest implements domain.MetaStore.
func (m *Memory) GetMergeRequest(ctx context.Context, treeID string, number int) (*domain.MergeRequest, error) {
	var out domain.MergeRequest
	err := m.read(ctx, func() error {
		mr, ok := m.mrs[treeID][number]
		if !ok {
			return notFound("merge request", treeID, strconv.Itoa(number))
		}
		out = normMR(mr)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListMergeRequests implements domain.MetaStore (number descending; state
// "" lists all states).
func (m *Memory) ListMergeRequests(ctx context.Context, treeID, state string) ([]domain.MergeRequest, error) {
	var out []domain.MergeRequest
	err := m.read(ctx, func() error {
		out = []domain.MergeRequest{}
		for _, mr := range m.mrs[treeID] {
			if state == "" || mr.State == state {
				out = append(out, normMR(mr))
			}
		}
		slices.SortFunc(out, func(a, b domain.MergeRequest) int { return cmp.Compare(b.Number, a.Number) })
		return nil
	})
	return out, err
}

// UpdateMergeRequest implements domain.MetaStore. The record is located by
// (TreeID, Number); UpdatedAt is set to now.
func (m *Memory) UpdateMergeRequest(ctx context.Context, mr *domain.MergeRequest) error {
	if mr == nil {
		return errNil("merge request")
	}
	return m.write(ctx, func() error {
		if err := checkKey("tree id", mr.TreeID); err != nil {
			return err
		}
		old, ok := m.mrs[mr.TreeID][mr.Number]
		if !ok {
			return notFound("merge request", mr.TreeID, strconv.Itoa(mr.Number))
		}
		if err := checkMRUpdate(mr, &old); err != nil {
			return err
		}
		m.mrs[mr.TreeID][mr.Number] = normMR(*mr)
		return nil
	})
}

// ---- audit ------------------------------------------------------------

// AppendAudit implements domain.MetaStore.
func (m *Memory) AppendAudit(ctx context.Context, e *domain.AuditEvent) error {
	if e == nil {
		return errNil("audit event")
	}
	return m.write(ctx, func() error {
		prepareAudit(e)
		m.auditSeq++
		rec := memAudit{seq: m.auditSeq, ev: normAudit(*e)}
		// Appends are almost always in time order, so this is O(1)
		// amortized; out-of-order timestamps are inserted in place.
		i := len(m.audit)
		for i > 0 && m.audit[i-1].ev.At.After(rec.ev.At) {
			i--
		}
		m.audit = slices.Insert(m.audit, i, rec)
		return nil
	})
}

// ListAudit implements domain.MetaStore (newest first).
func (m *Memory) ListAudit(ctx context.Context, f domain.AuditFilter) ([]domain.AuditEvent, error) {
	var out []domain.AuditEvent
	err := m.read(ctx, func() error {
		out = []domain.AuditEvent{}
		limit := auditLimit(&f)
		start := len(m.audit) - 1
		if !f.Until.IsZero() {
			// First index at or after the exclusive upper bound.
			start, _ = slices.BinarySearchFunc(m.audit, f.Until, func(e memAudit, u time.Time) int { return e.ev.At.Compare(u) })
			start--
		}
		for i := start; i >= 0 && len(out) < limit; i-- {
			e := &m.audit[i].ev
			if !f.Since.IsZero() && e.At.Before(f.Since) {
				break
			}
			if auditMatches(&f, e) {
				out = append(out, normAudit(*e))
			}
		}
		return nil
	})
	return out, err
}

// PruneAudit implements domain.MetaStore: it deletes every event whose At
// is before the given instant.
func (m *Memory) PruneAudit(ctx context.Context, before time.Time) (int, error) {
	if before.IsZero() {
		return 0, nil
	}
	n := 0
	err := m.write(ctx, func() error {
		// m.audit is sorted by At: drop the leading run before the bound.
		i := 0
		for i < len(m.audit) && m.audit[i].ev.At.Before(before) {
			i++
		}
		n = i
		m.audit = slices.Delete(m.audit, 0, i)
		return nil
	})
	if err != nil {
		return 0, err
	}
	return n, nil
}

// ---- license rules ----------------------------------------------------

// PutLicenseRule implements domain.MetaStore (insert or replace).
func (m *Memory) PutLicenseRule(ctx context.Context, r *domain.LicenseRule) error {
	if r == nil {
		return errNil("license rule")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tree id", r.TreeID, "path glob", r.PathGlob); err != nil {
			return err
		}
		rr := *r
		rr.ExpiresAt = utcPtr(rr.ExpiresAt)
		inner(m.licenses, r.TreeID)[r.PathGlob] = rr
		return nil
	})
}

// ListLicenseRules implements domain.MetaStore (ordered by path glob).
func (m *Memory) ListLicenseRules(ctx context.Context, treeID string) ([]domain.LicenseRule, error) {
	var out []domain.LicenseRule
	err := m.read(ctx, func() error {
		out = valuesByKey(m.licenses[treeID], func(r domain.LicenseRule) domain.LicenseRule { return r })
		return nil
	})
	return out, err
}

// DeleteLicenseRule implements domain.MetaStore.
func (m *Memory) DeleteLicenseRule(ctx context.Context, treeID, pathGlob string) error {
	return m.write(ctx, func() error {
		if _, ok := m.licenses[treeID][pathGlob]; !ok {
			return notFound("license rule", treeID, pathGlob)
		}
		delete(m.licenses[treeID], pathGlob)
		return nil
	})
}

// ---- access config ----------------------------------------------------

// PutAccessConfig implements domain.MetaStore (insert or replace).
func (m *Memory) PutAccessConfig(ctx context.Context, tenantID, kind string, content []byte) error {
	return m.write(ctx, func() error {
		if err := checkKeys("tenant id", tenantID, "kind", kind); err != nil {
			return err
		}
		m.access[key(tenantID, kind)] = cloneBytes(content)
		return nil
	})
}

// GetAccessConfig implements domain.MetaStore.
func (m *Memory) GetAccessConfig(ctx context.Context, tenantID, kind string) ([]byte, error) {
	var out []byte
	err := m.read(ctx, func() error {
		v, ok := m.access[key(tenantID, kind)]
		if !ok {
			return notFound("access config", tenantID, kind)
		}
		out = cloneBytes(v)
		return nil
	})
	return out, err
}

// Close implements domain.MetaStore. Subsequent calls fail with ErrClosed;
// closing twice is a no-op.
func (m *Memory) Close() error {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.closed = true
	return nil
}

// cloneBytes copies b, returning a non-nil slice.
func cloneBytes(b []byte) []byte {
	out := make([]byte, len(b))
	copy(out, b)
	return out
}

// newAuditID returns a time-ordered (v7) UUID, falling back to v4.
func newAuditID() string {
	if id, err := uuid.NewV7(); err == nil {
		return id.String()
	}
	return newID()
}

var (
	_ domain.MetaStore = (*Memory)(nil)
	_ SnapshotViewer   = (*Memory)(nil)
)
