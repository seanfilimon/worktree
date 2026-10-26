package fork

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"slices"
	"sort"
	"sync"
	"testing"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork/snapcopy"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

var t0 = time.Date(2026, 10, 1, 12, 0, 0, 0, time.UTC)

type memObjects struct {
	mu   sync.Mutex
	objs map[hash.Hash][]byte
}

func (o *memObjects) Put(_ context.Context, h hash.Hash, data []byte) error {
	o.mu.Lock()
	defer o.mu.Unlock()
	o.objs[h] = append([]byte(nil), data...)
	return nil
}

func (o *memObjects) Get(_ context.Context, h hash.Hash) ([]byte, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	d, ok := o.objs[h]
	if !ok {
		return nil, objstore.ErrNotFound
	}
	return d, nil
}

func (o *memObjects) Has(_ context.Context, h hash.Hash) (bool, error) {
	o.mu.Lock()
	defer o.mu.Unlock()
	_, ok := o.objs[h]
	return ok, nil
}

func (o *memObjects) Delete(context.Context, hash.Hash) error { return nil }

type recBus struct {
	mu     sync.Mutex
	events []domain.Event
}

func (b *recBus) Publish(e domain.Event) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.events = append(b.events, e)
}

func (b *recBus) Subscribe(domain.EventFilter) (<-chan domain.Event, func()) {
	return make(chan domain.Event), func() {}
}

func (b *recBus) types() []string {
	b.mu.Lock()
	defer b.mu.Unlock()
	var out []string
	for _, e := range b.events {
		out = append(out, e.Type)
	}
	return out
}

type recAudit struct {
	mu     sync.Mutex
	events []domain.AuditEvent
}

func (a *recAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, e)
}

func (a *recAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}

type quotaFunc func(*domain.Tenant) error

func (q quotaFunc) CheckTreeCreate(_ context.Context, t *domain.Tenant) error { return q(t) }

type env struct {
	t     *testing.T
	ctx   context.Context
	store *meta.Memory
	objs  *memObjects
	bus   *recBus
	audit *recAudit
	svc   *Service
	acme  *domain.Tenant
	users map[string]*domain.Principal
	tens  map[string]*domain.Tenant
	web   *domain.Tree
	n     int
}

func must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

// newEnv: acme/web (private) with main s1 <- s2, feature s1 <- s3 and tag
// v1 -> s1. alice owns acme; bob is a viewer of acme; carol has no role.
func newEnv(t *testing.T) *env {
	t.Helper()
	ctx := context.Background()
	e := &env{t: t, ctx: ctx, store: meta.NewMemory(), objs: &memObjects{objs: map[hash.Hash][]byte{}},
		bus: &recBus{}, audit: &recAudit{}, users: map[string]*domain.Principal{}, tens: map[string]*domain.Tenant{}}
	e.acme = &domain.Tenant{ID: "ten-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: t0}
	must(t, e.store.CreateTenant(ctx, e.acme))
	for u, role := range map[string]string{"alice": domain.RoleOwner, "bob": domain.RoleViewer, "carol": ""} {
		ten := &domain.Tenant{ID: "ten-" + u, Slug: u, Name: u, Kind: domain.TenantUser, CreatedAt: t0}
		must(t, e.store.CreateTenant(ctx, ten))
		acct := &domain.Account{ID: "acct-" + u, Username: u, Email: u + "@x.test", TenantID: ten.ID, CreatedAt: t0}
		must(t, e.store.CreateAccount(ctx, acct))
		must(t, e.store.PutMembership(ctx, &domain.Membership{TenantID: ten.ID, AccountID: acct.ID, Role: domain.RoleOwner, CreatedAt: t0}))
		if role != "" {
			must(t, e.store.PutMembership(ctx, &domain.Membership{TenantID: e.acme.ID, AccountID: acct.ID, Role: role, CreatedAt: t0}))
		}
		e.users[u] = &domain.Principal{AccountID: acct.ID, Username: u, TenantID: ten.ID}
		e.tens[u] = ten
	}
	e.web = &domain.Tree{ID: "tree-web", TenantID: e.acme.ID, Name: "web", Description: "the web", DefaultBranch: "main", Visibility: domain.VisibilityInternal, CreatedAt: t0, UpdatedAt: t0}
	must(t, e.store.CreateTree(ctx, e.web))
	s1 := e.commit(e.web, map[string]string{"README.md": "# web\n"})
	s2 := e.commit(e.web, map[string]string{"README.md": "# web 2\n", "src/app.go": "package app\n"}, s1)
	s3 := e.commit(e.web, map[string]string{"README.md": "# web\n", "secret/key.txt": "k"}, s1)
	e.branch(e.web, "main", s2)
	e.branch(e.web, "feature", s3)
	must(t, e.store.PutTag(ctx, &domain.Tag{TreeID: e.web.ID, Name: "v1", Target: s1, Tagger: "alice", Message: "one", CreatedAt: t0, Signature: []byte("sig"), Verified: true}))
	must(t, e.store.PutProtection(ctx, &domain.BranchProtection{TreeID: e.web.ID, Pattern: "main", NoDirectPush: true}))

	e.svc = New(Deps{Store: e.store, Objects: e.objs, History: meta.NewHistory(e.store), Authz: authz.New(e.store, authz.Options{}),
		Bus: e.bus, Audit: e.audit, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	e.svc.Configure(Options{License: license.New(e.store)})
	e.svc.now = func() time.Time { return t0.Add(time.Hour) }
	return e
}

// commit stores a snapshot of tree with files and returns its id.
func (e *env) commit(tree *domain.Tree, files map[string]string, parents ...string) string {
	e.t.Helper()
	m := &syncpb.ManifestRecord{}
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	for _, p := range paths {
		h := hash.Sum([]byte(files[p]))
		must(e.t, e.objs.Put(e.ctx, h, []byte(files[p])))
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(files[p]))})
	}
	raw, err := proto.Marshal(m)
	must(e.t, err)
	mh := hash.Sum(raw)
	must(e.t, e.objs.Put(e.ctx, mh, raw))
	e.n++
	id := fmt.Sprintf("00000000-0000-4000-8000-%012d", e.n)
	must(e.t, e.store.PutSnapshot(e.ctx, &domain.Snapshot{ID: id, TreeID: tree.ID, Parents: parents, ManifestHash: mh, Author: "alice",
		Message: id, CreatedAt: t0.Add(time.Duration(e.n) * time.Minute), Signature: []byte("sig"), SigningKeyID: "k1", Verified: true}))
	return id
}

