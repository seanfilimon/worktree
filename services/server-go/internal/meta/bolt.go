package meta

import (
	"bytes"
	"context"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
	"strconv"
	"sync"
	"time"

	bolt "go.etcd.io/bbolt"
	bolterrors "go.etcd.io/bbolt/errors"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// schemaVersion is stored in the meta bucket; NewBolt refuses to open a
// database written with a different (future) schema.
const schemaVersion = 1

// Bucket names. Primary buckets hold JSON values; idx_* buckets map a
// unique or secondary key to a primary key. Composite keys join their
// components with a NUL byte (see key).
var (
	bktMeta             = []byte("meta")
	bktTenants          = []byte("tenants")           // id -> Tenant
	bktTenantSlug       = []byte("idx_tenant_slug")   // slug -> tenant id
	bktAccounts         = []byte("accounts")          // id -> accountRec
	bktAccountName      = []byte("idx_account_name")  // username -> account id
	bktMembers          = []byte("memberships")       // tenant\0account -> Membership
	bktMembersByAccount = []byte("idx_member_acct")   // account\0tenant -> tenant\0account
	bktTokens           = []byte("tokens")            // id -> tokenRec
	bktTokenHash        = []byte("idx_token_hash")    // token hash -> token id
	bktTokensByAccount  = []byte("idx_token_acct")    // account\0id -> token id
	bktTrees            = []byte("trees")             // id -> Tree
	bktTreeName         = []byte("idx_tree_name")     // tenant\0name -> tree id
	bktBranches         = []byte("branches")          // tree\0name -> Branch
	bktSnapshots        = []byte("snapshots")         // id -> Snapshot
	bktSnapsByTree      = []byte("idx_snapshot_tree") // tree\0id -> snapshot id
	bktCounters         = []byte("counters")          // counter name -> be64
	bktTags             = []byte("tags")              // tree\0name -> Tag
	bktStaged           = []byte("staged")            // tree\0account\0branch -> StagedSnapshot
	bktProtections      = []byte("protections")       // tree\0pattern -> BranchProtection
	bktMRs              = []byte("merge_requests")    // tree\0be64(number) -> MergeRequest
	bktMRSeq            = []byte("mr_seq")            // tree -> be64(last number)
	bktAudit            = []byte("audit")             // be64(at)+be64(seq) -> AuditEvent
	bktLicenses         = []byte("licenses")          // tree\0glob -> LicenseRule
	bktAccess           = []byte("access_config")     // tenant\0kind -> 'v' + content

	allBuckets = [][]byte{
		bktMeta, bktTenants, bktTenantSlug, bktAccounts, bktAccountName,
		bktMembers, bktMembersByAccount, bktTokens, bktTokenHash,
		bktTokensByAccount, bktTrees, bktTreeName, bktBranches, bktSnapshots,
		bktSnapsByTree, bktCounters, bktTags, bktStaged, bktProtections,
		bktMRs, bktMRSeq, bktAudit, bktLicenses, bktAccess,
	}

	// Per-tree MR, tag... buckets cleared by DeleteTree (keys prefixed by
	// tree id).
	perTreeBuckets = [][]byte{bktBranches, bktTags, bktStaged, bktProtections, bktMRs, bktLicenses}
)

const counterSnapshots = "snapshots"

// accountRec is the stored form of an Account. domain.Account hides
// PasswordHash from JSON, so it is persisted through a shadowing field.
type accountRec struct {
	domain.Account
	PasswordHash string `json:"password_hash"`
}

// tokenRec is the stored form of an APIToken (TokenHash is hidden from
// JSON in the domain type).
type tokenRec struct {
	domain.APIToken
	TokenHash string `json:"token_hash"`
}

// Bolt is a [domain.MetaStore] persisted in a single bbolt database file.
// It is safe for concurrent use: reads run in parallel read transactions
// and writes are serialized by bbolt, so every method is atomic.
type Bolt struct {
	db        *bolt.DB
	closeOnce sync.Once
	closeErr  error
}

// NewBolt opens (creating if needed, along with its parent directory) the
// database at path. The file is locked for exclusive use by this process;
// if another process holds it, NewBolt fails after a few seconds.
func NewBolt(path string) (*Bolt, error) {
	if dir := filepath.Dir(path); dir != "" {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return nil, fmt.Errorf("meta: create db dir: %w", err)
		}
	}
	db, err := bolt.Open(path, 0o600, &bolt.Options{
		Timeout:      5 * time.Second,
		FreelistType: bolt.FreelistMapType,
	})
	if err != nil {
		return nil, fmt.Errorf("meta: open %s: %w", path, err)
	}
	err = db.Update(func(tx *bolt.Tx) error {
		for _, name := range allBuckets {
			if _, err := tx.CreateBucketIfNotExists(name); err != nil {
				return fmt.Errorf("create bucket %s: %w", name, err)
			}
		}
		mb := tx.Bucket(bktMeta)
		if v := mb.Get([]byte("schema")); v != nil {
			got, err := strconv.Atoi(string(v))
			if err != nil || got != schemaVersion {
				return fmt.Errorf("unsupported schema version %q (want %d)", v, schemaVersion)
			}
			return nil
		}
		return mb.Put([]byte("schema"), []byte(strconv.Itoa(schemaVersion)))
	})
	if err != nil {
		_ = db.Close()
		return nil, fmt.Errorf("meta: init %s: %w", path, err)
	}
	return &Bolt{db: db}, nil
}

// Path returns the database file path.
func (b *Bolt) Path() string { return b.db.Path() }

