package release

import (
	"bytes"
	"context"
	"errors"
	"io"
	"log/slog"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

type fakeAudit struct {
	mu      sync.Mutex
	actions []string
}

func (a *fakeAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.actions = append(a.actions, e.Action)
}

func (a *fakeAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}

func (a *fakeAudit) has(action string) bool {
	a.mu.Lock()
	defer a.mu.Unlock()
	for _, x := range a.actions {
		if x == action {
			return true
		}
	}
	return false
}

type fixture struct {
	ctx    context.Context
	store  *meta.Memory
	bus    *events.Bus
	audit  *fakeAudit
	objs   *objstore.DiskStore
	svc    *Service
	tenant *domain.Tenant
	tree   *domain.Tree
	p      *domain.Principal
	clock  time.Time
}

var t0 = time.Date(2026, 5, 6, 7, 8, 9, 0, time.UTC)

func must(t testing.TB, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func newFixture(t *testing.T) *fixture {
	t.Helper()
	f := &fixture{ctx: context.Background(), store: meta.NewMemory(), bus: events.NewBus(), audit: &fakeAudit{},
		objs: objstore.New(t.TempDir()), clock: t0}
	f.tenant = &domain.Tenant{ID: "t-acme", Slug: "acme", Kind: domain.TenantOrg}
	must(t, f.store.CreateTenant(f.ctx, f.tenant))
	f.tree = &domain.Tree{ID: "tree-web", TenantID: f.tenant.ID, Name: "web", DefaultBranch: "main"}
	must(t, f.store.CreateTree(f.ctx, f.tree))
	for _, id := range []string{"snap-1", "snap-2"} {
		must(t, f.store.PutSnapshot(f.ctx, &domain.Snapshot{ID: id, TreeID: f.tree.ID, ManifestHash: hash.Sum([]byte(id)), CreatedAt: t0}))
	}
	must(t, f.store.PutTag(f.ctx, &domain.Tag{TreeID: f.tree.ID, Name: "v1.0", Target: "snap-1", CreatedAt: t0}))
	f.p = &domain.Principal{AccountID: "a-alice", Username: "alice"}
	f.svc = New(Deps{Store: f.store, Objects: f.objs, Bus: f.bus, Audit: f.audit, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	f.svc.now = func() time.Time { return f.clock }
	return f
}

func TestCreateReleases(t *testing.T) {
	f := newFixture(t)
	evs, cancel := f.bus.Subscribe(domain.EventFilter{})
	defer cancel()

	// Existing tag; name defaults to the tag.
	r1, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0", Notes: "first"})
	must(t, err)
	if r1.SnapshotID != "snap-1" || r1.Name != "v1.0" || r1.Author != "alice" || r1.CreatedBy != "a-alice" ||
		r1.PublishedAt == nil || r1.Assets == nil || r1.Draft {
		t.Fatalf("r1 = %+v", r1)
	}
	if ev := <-evs; ev.Type != domain.EventReleaseCreated || ev.TreeID != f.tree.ID || ev.Actor != "alice" {
		t.Fatalf("event = %+v", ev)
	}

	// New tag created at the snapshot.
	f.clock = t0.Add(time.Hour)
	r2, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v2.0", Name: " Two ", SnapshotID: "snap-2", Draft: true, Prerelease: true})
	must(t, err)
	if r2.Name != "Two" || r2.SnapshotID != "snap-2" || r2.PublishedAt != nil || !r2.Prerelease {
		t.Fatalf("r2 = %+v", r2)
	}
	tag, err := f.store.GetTag(f.ctx, f.tree.ID, "v2.0")
	must(t, err)
	if tag.Target != "snap-2" || tag.Tagger != "alice" {
		t.Fatalf("tag = %+v", tag)
	}
	if ev := <-evs; ev.Type != domain.EventTagPushed {
		t.Fatalf("tag event = %+v", ev)
	}
	if !f.audit.has("tag.create") || !f.audit.has("release.create") {
		t.Fatalf("audit = %v", f.audit.actions)
	}

	errCases := []struct {
		name string
		in   CreateInput
		kind error
	}{
		{"duplicate", CreateInput{Tag: "v1.0"}, domain.ErrAlreadyExists},
		{"no tag", CreateInput{Tag: " "}, domain.ErrInvalid},
		{"new tag without snapshot", CreateInput{Tag: "v3"}, domain.ErrInvalid},
		{"tag elsewhere", CreateInput{Tag: "v2.0x", SnapshotID: "snap-1"}, nil},
		{"long name", CreateInput{Tag: "v4", SnapshotID: "snap-1", Name: strings.Repeat("n", MaxName+1)}, domain.ErrInvalid},
	}
	must(t, f.store.PutTag(f.ctx, &domain.Tag{TreeID: f.tree.ID, Name: "moved", Target: "snap-2"}))
	errCases = append(errCases, struct {
		name string
		in   CreateInput
		kind error
	}{"tag mismatch", CreateInput{Tag: "moved", SnapshotID: "snap-1"}, domain.ErrConflict})
	for _, c := range errCases {
		_, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, c.in)
		if c.kind == nil {
			must(t, err)
			continue
		}
		if !errors.Is(err, c.kind) {
			t.Fatalf("%s: err = %v", c.name, err)
		}
	}

	// Listing: newest first, drafts only on request.
	all, err := f.svc.List(f.ctx, f.tree.ID, true)
	must(t, err)
	if len(all) != 3 || all[2].Tag != "v1.0" {
		t.Fatalf("all = %+v", all)
	}
	pub, err := f.svc.List(f.ctx, f.tree.ID, false)
	must(t, err)
	for _, r := range pub {
		if r.Draft {
			t.Fatalf("draft listed: %+v", pub)
		}
	}
	if len(pub) != 2 {
		t.Fatalf("public = %+v", pub)
	}
	if _, err := f.svc.Get(f.ctx, f.tree.ID, "v2.0", false); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("draft visible: %v", err)
	}
	if _, err := f.svc.Get(f.ctx, f.tree.ID, "v2.0", true); err != nil {
		t.Fatal(err)
	}
}

func TestUpdateAndDelete(t *testing.T) {
	f := newFixture(t)
	_, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0", Draft: true})
	must(t, err)
	f.clock = t0.Add(time.Hour)
	name, notes, draft := "One", "notes", false
	r, err := f.svc.Update(f.ctx, f.p, f.tenant, f.tree, "v1.0", Patch{Name: &name, Notes: &notes, Draft: &draft})
	must(t, err)
	if r.Name != "One" || r.Notes != "notes" || r.Draft || r.PublishedAt == nil || !r.PublishedAt.Equal(f.clock) {
		t.Fatalf("updated = %+v", r)
	}
	blank := " "
	r, err = f.svc.Update(f.ctx, f.p, f.tenant, f.tree, "v1.0", Patch{Name: &blank})
	must(t, err)
	if r.Name != "v1.0" {
		t.Fatalf("blank name = %q", r.Name)
	}
	if _, err := f.svc.Update(f.ctx, f.p, f.tenant, f.tree, "nope", Patch{}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing: %v", err)
	}
	if !f.audit.has("release.update") {
		t.Fatal("no release.update audit")
	}
	must(t, f.svc.Delete(f.ctx, f.p, f.tenant, f.tree, "v1.0"))
	if err := f.svc.Delete(f.ctx, f.p, f.tenant, f.tree, "v1.0"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("second delete: %v", err)
	}
	if _, err := f.store.GetTag(f.ctx, f.tree.ID, "v1.0"); err != nil {
		t.Fatalf("tag removed with the release: %v", err)
	}
	if !f.audit.has("release.delete") {
		t.Fatal("no release.delete audit")
	}
}

func TestAssets(t *testing.T) {
	f := newFixture(t)
	_, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0"})
	must(t, err)

	a, err := f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "app.tar.gz", "", strings.NewReader("tarball"))
	must(t, err)
	if a.Size != 7 || a.Hash != hash.Sum([]byte("tarball")) || a.ContentType != "application/gzip" && a.ContentType != "application/x-gzip" {
		t.Fatalf("asset = %+v", a)
	}
	if ok, _ := f.objs.Has(f.ctx, a.Hash); !ok {
		t.Fatal("asset bytes not in the object store")
	}
	_, err = f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "notes.txt", "text/plain; charset=utf-8", strings.NewReader("hi"))
	must(t, err)
	// Replacing keeps one entry.
	_, err = f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "notes.txt", "bogus///", strings.NewReader("hello"))
	must(t, err)
	r, err := f.svc.Get(f.ctx, f.tree.ID, "v1.0", false)
	must(t, err)
	if len(r.Assets) != 2 || r.Assets[1].Size != 5 || !strings.HasPrefix(r.Assets[1].ContentType, "text/plain") {
		t.Fatalf("assets = %+v", r.Assets)
	}
	meta, data, err := f.svc.Asset(f.ctx, f.tree.ID, "v1.0", "notes.txt", false)
	must(t, err)
	if string(data) != "hello" || meta.Name != "notes.txt" {
		t.Fatalf("download = %q %+v", data, meta)
	}
	hashes, err := AssetHashes(f.ctx, f.store)
	must(t, err)
	if len(hashes) != 2 {
		t.Fatalf("hashes = %v", hashes)
	}

	for _, bad := range []string{"", ".", "..", "a/b", `a\b`, "x\x00", " lead", strings.Repeat("a", 256)} {
		if _, err := f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", bad, "", strings.NewReader("x")); !errors.Is(err, domain.ErrInvalid) {
			t.Fatalf("name %q: %v", bad, err)
		}
	}
	if _, err := f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "nope", "a", "", strings.NewReader("x")); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("missing release: %v", err)
	}
	var tle *TooLargeError
	if _, err := f.svc.PutAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "big", "", bytes.NewReader(make([]byte, MaxAssetSize+1))); !errors.As(err, &tle) {
		t.Fatalf("too large: %v", err)
	}

	must(t, f.svc.DeleteAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "app.tar.gz"))
	if err := f.svc.DeleteAsset(f.ctx, f.p, f.tenant, f.tree, "v1.0", "app.tar.gz"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("second delete: %v", err)
	}
	if _, _, err := f.svc.Asset(f.ctx, f.tree.ID, "v1.0", "app.tar.gz", false); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("deleted asset: %v", err)
	}
	for _, a := range []string{"release.asset.upload", "release.asset.delete"} {
		if !f.audit.has(a) {
			t.Fatalf("no %s audit", a)
		}
	}
}