func (e *env) branch(tree *domain.Tree, name, tip string) {
	e.t.Helper()
	must(e.t, e.store.PutBranch(e.ctx, &domain.Branch{TreeID: tree.ID, Name: name, Tip: tip}))
}

func (e *env) tip(tree *domain.Tree, name string) string {
	e.t.Helper()
	b, err := e.store.GetBranch(e.ctx, tree.ID, name)
	must(e.t, err)
	return b.Tip
}

func (e *env) fork(as, into string, in Input) (*domain.Tree, error) {
	in.Tenant = e.tens[into]
	if into == "acme" {
		in.Tenant = e.acme
	}
	return e.svc.Fork(e.ctx, e.users[as], e.web, in)
}

func TestForkCopiesBranchesAndTags(t *testing.T) {
	e := newEnv(t)
	upMain, upFeature := e.tip(e.web, "main"), e.tip(e.web, "feature")
	f, err := e.fork("bob", "bob", Input{})
	if err != nil {
		t.Fatalf("fork: %v", err)
	}
	if f.ForkOf != e.web.ID || f.TenantID != "ten-bob" || f.Name != "web" || f.Visibility != domain.VisibilityInternal ||
		f.DefaultBranch != "main" || f.Description != "the web" {
		t.Fatalf("fork tree = %+v", f)
	}
	// Branch tips are copies with derived ids; the history is complete.
	if got, want := e.tip(f, "main"), snapcopy.DerivedID(f.ID, upMain); got != want {
		t.Fatalf("main = %s, want %s", got, want)
	}
	if got := e.tip(f, "feature"); got != snapcopy.DerivedID(f.ID, upFeature) {
		t.Fatalf("feature = %s", got)
	}
	log, err := meta.NewHistory(e.store).Log(e.ctx, e.tip(f, "main"), 0)
	must(t, err)
	if len(log) != 2 {
		t.Fatalf("fork main history = %d snapshots", len(log))
	}
	for _, s := range log {
		if s.TreeID != f.ID || s.Signature != nil || s.SigningKeyID != "" || s.Verified {
			t.Fatalf("copied snapshot = %+v", s)
		}
	}
	if n, _ := e.store.CountSnapshots(e.ctx, f.ID); n != 3 {
		t.Fatalf("fork has %d snapshots, want 3", n)
	}
	tag, err := e.store.GetTag(e.ctx, f.ID, "v1")
	must(t, err)
	if tag.Target != snapcopy.DerivedID(f.ID, log[1].ID) && tag.Target != log[1].ID {
		t.Fatalf("tag = %+v", tag)
	}
	if tag.Signature != nil || tag.Verified || tag.Message != "one" {
		t.Fatalf("tag copy = %+v", tag)
	}
	// Protections are not copied.
	if ps, _ := e.store.ListProtections(e.ctx, f.ID); len(ps) != 0 {
		t.Fatalf("protections copied: %+v", ps)
	}
	// Record, events, audit.
	var rec Record
	must(t, e.store.GetDoc(e.ctx, Coll, domain.DocKey(e.web.ID, f.ID), &rec))
	if rec.CreatedBy != "bob" || !slices.Equal(rec.Branches, []string{"feature", "main"}) {
		t.Fatalf("record = %+v", rec)
	}
	if types := e.bus.types(); !slices.Equal(types, []string{domain.EventTreeForked, domain.EventTreeCreated}) {
		t.Fatalf("events = %v", types)
	}
	if ev := e.bus.events[0]; ev.TreeID != e.web.ID || ev.TenantID != e.acme.ID {
		t.Fatalf("tree.forked = %+v", ev)
	}
	if len(e.audit.events) != 2 || e.audit.events[0].Action != "tree.fork" || e.audit.events[1].TenantID != e.acme.ID {
		t.Fatalf("audit = %+v", e.audit.events)
	}

	list, err := e.svc.List(e.ctx, e.users["alice"], e.web)
	must(t, err)
	if len(list) != 1 || list[0].Tenant != "bob" || list[0].Name != "web" {
		t.Fatalf("list = %+v", list)
	}
}

