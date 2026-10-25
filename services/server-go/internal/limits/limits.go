// Package limits enforces per-tenant quotas (as a push check and from the
// REST create handlers) and per-principal request rate limits
// (docs/ENGINEERING_PLAN.md §13.3).
//
// Effective limits are the tenant's own Limits with every zero field
// replaced by the server default (itself 0 = unlimited unless configured).
//
// Storage accounting: the tenant_usage document ({bytes}) of a tenant is
// increased by the size of the objects each accepted push adds on top of
// what its parent snapshots already reference. The push checker computes
// that delta before the push is stored (and rejects pushes that would
// exceed MaxStorageBytes); the delta is applied when the push is published
// as accepted (EventBranchPushed). Garbage collection recomputes the exact
// value from reachability on every run (see SetStorageBytes).
package limits

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"sync"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// CollUsage is the doc collection holding per-tenant usage (key tenant id).
const CollUsage = "tenant_usage"

// Limit names (PushDenied.Reason, QuotaError.Limit).
const (
	LimitTrees    = "max_trees"
	LimitStorage  = "max_storage_bytes"
	LimitBranches = "max_branches_per_tree"
	LimitMembers  = "max_members"
)

// pendingTTL bounds how long a computed push delta waits for its push to
// be accepted.
const pendingTTL = 10 * time.Minute

// ErrQuotaExceeded is wrapped by *QuotaError.
var ErrQuotaExceeded = errors.New("quota exceeded")

// QuotaError reports a tenant quota that an operation would exceed.
type QuotaError struct {
	Limit string // Limit* name
	Max   int64
}

func (e *QuotaError) Error() string {
	return fmt.Sprintf("quota exceeded: %s (limit %d)", e.Limit, e.Max)
}

func (e *QuotaError) Unwrap() error { return ErrQuotaExceeded }

// UsageDoc is the tenant_usage document.
type UsageDoc struct {
	Bytes     int64     `json:"bytes"`
	UpdatedAt time.Time `json:"updated_at,omitempty"`
}

// Usage is the answer of GET /tenants/{t}/usage.
type Usage struct {
	Trees        int                 `json:"trees"`
	Branches     int                 `json:"branches"`
	Members      int                 `json:"members"`
	StorageBytes int64               `json:"storage_bytes"`
	Limits       domain.TenantLimits `json:"limits"`
	Effective    domain.TenantLimits `json:"effective"`
}

// Deps are the collaborators of the limits package. Store is required;
// Objects is needed for storage accounting; the others may be nil.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger

	// Defaults are the server-wide tenant limits (zero fields = unlimited).
	Defaults domain.TenantLimits
	// RateLimit is the default request budget per minute per principal or
	// anonymous IP (0 disables rate limiting). Tenant RequestsPerMinute
	// overrides it for principals of that tenant.
	RateLimit int
	// Now is the clock (nil = time.Now).
	Now func() time.Time
}

// Limiter is the limits component. It implements domain.PushChecker.
type Limiter struct {
	d Deps

	mu      sync.Mutex
	pending map[string]pendingDelta

	rate *rateLimiter
}

type pendingDelta struct {
	tenantID string
	bytes    int64
	at       time.Time
}

var _ domain.PushChecker = (*Limiter)(nil)

// New returns a Limiter.
func New(d Deps) *Limiter {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	if d.Now == nil {
		d.Now = time.Now
	}
	l := &Limiter{d: d, pending: map[string]pendingDelta{}}
	l.rate = newRateLimiter(d.Now)
	return l
}

// Defaults returns the server-wide default limits.
func (l *Limiter) Defaults() domain.TenantLimits { return l.d.Defaults }

// RateLimit returns the server default requests per minute (0 = off).
func (l *Limiter) RateLimit() int { return l.d.RateLimit }

// Effective merges the tenant's limits over the server defaults.
func (l *Limiter) Effective(t *domain.Tenant) domain.TenantLimits {
	eff := l.d.Defaults
	if eff.RequestsPerMinute == 0 {
		eff.RequestsPerMinute = l.d.RateLimit
	}
	if t == nil || t.Limits == nil {
		return eff
	}
	tl := *t.Limits
	if tl.MaxTrees > 0 {
		eff.MaxTrees = tl.MaxTrees
	}
	if tl.MaxStorageBytes > 0 {
		eff.MaxStorageBytes = tl.MaxStorageBytes
	}
	if tl.MaxBranchesPerTree > 0 {
		eff.MaxBranchesPerTree = tl.MaxBranchesPerTree
	}
	if tl.MaxMembers > 0 {
		eff.MaxMembers = tl.MaxMembers
	}
	if tl.RequestsPerMinute > 0 {
		eff.RequestsPerMinute = tl.RequestsPerMinute
	}
	return eff
}

// ---- storage usage ------------------------------------------------------