// Close implements domain.MetaStore. Subsequent calls fail with ErrClosed;
// closing twice is a no-op.
func (b *Bolt) Close() error {
	b.closeOnce.Do(func() { b.closeErr = b.db.Close() })
	return b.closeErr
}

func mapBoltErr(err error) error {
	if errors.Is(err, bolterrors.ErrDatabaseNotOpen) {
		return ErrClosed
	}
	return err
}

func (b *Bolt) view(ctx context.Context, fn func(tx *bolt.Tx) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	return mapBoltErr(b.db.View(fn))
}

func (b *Bolt) update(ctx context.Context, fn func(tx *bolt.Tx) error) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	return mapBoltErr(b.db.Update(fn))
}

// ---- encoding helpers -------------------------------------------------

func getJSON(bk *bolt.Bucket, k []byte, v any) (bool, error) {
	raw := bk.Get(k)
	if raw == nil {
		return false, nil
	}
	if err := json.Unmarshal(raw, v); err != nil {
		return false, fmt.Errorf("meta: decode record %q: %w", k, err)
	}
	return true, nil
}

func putJSON(bk *bolt.Bucket, k []byte, v any) error {
	raw, err := json.Marshal(v)
	if err != nil {
		return fmt.Errorf("meta: encode: %w", err)
	}
	return bk.Put(k, raw)
}

// scanPrefix calls fn for each key/value with the given prefix, in key
// order. k and v are valid only during fn.
func scanPrefix(bk *bolt.Bucket, p []byte, fn func(k, v []byte) error) error {
	c := bk.Cursor()
	for k, v := c.Seek(p); k != nil && bytes.HasPrefix(k, p); k, v = c.Next() {
		if err := fn(k, v); err != nil {
			return err
		}
	}
	return nil
}

// deletePrefix removes every key with the given prefix and returns how
// many were removed. Keys are collected first because deleting under a
// live cursor can skip entries.
func deletePrefix(bk *bolt.Bucket, p []byte) (int, error) {
	var keys [][]byte
	_ = scanPrefix(bk, p, func(k, _ []byte) error {
		keys = append(keys, slices.Clone(k))
		return nil
	})
	for _, k := range keys {
		if err := bk.Delete(k); err != nil {
			return 0, err
		}
	}
	return len(keys), nil
}

func be64(n uint64) []byte {
	var b [8]byte
	binary.BigEndian.PutUint64(b[:], n)
	return b[:]
}

func getCounter(bk *bolt.Bucket, name string) uint64 {
	v := bk.Get([]byte(name))
	if len(v) != 8 {
		return 0
	}
	return binary.BigEndian.Uint64(v)
}

func addCounter(bk *bolt.Bucket, name string, delta int64) error {
	n := int64(getCounter(bk, name)) + delta
	if n <= 0 {
		return bk.Delete([]byte(name))
	}
	return bk.Put([]byte(name), be64(uint64(n)))
}

func mrKey(treeID string, number int) []byte {
	return append([]byte(prefix(treeID)), be64(uint64(number))...)
}

// auditKey orders events by time, then by insertion sequence. The sign
// bit is flipped so pre-1970 instants sort before post-1970 ones.
func auditKey(at time.Time, seq uint64) []byte {
	k := make([]byte, 16)
	binary.BigEndian.PutUint64(k[:8], uint64(at.UnixNano())^(1<<63))
	binary.BigEndian.PutUint64(k[8:], seq)
	return k
}

// ---- tenants ----------------------------------------------------------

// CreateTenant implements domain.MetaStore.
func (b *Bolt) CreateTenant(ctx context.Context, t *domain.Tenant) error {
	if t == nil {
		return errNil("tenant")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareTenant(t); err != nil {
			return err
		}
		tb, ib := tx.Bucket(bktTenants), tx.Bucket(bktTenantSlug)
		if tb.Get([]byte(t.ID)) != nil {
			return alreadyExists("tenant", t.ID)
		}
		if ib.Get([]byte(t.Slug)) != nil {
			return alreadyExists("tenant slug", t.Slug)
		}
		if err := ib.Put([]byte(t.Slug), []byte(t.ID)); err != nil {
			return err
		}
		return putJSON(tb, []byte(t.ID), normTenant(*t))
	})
}

func getTenantTx(tx *bolt.Tx, id string) (*domain.Tenant, error) {
	var t domain.Tenant
	ok, err := getJSON(tx.Bucket(bktTenants), []byte(id), &t)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, notFound("tenant", id)
	}
	return &t, nil
}

// GetTenant implements domain.MetaStore.
func (b *Bolt) GetTenant(ctx context.Context, id string) (*domain.Tenant, error) {
	var out *domain.Tenant
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getTenantTx(tx, id)
		return err
	})
	return out, err
}

// GetTenantBySlug implements domain.MetaStore.
func (b *Bolt) GetTenantBySlug(ctx context.Context, slug string) (*domain.Tenant, error) {
	var out *domain.Tenant
	err := b.view(ctx, func(tx *bolt.Tx) error {
		id := tx.Bucket(bktTenantSlug).Get([]byte(slug))
		if id == nil {
			return notFound("tenant slug", slug)
		}
		var err error
		out, err = getTenantTx(tx, string(id))
		return err
	})
	return out, err
}