func TestForkOptionsAndRefusals(t *testing.T) {
	e := newEnv(t)
	if _, err := e.fork("carol", "carol", Input{}); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("carol (no role): %v, want ErrForbidden", err)
	}
	if _, err := e.svc.Fork(e.ctx, nil, e.web, Input{Tenant: e.acme}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous: %v", err)
	}
	// bob is only a viewer of acme: no tree:create there.
	if _, err := e.fork("bob", "acme", Input{Name: "web2"}); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("bob into acme: %v, want ErrForbidden", err)
	}
	if _, err := e.fork("bob", "bob", Input{Visibility: domain.VisibilityPublic}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("wider visibility: %v, want ErrInvalid", err)
	}
	if _, err := e.fork("bob", "bob", Input{Name: "bad name"}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad name: %v", err)
	}
	if _, err := e.fork("bob", "bob", Input{Branches: []string{"nope"}}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown branch: %v", err)
	}
	// Same-tenant fork by the owner under another name, one branch only.
	f, err := e.fork("alice", "acme", Input{Name: "web-fork", Branches: []string{"feature"}, Visibility: domain.VisibilityPrivate})
	if err != nil {
		t.Fatalf("same-tenant fork: %v", err)
	}
	bs, _ := e.store.ListBranches(e.ctx, f.ID)
	if len(bs) != 1 || bs[0].Name != "feature" || f.DefaultBranch != "feature" || f.Visibility != domain.VisibilityPrivate {
		t.Fatalf("fork = %+v, branches %+v", f, bs)
	}
	if _, err := e.fork("alice", "acme", Input{Name: "web-fork"}); !errors.Is(err, domain.ErrAlreadyExists) {
		t.Fatalf("duplicate: %v, want ErrAlreadyExists", err)
	}
	// Quota.
	e.svc.Configure(Options{Quota: quotaFunc(func(t *domain.Tenant) error {
		return fmt.Errorf("quota for %s: %w", t.Slug, domain.ErrConflict)
	})})
	if _, err := e.fork("bob", "bob", Input{}); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("quota: %v", err)
	}
	if _, err := e.store.GetTreeByName(e.ctx, "ten-bob", "web"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("refused fork left a tree behind: %v", err)
	}
}

func TestForkLicenseRedistribution(t *testing.T) {
	e := newEnv(t)
	must(t, e.store.PutLicenseRule(e.ctx, &domain.LicenseRule{TreeID: e.web.ID, PathGlob: "secret/**", SPDX: "LicenseRef-Proprietary"}))
	// bob is a member of acme (unrestricted himself), but the fork leaves
	// the tenant: the destination must be allowed to redistribute.
	_, err := e.fork("bob", "bob", Input{})
	var denied *domain.PushDenied
	if !errors.As(err, &denied) || denied.Kind != license.KindLicenseViolation || denied.Path != "secret/key.txt" {
		t.Fatalf("fork with proprietary paths: %v", err)
	}
	if _, err := e.store.GetTreeByName(e.ctx, "ten-bob", "web"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatal("refused fork left a tree behind")
	}
	// Branches (and tags) without the restricted paths may be forked.
	if _, err := e.fork("bob", "bob", Input{Name: "main-only", Branches: []string{"main"}}); err != nil {
		t.Fatalf("fork of main only: %v", err)
	}
	// Inside the tenant the rule does not apply.
	if _, err := e.fork("alice", "acme", Input{Name: "inside"}); err != nil {
		t.Fatalf("same-tenant fork: %v", err)
	}
	// A grant to bob's tenant allows redistribution there.
	must(t, e.store.PutLicenseRule(e.ctx, &domain.LicenseRule{TreeID: e.web.ID, PathGlob: "secret/*", SPDX: "LicenseRef-Proprietary", Grant: license.GrantRedistribute, Grantee: "bob"}))
	if _, err := e.fork("bob", "bob", Input{}); err != nil {
		t.Fatalf("fork with a redistribute grant: %v", err)
	}
}