// StorageBytes returns the recorded storage usage of a tenant (0 if none).
func StorageBytes(ctx context.Context, s domain.DocStore, tenantID string) (int64, error) {
	var u UsageDoc
	err := s.GetDoc(ctx, CollUsage, tenantID, &u)
	if errors.Is(err, domain.ErrNotFound) {
		return 0, nil
	}
	return u.Bytes, err
}

// SetStorageBytes records the exact storage usage of a tenant (used by
// garbage collection after recomputing it from reachability).
func SetStorageBytes(ctx context.Context, s domain.DocStore, tenantID string, n int64, at time.Time) error {
	return s.PutDoc(ctx, CollUsage, tenantID, UsageDoc{Bytes: n, UpdatedAt: at.UTC()})
}

// AddStorageBytes atomically adds delta to a tenant's storage usage.
func AddStorageBytes(ctx context.Context, s domain.DocStore, tenantID string, delta int64, at time.Time) error {
	return s.UpdateDoc(ctx, CollUsage, tenantID, func(raw []byte) ([]byte, error) {
		var u UsageDoc
		if raw != nil {
			if err := json.Unmarshal(raw, &u); err != nil {
				return nil, err
			}
		}
		u.Bytes += delta
		if u.Bytes < 0 {
			u.Bytes = 0
		}
		u.UpdatedAt = at.UTC()
		return json.Marshal(u)
	})
}