// ListTenants implements domain.MetaStore (ordered by slug).
func (b *Bolt) ListTenants(ctx context.Context) ([]domain.Tenant, error) {
	out := []domain.Tenant{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return tx.Bucket(bktTenantSlug).ForEach(func(_, id []byte) error {
			t, err := getTenantTx(tx, string(id))
			if err != nil {
				return err
			}
			out = append(out, *t)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// UpdateTenant implements domain.MetaStore.
func (b *Bolt) UpdateTenant(ctx context.Context, t *domain.Tenant) error {
	if t == nil {
		return errNil("tenant")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tenant id", t.ID, "tenant slug", t.Slug); err != nil {
			return err
		}
		old, err := getTenantTx(tx, t.ID)
		if err != nil {
			return err
		}
		ib := tx.Bucket(bktTenantSlug)
		if owner := ib.Get([]byte(t.Slug)); owner != nil && string(owner) != t.ID {
			return alreadyExists("tenant slug", t.Slug)
		}
		if t.CreatedAt.IsZero() {
			t.CreatedAt = old.CreatedAt
		}
		if err := ib.Delete([]byte(old.Slug)); err != nil {
			return err
		}
		if err := ib.Put([]byte(t.Slug), []byte(t.ID)); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktTenants), []byte(t.ID), normTenant(*t))
	})
}

// ---- accounts ---------------------------------------------------------

// CreateAccount implements domain.MetaStore.
func (b *Bolt) CreateAccount(ctx context.Context, a *domain.Account) error {
	if a == nil {
		return errNil("account")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareAccount(a); err != nil {
			return err
		}
		ab, ib := tx.Bucket(bktAccounts), tx.Bucket(bktAccountName)
		if ab.Get([]byte(a.ID)) != nil {
			return alreadyExists("account", a.ID)
		}
		if ib.Get([]byte(a.Username)) != nil {
			return alreadyExists("account username", a.Username)
		}
		if err := ib.Put([]byte(a.Username), []byte(a.ID)); err != nil {
			return err
		}
		n := normAccount(*a)
		return putJSON(ab, []byte(a.ID), accountRec{Account: n, PasswordHash: n.PasswordHash})
	})
}

func getAccountTx(tx *bolt.Tx, id string) (*domain.Account, error) {
	var rec accountRec
	ok, err := getJSON(tx.Bucket(bktAccounts), []byte(id), &rec)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, notFound("account", id)
	}
	a := rec.Account
	a.PasswordHash = rec.PasswordHash
	return &a, nil
}

// GetAccount implements domain.MetaStore.
func (b *Bolt) GetAccount(ctx context.Context, id string) (*domain.Account, error) {
	var out *domain.Account
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getAccountTx(tx, id)
		return err
	})
	return out, err
}

// GetAccountByUsername implements domain.MetaStore.
func (b *Bolt) GetAccountByUsername(ctx context.Context, username string) (*domain.Account, error) {
	var out *domain.Account
	err := b.view(ctx, func(tx *bolt.Tx) error {
		id := tx.Bucket(bktAccountName).Get([]byte(username))
		if id == nil {
			return notFound("account username", username)
		}
		var err error
		out, err = getAccountTx(tx, string(id))
		return err
	})
	return out, err
}

