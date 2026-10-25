package limits

import (
	"context"
	"errors"
	"log/slog"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

var quiet = slog.New(slog.DiscardHandler)

type clock struct {
	mu  sync.Mutex
	now time.Time
}

func (c *clock) Now() time.Time      { c.mu.Lock(); defer c.mu.Unlock(); return c.now }
func (c *clock) Add(d time.Duration) { c.mu.Lock(); c.now = c.now.Add(d); c.mu.Unlock() }

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

type fixture struct {
	t      *testing.T
	ctx    context.Context
	store  domain.MetaStore
	objs   *objstore.DiskStore
	clk    *clock
	tenant *domain.Tenant
	tree   *domain.Tree
}

func newFixture(t *testing.T, tl *domain.TenantLimits) *fixture {
	f := &fixture{t: t, ctx: context.Background(), store: meta.NewMemory(), objs: objstore.New(t.TempDir()),
		clk: &clock{now: time.Date(2026, 6, 1, 0, 0, 0, 0, time.UTC)}}
	t.Cleanup(func() { _ = f.store.Close() })
	f.tenant = &domain.Tenant{ID: "ten-1", Slug: "acme", Kind: domain.TenantOrg, Limits: tl}
	must(t, f.store.CreateTenant(f.ctx, f.tenant))
	f.tree = &domain.Tree{ID: "tree-1", TenantID: f.tenant.ID, Name: "web", DefaultBranch: "main"}
	must(t, f.store.CreateTree(f.ctx, f.tree))
	must(t, f.store.PutBranch(f.ctx, &domain.Branch{TreeID: f.tree.ID, Name: "main"}))
	return f
}

func (f *fixture) limiter(d Deps) *Limiter {
	d.Store, d.Objects, d.Logger, d.Now = f.store, f.objs, quiet, f.clk.Now
	return New(d)
}

func (f *fixture) put(data []byte) hash.Hash {
	h := hash.Sum(data)
	must(f.t, f.objs.Put(f.ctx, h, data))
	return h
}

// manifest stores a manifest over the given files (path -> content).
func (f *fixture) manifest(files map[string]string) hash.Hash {
	m := &syncpb.ManifestRecord{}
	for p, c := range files {
		h := f.put([]byte(c))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}})
	}
	raw, err := proto.Marshal(m)
	must(f.t, err)
	return f.put(raw)
}

func size(f *fixture, hs ...hash.Hash) int64 {
	var n int64
	for _, h := range hs {
		s, err := f.objs.Size(f.ctx, h)
		must(f.t, err)
		n += s
	}
	return n
}

func TestEffective(t *testing.T) {
	l := New(Deps{Defaults: domain.TenantLimits{MaxTrees: 10, MaxStorageBytes: 100}, RateLimit: 60})
	got := l.Effective(&domain.Tenant{Limits: &domain.TenantLimits{MaxTrees: 3, MaxMembers: 5}})
	want := domain.TenantLimits{MaxTrees: 3, MaxStorageBytes: 100, MaxMembers: 5, RequestsPerMinute: 60}
	if got != want {
		t.Fatalf("effective = %+v, want %+v", got, want)
	}
	if got := l.Effective(nil); got != (domain.TenantLimits{MaxTrees: 10, MaxStorageBytes: 100, RequestsPerMinute: 60}) {
		t.Fatalf("effective(nil) = %+v", got)
	}
}

func TestTreeAndMemberQuotas(t *testing.T) {
	f := newFixture(t, &domain.TenantLimits{MaxTrees: 1, MaxMembers: 1})
	l := f.limiter(Deps{})
	var qe *QuotaError
	if err := l.CheckTreeCreate(f.ctx, f.tenant); !errors.As(err, &qe) || qe.Limit != LimitTrees || !errors.Is(err, ErrQuotaExceeded) {
		t.Fatalf("tree quota err = %v", err)
	}
	if err := l.CheckMemberAdd(f.ctx, f.tenant); err != nil {
		t.Fatalf("first member: %v", err)
	}
	must(t, f.store.PutMembership(f.ctx, &domain.Membership{TenantID: f.tenant.ID, AccountID: "a1", Role: domain.RoleOwner}))
	if err := l.CheckMemberAdd(f.ctx, f.tenant); !errors.As(err, &qe) || qe.Limit != LimitMembers {
		t.Fatalf("member quota err = %v", err)
	}
	// Unlimited by default.
	f2 := newFixture(t, nil)
	if err := f2.limiter(Deps{}).CheckTreeCreate(f2.ctx, f2.tenant); err != nil {
		t.Fatal(err)
	}
	// Server default applies when the tenant sets nothing.
	if err := f2.limiter(Deps{Defaults: domain.TenantLimits{MaxTrees: 1}}).CheckTreeCreate(f2.ctx, f2.tenant); !errors.As(err, &qe) {
		t.Fatalf("default tree quota err = %v", err)
	}
}