// Usage reports a tenant's resource usage and limits.
func (l *Limiter) Usage(ctx context.Context, t *domain.Tenant) (*Usage, error) {
	trees, err := l.d.Store.ListTrees(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	u := &Usage{Trees: len(trees), Effective: l.Effective(t)}
	if t.Limits != nil {
		u.Limits = *t.Limits
	}
	for _, tr := range trees {
		bs, err := l.d.Store.ListBranches(ctx, tr.ID)
		if err != nil {
			return nil, err
		}
		u.Branches += len(bs)
	}
	ms, err := l.d.Store.ListTenantMembers(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	u.Members = len(ms)
	if u.StorageBytes, err = StorageBytes(ctx, l.d.Store, t.ID); err != nil {
		return nil, err
	}
	return u, nil
}

// ---- REST quotas --------------------------------------------------------

// CheckTreeCreate returns a *QuotaError when t already has MaxTrees trees.
func (l *Limiter) CheckTreeCreate(ctx context.Context, t *domain.Tenant) error {
	max := l.Effective(t).MaxTrees
	if max <= 0 {
		return nil
	}
	trees, err := l.d.Store.ListTrees(ctx, t.ID)
	if err != nil {
		return err
	}
	if len(trees) >= max {
		return &QuotaError{Limit: LimitTrees, Max: int64(max)}
	}
	return nil
}

// CheckMemberAdd returns a *QuotaError when t already has MaxMembers
// members (call it only when adding a new member, not changing a role).
func (l *Limiter) CheckMemberAdd(ctx context.Context, t *domain.Tenant) error {
	max := l.Effective(t).MaxMembers
	if max <= 0 {
		return nil
	}
	ms, err := l.d.Store.ListTenantMembers(ctx, t.ID)
	if err != nil {
		return err
	}
	if len(ms) >= max {
		return &QuotaError{Limit: LimitMembers, Max: int64(max)}
	}
	return nil
}

// ---- push checker -------------------------------------------------------

// CheckPushPlan implements domain.PushChecker: MaxBranchesPerTree for new
// branches and MaxStorageBytes for the bytes the push adds.
func (l *Limiter) CheckPushPlan(ctx context.Context, _ *domain.Principal, tree *domain.Tree, plan *domain.PushPlan) error {
	tenant, err := l.d.Store.GetTenant(ctx, tree.TenantID)
	if err != nil {
		return fmt.Errorf("limits: load tenant: %w", err)
	}
	eff := l.Effective(tenant)

	if eff.MaxBranchesPerTree > 0 {
		_, err := l.d.Store.GetBranch(ctx, tree.ID, plan.Branch)
		switch {
		case errors.Is(err, domain.ErrNotFound):
			bs, err := l.d.Store.ListBranches(ctx, tree.ID)
			if err != nil {
				return err
			}
			if len(bs) >= eff.MaxBranchesPerTree {
				return &domain.PushDenied{Kind: "quota_exceeded", Reason: LimitBranches}
			}
		case err != nil:
			return err
		}
	}

	if l.d.Objects == nil {
		return nil
	}
	delta, err := l.pushDelta(ctx, plan)
	if err != nil {
		return fmt.Errorf("limits: size push: %w", err)
	}
	if eff.MaxStorageBytes > 0 && delta > 0 {
		cur, err := StorageBytes(ctx, l.d.Store, tenant.ID)
		if err != nil {
			return err
		}
		if cur+delta > eff.MaxStorageBytes {
			return &domain.PushDenied{Kind: "quota_exceeded", Reason: LimitStorage}
		}
	}
	if delta > 0 {
		l.addPending(pendingKey(tree.ID, plan.Branch, plan.NewTip), pendingDelta{tenantID: tenant.ID, bytes: delta, at: l.d.Now()})
	}
	return nil
}

// pushDelta sizes the objects referenced by the push's new snapshots that
// their stored parents do not already reference.
func (l *Limiter) pushDelta(ctx context.Context, plan *domain.PushPlan) (int64, error) {
	isNew := make(map[string]bool, len(plan.NewSnapshots))
	for _, s := range plan.NewSnapshots {
		isNew[s.ID] = true
	}
	base := map[hash.Hash]struct{}{}
	seenBase := map[string]bool{}
	for _, s := range plan.NewSnapshots {
		for _, parent := range s.Parents {
			if isNew[parent] || seenBase[parent] {
				continue
			}
			seenBase[parent] = true
			ps, err := l.d.Store.GetSnapshot(ctx, parent)
			if errors.Is(err, domain.ErrNotFound) {
				continue
			}
			if err != nil {
				return 0, err
			}
			if err := SnapshotObjects(ctx, l.d.Objects, ps.ManifestHash, base); err != nil {
				return 0, err
			}
		}
	}
	added := map[hash.Hash]struct{}{}
	for _, s := range plan.NewSnapshots {
		if err := SnapshotObjects(ctx, l.d.Objects, s.ManifestHash, added); err != nil {
			return 0, err
		}
	}
	var total int64
	for h := range added {
		if _, ok := base[h]; ok {
			continue
		}
		n, err := objstore.ObjectSize(ctx, l.d.Objects, h)
		if errors.Is(err, objstore.ErrNotFound) {
			continue
		}
		if err != nil {
			return 0, err
		}
		total += n
	}
	return total, nil
}

// SnapshotObjects adds the manifest and every object its entries need
// (blobs, chunk lists and chunks) to set. A missing manifest adds only its
// hash; a missing chunk list adds only the chunk list hash.
func SnapshotObjects(ctx context.Context, objs objstore.Store, manifest hash.Hash, set map[hash.Hash]struct{}) error {
	set[manifest] = struct{}{}
	raw, err := objs.Get(ctx, manifest)
	if errors.Is(err, objstore.ErrNotFound) {
		return nil
	}
	if err != nil {
		return err
	}
	var m syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &m); err != nil {
		return fmt.Errorf("decode manifest %s: %w", manifest, err)
	}
	for _, e := range m.GetEntries() {
		h, err := content.EntryHash(e)
		if err != nil {
			return err
		}
		if _, ok := set[h]; ok {
			continue
		}
		if !e.GetChunked() {
			set[h] = struct{}{}
			continue
		}
		hs, err := content.EntryObjects(ctx, objs, e)
		if errors.Is(err, objstore.ErrNotFound) {
			set[h] = struct{}{}
			continue
		}
		if err != nil {
			return err
		}
		for _, x := range hs {
			set[x] = struct{}{}
		}
	}
	return nil
}

func pendingKey(treeID, branch, tip string) string { return domain.DocKey(treeID, branch, tip) }

func (l *Limiter) addPending(key string, d pendingDelta) {
	l.mu.Lock()
	defer l.mu.Unlock()
	for k, p := range l.pending {
		if d.at.Sub(p.at) > pendingTTL {
			delete(l.pending, k)
		}
	}
	l.pending[key] = d
}

// Accepted applies the storage delta computed for an accepted push.
func (l *Limiter) Accepted(ctx context.Context, treeID, branch, tip string) {
	key := pendingKey(treeID, branch, tip)
	l.mu.Lock()
	d, ok := l.pending[key]
	delete(l.pending, key)
	l.mu.Unlock()
	if !ok {
		return
	}
	if err := AddStorageBytes(ctx, l.d.Store, d.tenantID, d.bytes, l.d.Now()); err != nil {
		l.d.Logger.WarnContext(ctx, "limits: recording storage usage failed", slog.String("tenant_id", d.tenantID), slog.Any("error", err))
	}
}

// Start subscribes to accepted pushes (EventBranchPushed) to apply their
// storage deltas; it returns a stop function. Without a bus it is a no-op.
func (l *Limiter) Start(ctx context.Context) (stop func()) {
	if l.d.Bus == nil {
		return func() {}
	}
	ch, unsub := l.d.Bus.Subscribe(domain.EventFilter{Types: []string{domain.EventBranchPushed}})
	ctx, cancel := context.WithCancel(ctx)
	done := make(chan struct{})
	go func() {
		defer close(done)
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-ch:
				if !ok {
					return
				}
				branch, tip := payloadString(e.Payload, "branch"), payloadString(e.Payload, "new_tip")
				if e.TreeID != "" && branch != "" && tip != "" {
					l.Accepted(ctx, e.TreeID, branch, tip)
				}
			}
		}
	}()
	return func() { cancel(); unsub(); <-done }
}

func payloadString(p any, key string) string {
	switch m := p.(type) {
	case map[string]any:
		s, _ := m[key].(string)
		return s
	case map[string]string:
		return m[key]
	}
	return ""
}