// ListAccounts implements domain.MetaStore (ordered by username).
func (b *Bolt) ListAccounts(ctx context.Context) ([]domain.Account, error) {
	out := []domain.Account{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return tx.Bucket(bktAccountName).ForEach(func(_, id []byte) error {
			a, err := getAccountTx(tx, string(id))
			if err != nil {
				return err
			}
			out = append(out, *a)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// UpdateAccount implements domain.MetaStore.
func (b *Bolt) UpdateAccount(ctx context.Context, a *domain.Account) error {
	if a == nil {
		return errNil("account")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("account id", a.ID, "account username", a.Username); err != nil {
			return err
		}
		old, err := getAccountTx(tx, a.ID)
		if err != nil {
			return err
		}
		ib := tx.Bucket(bktAccountName)
		if owner := ib.Get([]byte(a.Username)); owner != nil && string(owner) != a.ID {
			return alreadyExists("account username", a.Username)
		}
		if a.CreatedAt.IsZero() {
			a.CreatedAt = old.CreatedAt
		}
		if err := ib.Delete([]byte(old.Username)); err != nil {
			return err
		}
		if err := ib.Put([]byte(a.Username), []byte(a.ID)); err != nil {
			return err
		}
		n := normAccount(*a)
		return putJSON(tx.Bucket(bktAccounts), []byte(a.ID), accountRec{Account: n, PasswordHash: n.PasswordHash})
	})
}

// ---- memberships ------------------------------------------------------

// PutMembership implements domain.MetaStore (insert or replace).
func (b *Bolt) PutMembership(ctx context.Context, m *domain.Membership) error {
	if m == nil {
		return errNil("membership")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tenant id", m.TenantID, "account id", m.AccountID); err != nil {
			return err
		}
		mb := tx.Bucket(bktMembers)
		k := []byte(key(m.TenantID, m.AccountID))
		if m.CreatedAt.IsZero() {
			var old domain.Membership
			ok, err := getJSON(mb, k, &old)
			if err != nil {
				return err
			}
			if ok {
				m.CreatedAt = old.CreatedAt
			} else {
				m.CreatedAt = now()
			}
		}
		if err := tx.Bucket(bktMembersByAccount).Put([]byte(key(m.AccountID, m.TenantID)), k); err != nil {
			return err
		}
		return putJSON(mb, k, normMembership(*m))
	})
}

// GetMembership implements domain.MetaStore.
func (b *Bolt) GetMembership(ctx context.Context, tenantID, accountID string) (*domain.Membership, error) {
	var out domain.Membership
	err := b.view(ctx, func(tx *bolt.Tx) error {
		ok, err := getJSON(tx.Bucket(bktMembers), []byte(key(tenantID, accountID)), &out)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("membership", tenantID, accountID)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTenantMembers implements domain.MetaStore (ordered by account id).
func (b *Bolt) ListTenantMembers(ctx context.Context, tenantID string) ([]domain.Membership, error) {
	out := []domain.Membership{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return scanPrefix(tx.Bucket(bktMembers), []byte(prefix(tenantID)), func(_, v []byte) error {
			var m domain.Membership
			if err := json.Unmarshal(v, &m); err != nil {
				return fmt.Errorf("meta: decode membership: %w", err)
			}
			out = append(out, m)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// ListAccountMemberships implements domain.MetaStore (ordered by tenant id).
func (b *Bolt) ListAccountMemberships(ctx context.Context, accountID string) ([]domain.Membership, error) {
	out := []domain.Membership{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		mb := tx.Bucket(bktMembers)
		return scanPrefix(tx.Bucket(bktMembersByAccount), []byte(prefix(accountID)), func(_, pk []byte) error {
			var m domain.Membership
			ok, err := getJSON(mb, pk, &m)
			if err != nil {
				return err
			}
			if ok {
				out = append(out, m)
			}
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// DeleteMembership implements domain.MetaStore.
func (b *Bolt) DeleteMembership(ctx context.Context, tenantID, accountID string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		mb := tx.Bucket(bktMembers)
		k := []byte(key(tenantID, accountID))
		if mb.Get(k) == nil {
			return notFound("membership", tenantID, accountID)
		}
		if err := tx.Bucket(bktMembersByAccount).Delete([]byte(key(accountID, tenantID))); err != nil {
			return err
		}
		return mb.Delete(k)
	})
}

// ---- tokens -----------------------------------------------------------

func putTokenTx(tx *bolt.Tx, t domain.APIToken) error {
	n := normToken(t)
	return putJSON(tx.Bucket(bktTokens), []byte(n.ID), tokenRec{APIToken: n, TokenHash: n.TokenHash})
}

func getTokenTx(tx *bolt.Tx, id string) (*domain.APIToken, error) {
	var rec tokenRec
	ok, err := getJSON(tx.Bucket(bktTokens), []byte(id), &rec)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, notFound("token", id)
	}
	t := rec.APIToken
	t.TokenHash = rec.TokenHash
	return &t, nil
}

// CreateToken implements domain.MetaStore.
func (b *Bolt) CreateToken(ctx context.Context, t *domain.APIToken) error {
	if t == nil {
		return errNil("token")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareToken(t); err != nil {
			return err
		}
		if tx.Bucket(bktTokens).Get([]byte(t.ID)) != nil {
			return alreadyExists("token", t.ID)
		}
		hb := tx.Bucket(bktTokenHash)
		if hb.Get([]byte(t.TokenHash)) != nil {
			return alreadyExists("token hash", "")
		}
		if err := hb.Put([]byte(t.TokenHash), []byte(t.ID)); err != nil {
			return err
		}
		if err := tx.Bucket(bktTokensByAccount).Put([]byte(key(t.AccountID, t.ID)), []byte(t.ID)); err != nil {
			return err
		}
		return putTokenTx(tx, *t)
	})
}

// GetTokenByHash implements domain.MetaStore.
func (b *Bolt) GetTokenByHash(ctx context.Context, tokenHash string) (*domain.APIToken, error) {
	var out *domain.APIToken
	err := b.view(ctx, func(tx *bolt.Tx) error {
		id := tx.Bucket(bktTokenHash).Get([]byte(tokenHash))
		if id == nil {
			return notFound("token hash", "")
		}
		var err error
		out, err = getTokenTx(tx, string(id))
		return err
	})
	return out, err
}

// ListTokens implements domain.MetaStore (ordered by creation time).
func (b *Bolt) ListTokens(ctx context.Context, accountID string) ([]domain.APIToken, error) {
	out := []domain.APIToken{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return scanPrefix(tx.Bucket(bktTokensByAccount), []byte(prefix(accountID)), func(_, id []byte) error {
			t, err := getTokenTx(tx, string(id))
			if err != nil {
				return err
			}
			out = append(out, *t)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	sortTokens(out)
	return out, nil
}

// DeleteToken implements domain.MetaStore.
func (b *Bolt) DeleteToken(ctx context.Context, id string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		t, err := getTokenTx(tx, id)
		if err != nil {
			return err
		}
		if err := tx.Bucket(bktTokenHash).Delete([]byte(t.TokenHash)); err != nil {
			return err
		}
		if err := tx.Bucket(bktTokensByAccount).Delete([]byte(key(t.AccountID, t.ID))); err != nil {
			return err
		}
		return tx.Bucket(bktTokens).Delete([]byte(id))
	})
}

// TouchToken implements domain.MetaStore.
func (b *Bolt) TouchToken(ctx context.Context, id string, at time.Time) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		t, err := getTokenTx(tx, id)
		if err != nil {
			return err
		}
		at = utc(at)
		t.LastUsedAt = &at
		return putTokenTx(tx, *t)
	})
}

// ---- trees ------------------------------------------------------------

func getTreeTx(tx *bolt.Tx, id string) (*domain.Tree, error) {
	var t domain.Tree
	ok, err := getJSON(tx.Bucket(bktTrees), []byte(id), &t)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, notFound("tree", id)
	}
	return &t, nil
}

// CreateTree implements domain.MetaStore.
func (b *Bolt) CreateTree(ctx context.Context, t *domain.Tree) error {
	if t == nil {
		return errNil("tree")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareTree(t); err != nil {
			return err
		}
		tb, ib := tx.Bucket(bktTrees), tx.Bucket(bktTreeName)
		if tb.Get([]byte(t.ID)) != nil {
			return alreadyExists("tree", t.ID)
		}
		nk := []byte(key(t.TenantID, t.Name))
		if ib.Get(nk) != nil {
			return alreadyExists("tree", t.TenantID, t.Name)
		}
		if err := ib.Put(nk, []byte(t.ID)); err != nil {
			return err
		}
		return putJSON(tb, []byte(t.ID), normTree(*t))
	})
}

// GetTree implements domain.MetaStore.
func (b *Bolt) GetTree(ctx context.Context, id string) (*domain.Tree, error) {
	var out *domain.Tree
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getTreeTx(tx, id)
		return err
	})
	return out, err
}

// GetTreeByName implements domain.MetaStore.
func (b *Bolt) GetTreeByName(ctx context.Context, tenantID, name string) (*domain.Tree, error) {
	var out *domain.Tree
	err := b.view(ctx, func(tx *bolt.Tx) error {
		id := tx.Bucket(bktTreeName).Get([]byte(key(tenantID, name)))
		if id == nil {
			return notFound("tree", tenantID, name)
		}
		var err error
		out, err = getTreeTx(tx, string(id))
		return err
	})
	return out, err
}

// ListTrees implements domain.MetaStore (ordered by name, then tenant id;
// tenantID "" lists all tenants).
func (b *Bolt) ListTrees(ctx context.Context, tenantID string) ([]domain.Tree, error) {
	out := []domain.Tree{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		if tenantID == "" {
			return tx.Bucket(bktTrees).ForEach(func(_, v []byte) error {
				var t domain.Tree
				if err := json.Unmarshal(v, &t); err != nil {
					return fmt.Errorf("meta: decode tree: %w", err)
				}
				out = append(out, t)
				return nil
			})
		}
		return scanPrefix(tx.Bucket(bktTreeName), []byte(prefix(tenantID)), func(_, id []byte) error {
			t, err := getTreeTx(tx, string(id))
			if err != nil {
				return err
			}
			out = append(out, *t)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	sortTrees(out)
	return out, nil
}

// UpdateTree implements domain.MetaStore. UpdatedAt is set to now.
func (b *Bolt) UpdateTree(ctx context.Context, t *domain.Tree) error {
	if t == nil {
		return errNil("tree")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tree id", t.ID, "tenant id", t.TenantID, "tree name", t.Name); err != nil {
			return err
		}
		old, err := getTreeTx(tx, t.ID)
		if err != nil {
			return err
		}
		ib := tx.Bucket(bktTreeName)
		nk := []byte(key(t.TenantID, t.Name))
		if owner := ib.Get(nk); owner != nil && string(owner) != t.ID {
			return alreadyExists("tree", t.TenantID, t.Name)
		}
		if t.CreatedAt.IsZero() {
			t.CreatedAt = old.CreatedAt
		}
		t.UpdatedAt = now()
		if err := ib.Delete([]byte(key(old.TenantID, old.Name))); err != nil {
			return err
		}
		if err := ib.Put(nk, []byte(t.ID)); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktTrees), []byte(t.ID), normTree(*t))
	})
}

// DeleteTree implements domain.MetaStore. It cascades to every per-tree
// record: branches, snapshots, tags, staged snapshots, protections, merge
// requests (and the MR counter) and license rules.
func (b *Bolt) DeleteTree(ctx context.Context, id string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		t, err := getTreeTx(tx, id)
		if err != nil {
			return err
		}
		p := []byte(prefix(id))
		for _, name := range perTreeBuckets {
			if _, err := deletePrefix(tx.Bucket(name), p); err != nil {
				return err
			}
		}
		// Snapshots: walk the per-tree index.
		sb, xb := tx.Bucket(bktSnapshots), tx.Bucket(bktSnapsByTree)
		var snapIDs [][]byte
		_ = scanPrefix(xb, p, func(_, sid []byte) error {
			snapIDs = append(snapIDs, slices.Clone(sid))
			return nil
		})
		for _, sid := range snapIDs {
			if err := sb.Delete(sid); err != nil {
				return err
			}
		}
		if _, err := deletePrefix(xb, p); err != nil {
			return err
		}
		cb := tx.Bucket(bktCounters)
		if err := addCounter(cb, counterSnapshots, -int64(len(snapIDs))); err != nil {
			return err
		}
		if err := cb.Delete([]byte(key(counterSnapshots, id))); err != nil {
			return err
		}
		if err := tx.Bucket(bktMRSeq).Delete([]byte(id)); err != nil {
			return err
		}
		if err := tx.Bucket(bktTreeName).Delete([]byte(key(t.TenantID, t.Name))); err != nil {
			return err
		}
		return tx.Bucket(bktTrees).Delete([]byte(id))
	})
}