func TestBranchQuota(t *testing.T) {
	f := newFixture(t, &domain.TenantLimits{MaxBranchesPerTree: 1})
	l := f.limiter(Deps{})
	m := f.manifest(map[string]string{"a": "x"})
	snap := &domain.Snapshot{ID: "s1", TreeID: f.tree.ID, ManifestHash: m}
	// Pushing to the existing (empty) default branch is fine.
	must(t, l.CheckPushPlan(f.ctx, nil, f.tree, &domain.PushPlan{Branch: "main", NewTip: "s1", NewSnapshots: []*domain.Snapshot{snap}}))
	// A new branch would exceed the quota.
	err := l.CheckPushPlan(f.ctx, nil, f.tree, &domain.PushPlan{Branch: "feature", NewTip: "s1", NewSnapshots: []*domain.Snapshot{snap}})
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != "quota_exceeded" || denied.Reason != LimitBranches {
		t.Fatalf("err = %v", err)
	}
}

func TestStorageQuotaAndAccounting(t *testing.T) {
	f := newFixture(t, &domain.TenantLimits{MaxStorageBytes: 1000})
	bus := events.NewBus()
	l := f.limiter(Deps{Bus: bus})
	stop := l.Start(f.ctx)
	defer stop()

	// First push: s1 with one file.
	m1 := f.manifest(map[string]string{"a.txt": "hello"})
	s1 := &domain.Snapshot{ID: "s1", TreeID: f.tree.ID, ManifestHash: m1}
	must(t, l.CheckPushPlan(f.ctx, nil, f.tree, &domain.PushPlan{Branch: "main", NewTip: "s1", NewSnapshots: []*domain.Snapshot{s1}}))
	want := size(f, m1, hash.Sum([]byte("hello")))
	// Not accepted yet: nothing recorded.
	if n, _ := StorageBytes(f.ctx, f.store, f.tenant.ID); n != 0 {
		t.Fatalf("usage before acceptance = %d", n)
	}
	bus.Publish(domain.Event{Type: domain.EventBranchPushed, TenantID: f.tenant.ID, TreeID: f.tree.ID,
		Payload: map[string]any{"branch": "main", "new_tip": "s1"}})
	waitUsage(t, f, want)
	must(t, f.store.PutSnapshot(f.ctx, s1))

	// Second push on top of s1: only the new file and manifest count.
	m2 := f.manifest(map[string]string{"a.txt": "hello", "b.txt": "world!"})
	s2 := &domain.Snapshot{ID: "s2", TreeID: f.tree.ID, Parents: []string{"s1"}, ManifestHash: m2}
	must(t, l.CheckPushPlan(f.ctx, nil, f.tree, &domain.PushPlan{Branch: "main", OldTip: "s1", NewTip: "s2", NewSnapshots: []*domain.Snapshot{s2}}))
	l.Accepted(f.ctx, f.tree.ID, "main", "s2")
	want += size(f, m2, hash.Sum([]byte("world!")))
	waitUsage(t, f, want)
	// Accepting twice is a no-op.
	l.Accepted(f.ctx, f.tree.ID, "main", "s2")
	waitUsage(t, f, want)

	// A push that would exceed the quota is denied.
	big := make([]byte, 2000)
	for i := range big {
		big[i] = byte(i)
	}
	m3 := f.manifest(map[string]string{"big.bin": string(big)})
	s3 := &domain.Snapshot{ID: "s3", TreeID: f.tree.ID, Parents: []string{"s1"}, ManifestHash: m3}
	err := l.CheckPushPlan(f.ctx, nil, f.tree, &domain.PushPlan{Branch: "main", NewTip: "s3", NewSnapshots: []*domain.Snapshot{s3}})
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != "quota_exceeded" || denied.Reason != LimitStorage {
		t.Fatalf("err = %v", err)
	}

	// SetStorageBytes (GC recomputation) overrides the running total.
	must(t, SetStorageBytes(f.ctx, f.store, f.tenant.ID, 7, f.clk.Now()))
	waitUsage(t, f, 7)
	must(t, AddStorageBytes(f.ctx, f.store, f.tenant.ID, -100, f.clk.Now()))
	waitUsage(t, f, 0)
}

func waitUsage(t *testing.T, f *fixture, want int64) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for {
		n, err := StorageBytes(f.ctx, f.store, f.tenant.ID)
		must(t, err)
		if n == want {
			return
		}
		if time.Now().After(deadline) {
			t.Fatalf("usage = %d, want %d", n, want)
		}
		time.Sleep(2 * time.Millisecond)
	}
}