func TestTreeDeletedRemovesReleases(t *testing.T) {
	f := newFixture(t)
	other := &domain.Tree{ID: "tree-web2", TenantID: f.tenant.ID, Name: "web2"}
	must(t, f.store.CreateTree(f.ctx, other))
	must(t, f.store.PutSnapshot(f.ctx, &domain.Snapshot{ID: "snap-o", TreeID: other.ID, ManifestHash: hash.Sum([]byte("o"))}))
	_, err := f.svc.Create(f.ctx, f.p, f.tenant, f.tree, CreateInput{Tag: "v1.0"})
	must(t, err)
	_, err = f.svc.Create(f.ctx, f.p, f.tenant, other, CreateInput{Tag: "v1.0", SnapshotID: "snap-o"})
	must(t, err)

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	f.svc.Start(ctx)
	f.bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: f.tenant.ID, TreeID: f.tree.ID})
	deadline := time.Now().Add(5 * time.Second)
	for {
		l, err := f.svc.List(f.ctx, f.tree.ID, true)
		must(t, err)
		if len(l) == 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("releases not removed")
		}
		time.Sleep(5 * time.Millisecond)
	}
	if l, _ := f.svc.List(f.ctx, other.ID, true); len(l) != 1 {
		t.Fatalf("other tree lost its release: %+v", l)
	}
	must(t, f.svc.HandleEvent(f.ctx, domain.Event{Type: domain.EventBranchPushed, TreeID: other.ID}))
}