// ---- branches ---------------------------------------------------------

func getBranchTx(tx *bolt.Tx, treeID, name string) (domain.Branch, bool, error) {
	var br domain.Branch
	ok, err := getJSON(tx.Bucket(bktBranches), []byte(key(treeID, name)), &br)
	return br, ok, err
}

// GetBranch implements domain.MetaStore.
func (b *Bolt) GetBranch(ctx context.Context, treeID, name string) (*domain.Branch, error) {
	var out domain.Branch
	err := b.view(ctx, func(tx *bolt.Tx) error {
		br, ok, err := getBranchTx(tx, treeID, name)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("branch", treeID, name)
		}
		out = br
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListBranches implements domain.MetaStore (ordered by name).
func (b *Bolt) ListBranches(ctx context.Context, treeID string) ([]domain.Branch, error) {
	return listPrefix[domain.Branch](ctx, b, bktBranches, treeID)
}

// listPrefix decodes every value in bucket whose key starts with
// first\0, in key order.
func listPrefix[T any](ctx context.Context, b *Bolt, bucket []byte, first string) ([]T, error) {
	out := []T{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return scanPrefix(tx.Bucket(bucket), []byte(prefix(first)), func(_, v []byte) error {
			var item T
			if err := json.Unmarshal(v, &item); err != nil {
				return fmt.Errorf("meta: decode %s: %w", bucket, err)
			}
			out = append(out, item)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// PutBranch implements domain.MetaStore (insert or replace). UpdatedAt is
// set to now.
func (b *Bolt) PutBranch(ctx context.Context, br *domain.Branch) error {
	if br == nil {
		return errNil("branch")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tree id", br.TreeID, "branch name", br.Name); err != nil {
			return err
		}
		old, exists, err := getBranchTx(tx, br.TreeID, br.Name)
		if err != nil {
			return err
		}
		prepareBranchPut(br, old, exists)
		return putJSON(tx.Bucket(bktBranches), []byte(key(br.TreeID, br.Name)), normBranch(*br))
	})
}

// CompareAndSwapBranchTip implements domain.MetaStore. The read, compare
// and write happen in one bbolt update transaction.
func (b *Bolt) CompareAndSwapBranchTip(ctx context.Context, treeID, name, oldTip, newTip string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tree id", treeID, "branch name", name); err != nil {
			return err
		}
		cur, exists, err := getBranchTx(tx, treeID, name)
		if err != nil {
			return err
		}
		next, err := casBranch(treeID, name, cur, exists, oldTip, newTip)
		if err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktBranches), []byte(key(treeID, name)), next)
	})
}