func TestUsage(t *testing.T) {
	f := newFixture(t, &domain.TenantLimits{MaxTrees: 4})
	l := f.limiter(Deps{Defaults: domain.TenantLimits{MaxStorageBytes: 50}, RateLimit: 30})
	must(t, f.store.PutBranch(f.ctx, &domain.Branch{TreeID: f.tree.ID, Name: "dev"}))
	must(t, f.store.PutMembership(f.ctx, &domain.Membership{TenantID: f.tenant.ID, AccountID: "a1", Role: domain.RoleOwner}))
	must(t, SetStorageBytes(f.ctx, f.store, f.tenant.ID, 42, f.clk.Now()))
	u, err := l.Usage(f.ctx, f.tenant)
	must(t, err)
	if u.Trees != 1 || u.Branches != 2 || u.Members != 1 || u.StorageBytes != 42 ||
		u.Limits != (domain.TenantLimits{MaxTrees: 4}) ||
		u.Effective != (domain.TenantLimits{MaxTrees: 4, MaxStorageBytes: 50, RequestsPerMinute: 30}) {
		t.Fatalf("usage = %+v", u)
	}
}

func TestRateLimit(t *testing.T) {
	f := newFixture(t, nil)
	l := f.limiter(Deps{RateLimit: 3})
	alice := &domain.Principal{AccountID: "a1", Username: "alice", TenantID: "personal"}
	for i := 0; i < 3; i++ {
		d := l.Allow(f.ctx, alice, "10.0.0.1")
		if !d.Allowed || d.Limit != 3 || d.Remaining != 2-i {
			t.Fatalf("request %d: %+v", i, d)
		}
	}
	d := l.Allow(f.ctx, alice, "10.0.0.1")
	if d.Allowed || d.RetryAfter <= 0 || d.RetryAfter > 20*time.Second {
		t.Fatalf("over budget: %+v", d)
	}
	// Other principals and anonymous IPs have their own buckets.
	if d := l.Allow(f.ctx, &domain.Principal{AccountID: "b1", Username: "bob"}, "10.0.0.1"); !d.Allowed {
		t.Fatalf("bob: %+v", d)
	}
	if d := l.Allow(f.ctx, nil, "10.0.0.1"); !d.Allowed {
		t.Fatalf("anonymous: %+v", d)
	}
	// 3/min refills one token every 20s.
	f.clk.Add(20 * time.Second)
	if d := l.Allow(f.ctx, alice, ""); !d.Allowed {
		t.Fatalf("after refill: %+v", d)
	}
	if d := l.Allow(f.ctx, alice, ""); d.Allowed {
		t.Fatalf("refill granted more than one token: %+v", d)
	}
	// The bucket never exceeds its capacity.
	f.clk.Add(time.Hour)
	for i := 0; i < 3; i++ {
		if d := l.Allow(f.ctx, alice, ""); !d.Allowed {
			t.Fatalf("after idle, request %d denied", i)
		}
	}
	if d := l.Allow(f.ctx, alice, ""); d.Allowed {
		t.Fatal("bucket overfilled")
	}
}

func TestRateLimitTenantOverrideAndDisabled(t *testing.T) {
	f := newFixture(t, &domain.TenantLimits{RequestsPerMinute: 1})
	l := f.limiter(Deps{RateLimit: 100})
	sa := &domain.Principal{Username: "sa:acme/ci", TenantID: f.tenant.ID, ServiceAccountID: "sa1", Kind: domain.PrincipalServiceAccount}
	if d := l.Allow(f.ctx, sa, ""); !d.Allowed || d.Limit != 1 {
		t.Fatalf("first: %+v", d)
	}
	if d := l.Allow(f.ctx, sa, ""); d.Allowed {
		t.Fatalf("tenant override not applied: %+v", d)
	}
	off := f.limiter(Deps{})
	for i := 0; i < 1000; i++ {
		if d := off.Allow(f.ctx, nil, "1.2.3.4"); !d.Allowed || d.Limit != 0 {
			t.Fatalf("disabled limiter denied: %+v", d)
		}
	}
}

func TestRateKey(t *testing.T) {
	cases := map[string]string{
		RateKey(nil, "1.2.3.4"):                                "ip:1.2.3.4",
		RateKey(&domain.Principal{AccountID: "a"}, "x"):        "acct:a",
		RateKey(&domain.Principal{ServiceAccountID: "s"}, "x"): "sa:s",
	}
	for got, want := range cases {
		if got != want {
			t.Errorf("RateKey = %q, want %q", got, want)
		}
	}
}