// DeleteBranch implements domain.MetaStore.
func (b *Bolt) DeleteBranch(ctx context.Context, treeID, name string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktBranches)
		k := []byte(key(treeID, name))
		if bk.Get(k) == nil {
			return notFound("branch", treeID, name)
		}
		return bk.Delete(k)
	})
}

// ---- snapshots --------------------------------------------------------

func getSnapshotTx(tx *bolt.Tx, id string) (*domain.Snapshot, error) {
	var s domain.Snapshot
	ok, err := getJSON(tx.Bucket(bktSnapshots), []byte(id), &s)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, notFound("snapshot", id)
	}
	if s.Parents == nil {
		s.Parents = []string{}
	}
	return &s, nil
}

// PutSnapshot implements domain.MetaStore.
func (b *Bolt) PutSnapshot(ctx context.Context, s *domain.Snapshot) error {
	if s == nil {
		return errNil("snapshot")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareSnapshot(s); err != nil {
			return err
		}
		ns := normSnapshot(*s)
		sb := tx.Bucket(bktSnapshots)
		if sb.Get([]byte(s.ID)) != nil {
			old, err := getSnapshotTx(tx, s.ID)
			if err != nil {
				return err
			}
			if snapshotContentEqual(old, &ns) {
				return nil
			}
			return fmt.Errorf("meta: snapshot %q exists with different content: %w", s.ID, domain.ErrConflict)
		}
		if err := putJSON(sb, []byte(s.ID), ns); err != nil {
			return err
		}
		if err := tx.Bucket(bktSnapsByTree).Put([]byte(key(s.TreeID, s.ID)), []byte(s.ID)); err != nil {
			return err
		}
		cb := tx.Bucket(bktCounters)
		if err := addCounter(cb, counterSnapshots, 1); err != nil {
			return err
		}
		return addCounter(cb, key(counterSnapshots, s.TreeID), 1)
	})
}

// GetSnapshot implements domain.MetaStore.
func (b *Bolt) GetSnapshot(ctx context.Context, id string) (*domain.Snapshot, error) {
	var out *domain.Snapshot
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getSnapshotTx(tx, id)
		return err
	})
	return out, err
}

// ViewSnapshots implements [SnapshotViewer]: all lookups made through get
// are served from a single read transaction.
func (b *Bolt) ViewSnapshots(ctx context.Context, fn func(get func(id string) (*domain.Snapshot, error)) error) error {
	return b.view(ctx, func(tx *bolt.Tx) error {
		return fn(func(id string) (*domain.Snapshot, error) { return getSnapshotTx(tx, id) })
	})
}

// HasSnapshot implements domain.MetaStore.
func (b *Bolt) HasSnapshot(ctx context.Context, id string) (bool, error) {
	var ok bool
	err := b.view(ctx, func(tx *bolt.Tx) error {
		ok = id != "" && tx.Bucket(bktSnapshots).Get([]byte(id)) != nil
		return nil
	})
	return ok, err
}

// CountSnapshots implements domain.MetaStore ("" counts all trees). It
// reads a maintained counter, so it is O(1).
func (b *Bolt) CountSnapshots(ctx context.Context, treeID string) (int, error) {
	var n uint64
	err := b.view(ctx, func(tx *bolt.Tx) error {
		name := counterSnapshots
		if treeID != "" {
			name = key(counterSnapshots, treeID)
		}
		n = getCounter(tx.Bucket(bktCounters), name)
		return nil
	})
	return int(n), err
}

// ---- tags -------------------------------------------------------------

// PutTag implements domain.MetaStore.
func (b *Bolt) PutTag(ctx context.Context, t *domain.Tag) error {
	if t == nil {
		return errNil("tag")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareTag(t); err != nil {
			return err
		}
		bk := tx.Bucket(bktTags)
		k := []byte(key(t.TreeID, t.Name))
		if bk.Get(k) != nil {
			return alreadyExists("tag", t.TreeID, t.Name)
		}
		return putJSON(bk, k, normTag(*t))
	})
}

// GetTag implements domain.MetaStore.
func (b *Bolt) GetTag(ctx context.Context, treeID, name string) (*domain.Tag, error) {
	var out domain.Tag
	err := b.view(ctx, func(tx *bolt.Tx) error {
		ok, err := getJSON(tx.Bucket(bktTags), []byte(key(treeID, name)), &out)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("tag", treeID, name)
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTags implements domain.MetaStore (ordered by name).
func (b *Bolt) ListTags(ctx context.Context, treeID string) ([]domain.Tag, error) {
	return listPrefix[domain.Tag](ctx, b, bktTags, treeID)
}

// DeleteTag implements domain.MetaStore.
func (b *Bolt) DeleteTag(ctx context.Context, treeID, name string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktTags)
		k := []byte(key(treeID, name))
		if bk.Get(k) == nil {
			return notFound("tag", treeID, name)
		}
		return bk.Delete(k)
	})
}

// ---- staged -----------------------------------------------------------

// PutStaged implements domain.MetaStore (replaces the entry for the same
// tree, account and branch).
func (b *Bolt) PutStaged(ctx context.Context, s *domain.StagedSnapshot) error {
	if s == nil {
		return errNil("staged snapshot")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareStaged(s); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktStaged), []byte(key(s.TreeID, s.AccountID, s.BranchName)), normStaged(*s))
	})
}

// ListStaged implements domain.MetaStore (newest staged_at first; branch
// "" lists all branches).
func (b *Bolt) ListStaged(ctx context.Context, treeID, branch string) ([]domain.StagedSnapshot, error) {
	all, err := listPrefix[domain.StagedSnapshot](ctx, b, bktStaged, treeID)
	if err != nil {
		return nil, err
	}
	out := all[:0]
	for _, s := range all {
		if branch == "" || s.BranchName == branch {
			if s.FilesChanged == nil {
				s.FilesChanged = []string{}
			}
			out = append(out, s)
		}
	}
	sortStaged(out)
	return out, nil
}

// DeleteStaged implements domain.MetaStore. Deleting a missing entry is a
// no-op.
func (b *Bolt) DeleteStaged(ctx context.Context, treeID, accountID, branch string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		return tx.Bucket(bktStaged).Delete([]byte(key(treeID, accountID, branch)))
	})
}

// CountStaged implements domain.MetaStore.
func (b *Bolt) CountStaged(ctx context.Context) (int, error) {
	var n int
	err := b.view(ctx, func(tx *bolt.Tx) error {
		n = tx.Bucket(bktStaged).Stats().KeyN
		return nil
	})
	return n, err
}

// ---- protections ------------------------------------------------------

// PutProtection implements domain.MetaStore (insert or replace).
func (b *Bolt) PutProtection(ctx context.Context, p *domain.BranchProtection) error {
	if p == nil {
		return errNil("branch protection")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tree id", p.TreeID, "pattern", p.Pattern); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktProtections), []byte(key(p.TreeID, p.Pattern)), normProtection(*p))
	})
}

// ListProtections implements domain.MetaStore (ordered by pattern).
func (b *Bolt) ListProtections(ctx context.Context, treeID string) ([]domain.BranchProtection, error) {
	out, err := listPrefix[domain.BranchProtection](ctx, b, bktProtections, treeID)
	if err != nil {
		return nil, err
	}
	for i := range out {
		if out[i].AllowedPushers == nil {
			out[i].AllowedPushers = []string{}
		}
	}
	return out, nil
}

// DeleteProtection implements domain.MetaStore.
func (b *Bolt) DeleteProtection(ctx context.Context, treeID, pattern string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktProtections)
		k := []byte(key(treeID, pattern))
		if bk.Get(k) == nil {
			return notFound("branch protection", treeID, pattern)
		}
		return bk.Delete(k)
	})
}

// ---- merge requests ---------------------------------------------------

// CreateMergeRequest implements domain.MetaStore. The per-tree number is
// allocated from a persisted counter in the same transaction.
func (b *Bolt) CreateMergeRequest(ctx context.Context, mr *domain.MergeRequest) error {
	if mr == nil {
		return errNil("merge request")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKey("tree id", mr.TreeID); err != nil {
			return err
		}
		seq := tx.Bucket(bktMRSeq)
		var last uint64
		if v := seq.Get([]byte(mr.TreeID)); len(v) == 8 {
			last = binary.BigEndian.Uint64(v)
		}
		n := int(last) + 1
		prepareMR(mr, n)
		if err := seq.Put([]byte(mr.TreeID), be64(uint64(n))); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktMRs), mrKey(mr.TreeID, n), normMR(*mr))
	})
}

func getMRTx(tx *bolt.Tx, treeID string, number int) (*domain.MergeRequest, error) {
	var mr domain.MergeRequest
	ok := false
	var err error
	if number > 0 {
		ok, err = getJSON(tx.Bucket(bktMRs), mrKey(treeID, number), &mr)
	}
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, notFound("merge request", treeID, strconv.Itoa(number))
	}
	fixMR(&mr)
	return &mr, nil
}

func fixMR(mr *domain.MergeRequest) {
	if mr.Approvals == nil {
		mr.Approvals = []domain.Approval{}
	}
}

// GetMergeRequest implements domain.MetaStore.
func (b *Bolt) GetMergeRequest(ctx context.Context, treeID string, number int) (*domain.MergeRequest, error) {
	var out *domain.MergeRequest
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getMRTx(tx, treeID, number)
		return err
	})
	return out, err
}

// ListMergeRequests implements domain.MetaStore (number descending; state
// "" lists all states).
func (b *Bolt) ListMergeRequests(ctx context.Context, treeID, state string) ([]domain.MergeRequest, error) {
	out := []domain.MergeRequest{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		p := []byte(prefix(treeID))
		c := tx.Bucket(bktMRs).Cursor()
		// Walk backwards from the end of this tree's key range: the
		// big-endian number suffix makes key order numeric order.
		upper := append(slices.Clone(p), 0xff)
		k, v := c.Seek(upper)
		if k == nil {
			k, v = c.Last()
		} else {
			k, v = c.Prev()
		}
		for ; k != nil && bytes.HasPrefix(k, p); k, v = c.Prev() {
			var mr domain.MergeRequest
			if err := json.Unmarshal(v, &mr); err != nil {
				return fmt.Errorf("meta: decode merge request: %w", err)
			}
			if state == "" || mr.State == state {
				fixMR(&mr)
				out = append(out, mr)
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// UpdateMergeRequest implements domain.MetaStore. The record is located by
// (TreeID, Number); UpdatedAt is set to now.
func (b *Bolt) UpdateMergeRequest(ctx context.Context, mr *domain.MergeRequest) error {
	if mr == nil {
		return errNil("merge request")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKey("tree id", mr.TreeID); err != nil {
			return err
		}
		old, err := getMRTx(tx, mr.TreeID, mr.Number)
		if err != nil {
			return err
		}
		if err := checkMRUpdate(mr, old); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktMRs), mrKey(mr.TreeID, mr.Number), normMR(*mr))
	})
}

// ---- audit ------------------------------------------------------------

// AppendAudit implements domain.MetaStore. Events are keyed by
// (At, insertion sequence), so ListAudit reads newest-first with a reverse
// cursor scan and stops as soon as the filter's Since bound is crossed.
func (b *Bolt) AppendAudit(ctx context.Context, e *domain.AuditEvent) error {
	if e == nil {
		return errNil("audit event")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		prepareAudit(e)
		bk := tx.Bucket(bktAudit)
		seq, err := bk.NextSequence()
		if err != nil {
			return err
		}
		ne := normAudit(*e)
		return putJSON(bk, auditKey(ne.At, seq), ne)
	})
}

// ListAudit implements domain.MetaStore (newest first).
func (b *Bolt) ListAudit(ctx context.Context, f domain.AuditFilter) ([]domain.AuditEvent, error) {
	out := []domain.AuditEvent{}
	limit := auditLimit(&f)
	var sinceKey []byte
	if !f.Since.IsZero() {
		sinceKey = auditKey(f.Since, 0)
	}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		c := tx.Bucket(bktAudit).Cursor()
		for k, v := c.Last(); k != nil && len(out) < limit; k, v = c.Prev() {
			if sinceKey != nil && bytes.Compare(k, sinceKey) < 0 {
				break
			}
			var e domain.AuditEvent
			if err := json.Unmarshal(v, &e); err != nil {
				return fmt.Errorf("meta: decode audit event: %w", err)
			}
			if auditMatches(&f, &e) {
				out = append(out, e)
			}
		}
		return nil
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// ---- license rules ----------------------------------------------------

// PutLicenseRule implements domain.MetaStore (insert or replace).
func (b *Bolt) PutLicenseRule(ctx context.Context, r *domain.LicenseRule) error {
	if r == nil {
		return errNil("license rule")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tree id", r.TreeID, "path glob", r.PathGlob); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktLicenses), []byte(key(r.TreeID, r.PathGlob)), *r)
	})
}

// ListLicenseRules implements domain.MetaStore (ordered by path glob).
func (b *Bolt) ListLicenseRules(ctx context.Context, treeID string) ([]domain.LicenseRule, error) {
	return listPrefix[domain.LicenseRule](ctx, b, bktLicenses, treeID)
}

// DeleteLicenseRule implements domain.MetaStore.
func (b *Bolt) DeleteLicenseRule(ctx context.Context, treeID, pathGlob string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktLicenses)
		k := []byte(key(treeID, pathGlob))
		if bk.Get(k) == nil {
			return notFound("license rule", treeID, pathGlob)
		}
		return bk.Delete(k)
	})
}

// ---- access config ----------------------------------------------------

// PutAccessConfig implements domain.MetaStore (insert or replace).
func (b *Bolt) PutAccessConfig(ctx context.Context, tenantID, kind string, content []byte) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tenant id", tenantID, "kind", kind); err != nil {
			return err
		}
		// A one-byte prefix keeps empty documents distinguishable from
		// missing keys.
		v := make([]byte, 1+len(content))
		v[0] = 'v'
		copy(v[1:], content)
		return tx.Bucket(bktAccess).Put([]byte(key(tenantID, kind)), v)
	})
}

// GetAccessConfig implements domain.MetaStore.
func (b *Bolt) GetAccessConfig(ctx context.Context, tenantID, kind string) ([]byte, error) {
	var out []byte
	err := b.view(ctx, func(tx *bolt.Tx) error {
		v := tx.Bucket(bktAccess).Get([]byte(key(tenantID, kind)))
		if len(v) == 0 {
			return notFound("access config", tenantID, kind)
		}
		out = cloneBytes(v[1:])
		return nil
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

var (
	_ domain.MetaStore = (*Bolt)(nil)
	_ SnapshotViewer   = (*Bolt)(nil)
)
