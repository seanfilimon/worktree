package staged

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ---- fakes ---------------------------------------------------------------

type fakeStore struct {
	domain.MetaStore // unimplemented methods panic

	mu      sync.Mutex
	staged  map[string]domain.StagedSnapshot
	puts    int
	deletes int
	listErr error
}

func newFakeStore() *fakeStore {
	return &fakeStore{staged: make(map[string]domain.StagedSnapshot)}
}

func key(tree, acct, branch string) string { return tree + "|" + acct + "|" + branch }

func (f *fakeStore) PutStaged(_ context.Context, s *domain.StagedSnapshot) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	cp := *s
	cp.FilesChanged = append([]string(nil), s.FilesChanged...)
	f.staged[key(s.TreeID, s.AccountID, s.BranchName)] = cp
	f.puts++
	return nil
}

func (f *fakeStore) ListStaged(_ context.Context, treeID, branch string) ([]domain.StagedSnapshot, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.listErr != nil {
		return nil, f.listErr
	}
	out := []domain.StagedSnapshot{}
	for _, s := range f.staged {
		if s.TreeID == treeID && (branch == "" || s.BranchName == branch) {
			cp := s
			cp.FilesChanged = append([]string(nil), s.FilesChanged...)
			out = append(out, cp)
		}
	}
	return out, nil
}

func (f *fakeStore) DeleteStaged(_ context.Context, treeID, accountID, branch string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	k := key(treeID, accountID, branch)
	if _, ok := f.staged[k]; !ok {
		return fmt.Errorf("staged %s: %w", k, domain.ErrNotFound)
	}
	delete(f.staged, k)
	f.deletes++
	return nil
}

func (f *fakeStore) count() int {
	f.mu.Lock()
	defer f.mu.Unlock()
	return len(f.staged)
}

// fakeAuthz allows everything except the actions in deny.
type fakeAuthz struct {
	deny map[domain.Action]bool
}

var errDenied = fmt.Errorf("fake: %w", domain.ErrForbidden)

func (a *fakeAuthz) Authorize(_ context.Context, p *domain.Principal, act domain.Action, _ string, _ *domain.Tree) error {
	if a.deny[act] {
		return errDenied
	}
	if p == nil && act != domain.ActTreeRead {
		return fmt.Errorf("fake: %w", domain.ErrUnauthenticated)
	}
	return nil
}

func (a *fakeAuthz) RoleOf(context.Context, string, string) (string, error) { return "", nil }

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
	panic("not used")
}

func (b *fakeBus) all() []domain.Event {
	b.mu.Lock()
	defer b.mu.Unlock()
	return append([]domain.Event(nil), b.events...)
}

func (b *fakeBus) ofType(t string) []domain.Event {
	var out []domain.Event
	for _, e := range b.all() {
		if e.Type == t {
			out = append(out, e)
		}
	}
	return out
}

// ---- helpers -------------------------------------------------------------

var (
	tree  = &domain.Tree{ID: "tree-1", TenantID: "tenant-1", Name: "repo"}
	alice = &domain.Principal{AccountID: "acct-alice", Username: "alice", TenantID: "t-alice"}
	bob   = &domain.Principal{AccountID: "acct-bob", Username: "bob", TenantID: "t-bob"}
	carol = &domain.Principal{AccountID: "acct-carol", Username: "carol", TenantID: "t-carol"}
)

type clock struct {
	mu sync.Mutex
	t  time.Time
}

func (c *clock) now() time.Time { c.mu.Lock(); defer c.mu.Unlock(); return c.t }
func (c *clock) advance(d time.Duration) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.t = c.t.Add(d)
}

type env struct {
	svc   *Service
	store *fakeStore
	authz *fakeAuthz
	bus   *fakeBus
	clock *clock
}

func newEnv() *env {
	e := &env{
		store: newFakeStore(),
		authz: &fakeAuthz{deny: map[domain.Action]bool{}},
		bus:   &fakeBus{},
		clock: &clock{t: time.Date(2026, 3, 1, 12, 0, 0, 0, time.UTC)},
	}
	e.svc = New(e.store, e.authz, e.bus)
	e.svc.now = e.clock.now
	return e
}

func stage(t *testing.T, e *env, p *domain.Principal, branch, snap string, files ...string) domain.StagedSnapshot {
	t.Helper()
	s := &domain.StagedSnapshot{SnapshotID: snap, BranchName: branch, FilesChanged: files, FilesModified: uint32(len(files))}
	if err := e.svc.Stage(context.Background(), p, tree, s); err != nil {
		t.Fatalf("Stage(%s, %s, %s): %v", p.Username, branch, snap, err)
	}
	return *s
}

// ---- Stage ---------------------------------------------------------------

func TestStageSetsServerFieldsAndPublishes(t *testing.T) {
	e := newEnv()
	in := &domain.StagedSnapshot{
		SnapshotID:   "snap-1",
		BranchName:   "feature/x",
		TreeID:       "spoofed-tree",
		TenantID:     "spoofed-tenant",
		AccountID:    "spoofed-acct",
		Author:       "mallory",
		FilesChanged: []string{"b.go", `src\a.go`, "./b.go", "", "dir//c.go"},
		FilesAdded:   1, FilesModified: 2, FilesDeleted: 3,
		Message:  "WIP",
		StagedAt: time.Unix(1, 0),
	}
	if err := e.svc.Stage(context.Background(), alice, tree, in); err != nil {
		t.Fatal(err)
	}
	if in.TreeID != tree.ID || in.TenantID != tree.TenantID || in.AccountID != alice.AccountID || in.Author != alice.Username {
		t.Fatalf("server fields not set: %+v", in)
	}
	if !in.StagedAt.Equal(e.clock.now()) {
		t.Fatalf("StagedAt = %v", in.StagedAt)
	}
	if got := strings.Join(in.FilesChanged, ","); got != "b.go,dir/c.go,src/a.go" {
		t.Fatalf("FilesChanged = %q", got)
	}
	if in.FilesAdded != 1 || in.FilesModified != 2 || in.FilesDeleted != 3 {
		t.Fatalf("counters changed: %+v", in)
	}

	stored, _ := e.store.ListStaged(context.Background(), tree.ID, "")
	if len(stored) != 1 || stored[0].SnapshotID != "snap-1" || stored[0].AccountID != alice.AccountID {
		t.Fatalf("stored = %+v", stored)
	}

	evs := e.bus.all()
	if len(evs) != 1 {
		t.Fatalf("events = %d", len(evs))
	}
	ev := evs[0]
	if ev.Type != domain.EventSnapshotStaged || ev.TenantID != tree.TenantID || ev.TreeID != tree.ID || ev.Actor != "alice" {
		t.Fatalf("event = %+v", ev)
	}
	payload, ok := ev.Payload.(domain.StagedSnapshot)
	if !ok {
		t.Fatalf("payload type %T", ev.Payload)
	}
	if payload.SnapshotID != "snap-1" || payload.Author != "alice" || len(payload.FilesChanged) != 3 {
		t.Fatalf("payload = %+v", payload)
	}
	// Payload must not alias the caller's slice.
	in.FilesChanged[0] = "mutated"
	if e.bus.all()[0].Payload.(domain.StagedSnapshot).FilesChanged[0] == "mutated" {
		t.Fatal("event payload aliases caller's FilesChanged")
	}
}

func TestStageValidation(t *testing.T) {
	tooMany := make([]string, MaxFilesListed+1)
	for i := range tooMany {
		tooMany[i] = fmt.Sprintf("f%d", i)
	}
	tests := []struct {
		name string
		p    *domain.Principal
		tree *domain.Tree
		s    *domain.StagedSnapshot
		want error
	}{
		{"nil tree", alice, nil, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "b"}, domain.ErrInvalid},
		{"nil snapshot", alice, tree, nil, domain.ErrInvalid},
		{"empty snapshot id", alice, tree, &domain.StagedSnapshot{SnapshotID: " ", BranchName: "b"}, domain.ErrInvalid},
		{"empty branch", alice, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: ""}, domain.ErrInvalid},
		{"too many files", alice, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "b", FilesChanged: tooMany}, domain.ErrInvalid},
		{"anonymous", nil, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "b"}, domain.ErrUnauthenticated},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			e := newEnv()
			var before domain.StagedSnapshot
			if tt.s != nil {
				before = *tt.s
			}
			err := e.svc.Stage(context.Background(), tt.p, tt.tree, tt.s)
			if !errors.Is(err, tt.want) {
				t.Fatalf("err = %v, want %v", err, tt.want)
			}
			if e.store.count() != 0 || len(e.bus.all()) != 0 {
				t.Fatal("invalid stage had side effects")
			}
			if tt.s != nil && (tt.s.TreeID != before.TreeID || !tt.s.StagedAt.Equal(before.StagedAt)) {
				t.Fatal("input mutated on error")
			}
		})
	}
}

func TestStageExactlyMaxFilesAccepted(t *testing.T) {
	e := newEnv()
	files := make([]string, MaxFilesListed)
	for i := range files {
		files[i] = fmt.Sprintf("dir/f%05d.go", i)
	}
	s := &domain.StagedSnapshot{SnapshotID: "s", BranchName: "main", FilesChanged: files, FilesModified: MaxFilesListed}
	if err := e.svc.Stage(context.Background(), alice, tree, s); err != nil {
		t.Fatal(err)
	}
	if len(s.FilesChanged) != MaxFilesStored {
		t.Fatalf("stored %d files, want %d", len(s.FilesChanged), MaxFilesStored)
	}
	if s.FilesChanged[0] != "dir/f00000.go" || s.FilesChanged[MaxFilesStored-1] != fmt.Sprintf("dir/f%05d.go", MaxFilesStored-1) {
		t.Fatalf("truncation not deterministic: first=%s last=%s", s.FilesChanged[0], s.FilesChanged[MaxFilesStored-1])
	}
	if s.FilesModified != MaxFilesListed {
		t.Fatalf("FilesModified = %d, counters must be kept", s.FilesModified)
	}
	stored, _ := e.store.ListStaged(context.Background(), tree.ID, "main")
	if len(stored[0].FilesChanged) != MaxFilesStored {
		t.Fatalf("store got %d files", len(stored[0].FilesChanged))
	}
}

func TestStageTruncatesMessage(t *testing.T) {
	e := newEnv()
	msg := strings.Repeat("é", MaxMessageBytes) // 2 bytes per rune
	s := &domain.StagedSnapshot{SnapshotID: "s", BranchName: "main", Message: msg}
	if err := e.svc.Stage(context.Background(), alice, tree, s); err != nil {
		t.Fatal(err)
	}
	if len(s.Message) > MaxMessageBytes || !strings.HasPrefix(msg, s.Message) || len(s.Message) != MaxMessageBytes {
		t.Fatalf("message len %d", len(s.Message))
	}
	if got := truncateUTF8("aé", 2); got != "a" {
		t.Fatalf("truncateUTF8 split a rune: %q", got)
	}
}

func TestStageAuthz(t *testing.T) {
	e := newEnv()
	e.authz.deny[domain.ActTreeWrite] = true
	err := e.svc.Stage(context.Background(), alice, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "b"})
	if !errors.Is(err, errDenied) {
		t.Fatalf("err = %v, want authorizer's error", err)
	}
	if e.store.count() != 0 || len(e.bus.all()) != 0 {
		t.Fatal("denied stage had side effects")
	}
}

func TestStageReplaces(t *testing.T) {
	e := newEnv()
	stage(t, e, alice, "main", "s1", "a.go")
	e.clock.advance(time.Minute)
	stage(t, e, alice, "main", "s2", "b.go")
	stage(t, e, alice, "feature", "s3", "c.go") // different branch: separate entry
	stage(t, e, bob, "main", "s4", "a.go")      // different account: separate entry

	got, err := e.svc.List(context.Background(), alice, tree, "main")
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 2 {
		t.Fatalf("main entries = %+v", got)
	}
	for _, s := range got {
		if s.AccountID == alice.AccountID && (s.SnapshotID != "s2" || s.FilesChanged[0] != "b.go") {
			t.Fatalf("alice's entry not replaced: %+v", s)
		}
	}
	if all, _ := e.svc.List(context.Background(), alice, tree, ""); len(all) != 3 {
		t.Fatalf("all entries = %d, want 3", len(all))
	}
	if n := len(e.bus.ofType(domain.EventSnapshotStaged)); n != 4 {
		t.Fatalf("staged events = %d", n)
	}
}

func TestNilBus(t *testing.T) {
	svc := New(newFakeStore(), &fakeAuthz{}, nil)
	if err := svc.Stage(context.Background(), alice, tree, &domain.StagedSnapshot{SnapshotID: "s", BranchName: "b"}); err != nil {
		t.Fatal(err)
	}
	if err := svc.ClearOnPush(context.Background(), alice, tree, "b"); err != nil {
		t.Fatal(err)
	}
}

// ---- List ----------------------------------------------------------------

func TestListAuthzAndOrder(t *testing.T) {
	e := newEnv()
	stage(t, e, alice, "main", "s1")
	e.clock.advance(time.Second)
	stage(t, e, bob, "main", "s2")
	e.clock.advance(time.Second)
	stage(t, e, carol, "dev", "s3")

	got, err := e.svc.List(context.Background(), nil, tree, "") // anonymous read allowed by fake
	if err != nil {
		t.Fatal(err)
	}
	var ids []string
	for _, s := range got {
		ids = append(ids, s.SnapshotID)
	}
	if strings.Join(ids, ",") != "s3,s2,s1" {
		t.Fatalf("order = %v, want newest first", ids)
	}

	e.authz.deny[domain.ActTreeRead] = true
	if _, err := e.svc.List(context.Background(), alice, tree, ""); !errors.Is(err, errDenied) {
		t.Fatalf("err = %v", err)
	}
	if _, err := e.svc.List(context.Background(), alice, nil, ""); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("nil tree err = %v", err)
	}
}

func TestListEmptyNonNil(t *testing.T) {
	e := newEnv()
	got, err := e.svc.List(context.Background(), alice, tree, "")
	if err != nil || got == nil || len(got) != 0 {
		t.Fatalf("got %#v, %v", got, err)
	}
	e.svc.MaxAge = 0
	got, err = e.svc.List(context.Background(), alice, tree, "")
	if err != nil || got == nil {
		t.Fatalf("got %#v, %v", got, err)
	}
}

func TestListStoreError(t *testing.T) {
	e := newEnv()
	boom := errors.New("boom")
	e.store.listErr = boom
	if _, err := e.svc.List(context.Background(), alice, tree, ""); !errors.Is(err, boom) {
		t.Fatalf("err = %v", err)
	}
}

func TestListTTLExpiry(t *testing.T) {
	e := newEnv()
	if e.svc.MaxAge != DefaultMaxAge {
		t.Fatalf("default MaxAge = %v", e.svc.MaxAge)
	}
	stage(t, e, alice, "main", "old")
	e.clock.advance(DefaultMaxAge - time.Hour)
	stage(t, e, bob, "main", "fresh")
	e.clock.advance(2 * time.Hour) // alice's entry is now older than MaxAge

	got, err := e.svc.List(context.Background(), carol, tree, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || got[0].SnapshotID != "fresh" {
		t.Fatalf("got %+v, want only fresh", got)
	}
	if e.store.count() != 1 {
		t.Fatalf("expired entry not lazily deleted (store has %d)", e.store.count())
	}
	cleared := e.bus.ofType(domain.EventStagedCleared)
	if len(cleared) != 1 {
		t.Fatalf("cleared events = %d", len(cleared))
	}
	c := cleared[0].Payload.(Cleared)
	if c.Reason != ReasonExpired || c.AccountID != alice.AccountID || c.SnapshotID != "old" || c.BranchName != "main" {
		t.Fatalf("cleared payload = %+v", c)
	}

	// Expiry disabled: nothing hidden even when very old.
	e2 := newEnv()
	e2.svc.MaxAge = 0
	stage(t, e2, alice, "main", "ancient")
	e2.clock.advance(365 * 24 * time.Hour)
	if got, _ := e2.svc.List(context.Background(), alice, tree, ""); len(got) != 1 {
		t.Fatalf("MaxAge=0 hid entries: %+v", got)
	}
}

func TestExpireSkipsReplacedEntry(t *testing.T) {
	e := newEnv()
	old := stage(t, e, alice, "main", "old")
	e.clock.advance(DefaultMaxAge + time.Hour)
	stage(t, e, alice, "main", "new") // re-staged after the stale read
	e.svc.expire(context.Background(), tree, old)
	got, _ := e.store.ListStaged(context.Background(), tree.ID, "main")
	if len(got) != 1 || got[0].SnapshotID != "new" {
		t.Fatalf("fresh entry was deleted by stale expiry: %+v", got)
	}
	if len(e.bus.ofType(domain.EventStagedCleared)) != 0 {
		t.Fatal("cleared event published for a replaced entry")
	}
}

// ---- ClearOnPush ---------------------------------------------------------

func TestClearOnPush(t *testing.T) {
	e := newEnv()
	stage(t, e, alice, "main", "s1", "a.go")
	stage(t, e, alice, "dev", "s2", "a.go")
	stage(t, e, bob, "main", "s3", "a.go")

	if err := e.svc.ClearOnPush(context.Background(), alice, tree, "main"); err != nil {
		t.Fatal(err)
	}
	got, _ := e.store.ListStaged(context.Background(), tree.ID, "")
	if len(got) != 2 {
		t.Fatalf("remaining = %+v", got)
	}
	for _, s := range got {
		if s.AccountID == alice.AccountID && s.BranchName == "main" {
			t.Fatal("alice's main entry not cleared")
		}
	}
	cleared := e.bus.ofType(domain.EventStagedCleared)
	if len(cleared) != 1 {
		t.Fatalf("cleared events = %d", len(cleared))
	}
	ev := cleared[0]
	c := ev.Payload.(Cleared)
	if ev.TreeID != tree.ID || ev.TenantID != tree.TenantID || ev.Actor != "alice" ||
		c.Reason != ReasonPushed || c.SnapshotID != "s1" || c.BranchName != "main" || c.AccountID != alice.AccountID {
		t.Fatalf("event = %+v payload = %+v", ev, c)
	}

	// Clearing again (nothing staged) is a silent no-op.
	if err := e.svc.ClearOnPush(context.Background(), alice, tree, "main"); err != nil {
		t.Fatalf("second clear: %v", err)
	}
	if err := e.svc.ClearOnPush(context.Background(), carol, tree, "never"); err != nil {
		t.Fatalf("clear without entry: %v", err)
	}
	if n := len(e.bus.ofType(domain.EventStagedCleared)); n != 1 {
		t.Fatalf("no-op clears published events (%d)", n)
	}
}

func TestClearOnPushErrors(t *testing.T) {
	e := newEnv()
	if err := e.svc.ClearOnPush(context.Background(), nil, tree, "main"); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("anonymous err = %v", err)
	}
	if err := e.svc.ClearOnPush(context.Background(), alice, tree, ""); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty branch err = %v", err)
	}
	if err := e.svc.ClearOnPush(context.Background(), alice, nil, "main"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("nil tree err = %v", err)
	}
	stage(t, e, alice, "main", "s1")
	e.authz.deny[domain.ActTreeWrite] = true
	if err := e.svc.ClearOnPush(context.Background(), alice, tree, "main"); !errors.Is(err, errDenied) {
		t.Fatalf("denied err = %v", err)
	}
	if e.store.count() != 1 {
		t.Fatal("denied clear deleted the entry")
	}
}

// ---- Summary -------------------------------------------------------------

func TestSummaryOverlapDetection(t *testing.T) {
	e := newEnv()
	stage(t, e, alice, "feature/oauth", "a1", "src/oauth.rs", "src/token.rs", "README.md")
	e.clock.advance(time.Minute)
	stage(t, e, bob, "feature/oauth", "b1", "src/oauth.rs", "src/other.rs")
	e.clock.advance(time.Minute)
	stage(t, e, carol, "fix/pricing", "c1", "billing/pricing.rs", "README.md")
	e.clock.advance(time.Minute)
	stage(t, e, alice, "solo", "a2", "only-alice.rs", "src/token.rs") // same author, different branch

	sum, err := e.svc.Summary(context.Background(), alice, tree)
	if err != nil {
		t.Fatal(err)
	}
	if len(sum) != 3 {
		t.Fatalf("branches = %d: %+v", len(sum), sum)
	}
	// Newest activity first.
	if sum[0].Branch != "solo" || sum[1].Branch != "fix/pricing" || sum[2].Branch != "feature/oauth" {
		t.Fatalf("order = %s,%s,%s", sum[0].Branch, sum[1].Branch, sum[2].Branch)
	}
	byName := map[string]BranchActivity{}
	for _, b := range sum {
		byName[b.Branch] = b
	}

	oauth := byName["feature/oauth"]
	if len(oauth.Authors) != 2 || oauth.Authors[0].Author != "bob" || oauth.Authors[1].Author != "alice" {
		t.Fatalf("oauth authors = %+v", oauth.Authors)
	}
	if oauth.DistinctFiles != 4 || oauth.FilesModified != 5 {
		t.Fatalf("oauth counts distinct=%d modified=%d", oauth.DistinctFiles, oauth.FilesModified)
	}
	if !oauth.LatestAt.Equal(oauth.Authors[0].StagedAt) {
		t.Fatalf("LatestAt = %v", oauth.LatestAt)
	}
	overlapPaths := func(b BranchActivity) string {
		var ps []string
		for _, o := range b.Overlaps {
			ps = append(ps, o.Path)
		}
		return strings.Join(ps, ",")
	}
	// README.md: alice (oauth) vs carol (pricing) -> cross-branch overlap.
	// src/oauth.rs: alice vs bob on the same branch.
	// src/token.rs: alice on two branches only -> NOT an overlap.
	if got := overlapPaths(oauth); got != "README.md,src/oauth.rs" {
		t.Fatalf("oauth overlaps = %q", got)
	}
	if got := overlapPaths(byName["fix/pricing"]); got != "README.md" {
		t.Fatalf("pricing overlaps = %q", got)
	}
	if got := overlapPaths(byName["solo"]); got != "" {
		t.Fatalf("solo overlaps = %q", got)
	}

	var readme FileOverlap
	for _, o := range oauth.Overlaps {
		if o.Path == "README.md" {
			readme = o
		}
	}
	want := []Contributor{
		{AccountID: alice.AccountID, Author: "alice", Branch: "feature/oauth"},
		{AccountID: carol.AccountID, Author: "carol", Branch: "fix/pricing"},
	}
	if fmt.Sprint(readme.Contributors) != fmt.Sprint(want) {
		t.Fatalf("README contributors = %+v", readme.Contributors)
	}
}

func TestSummaryAuthzEmptyAndTTL(t *testing.T) {
	e := newEnv()
	sum, err := e.svc.Summary(context.Background(), alice, tree)
	if err != nil || sum == nil || len(sum) != 0 {
		t.Fatalf("empty summary = %#v, %v", sum, err)
	}

	stage(t, e, alice, "main", "old", "x.go")
	e.clock.advance(DefaultMaxAge + time.Minute)
	stage(t, e, bob, "main", "new", "x.go")
	sum, err = e.svc.Summary(context.Background(), alice, tree)
	if err != nil {
		t.Fatal(err)
	}
	if len(sum) != 1 || len(sum[0].Authors) != 1 || sum[0].Authors[0].Author != "bob" || len(sum[0].Overlaps) != 0 {
		t.Fatalf("summary with expired entry = %+v", sum)
	}
	if e.store.count() != 1 {
		t.Fatal("expired entry not deleted by Summary")
	}

	e.authz.deny[domain.ActTreeRead] = true
	if _, err := e.svc.Summary(context.Background(), alice, tree); !errors.Is(err, errDenied) {
		t.Fatalf("err = %v", err)
	}
	if _, err := e.svc.Summary(context.Background(), alice, nil); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("nil tree err = %v", err)
	}
}

// ---- concurrency ---------------------------------------------------------

func TestConcurrentStageListClear(t *testing.T) {
	e := newEnv()
	e.svc.now = time.Now
	users := []*domain.Principal{alice, bob, carol}
	var wg sync.WaitGroup
	for g := 0; g < 24; g++ {
		wg.Add(1)
		go func(g int) {
			defer wg.Done()
			p := users[g%len(users)]
			branch := fmt.Sprintf("b%d", g%4)
			for i := 0; i < 100; i++ {
				ctx := context.Background()
				switch i % 4 {
				case 0, 1:
					s := &domain.StagedSnapshot{SnapshotID: fmt.Sprintf("s-%d-%d", g, i), BranchName: branch, FilesChanged: []string{"a.go", "b.go"}}
					if err := e.svc.Stage(ctx, p, tree, s); err != nil {
						t.Error(err)
						return
					}
				case 2:
					if _, err := e.svc.List(ctx, p, tree, ""); err != nil {
						t.Error(err)
						return
					}
					if _, err := e.svc.Summary(ctx, p, tree); err != nil {
						t.Error(err)
						return
					}
				case 3:
					if err := e.svc.ClearOnPush(ctx, p, tree, branch); err != nil {
						t.Error(err)
						return
					}
				}
			}
		}(g)
	}
	wg.Wait()
	if n := e.store.count(); n > len(users)*4 {
		t.Fatalf("store holds %d entries, more than one per (account, branch)", n)
	}
}

func TestNormalizePaths(t *testing.T) {
	tests := []struct {
		in   []string
		max  int
		want string
	}{
		{nil, 10, ""},
		{[]string{"", "  ", "/", "."}, 10, ""},
		{[]string{`a\b\c.go`, "a/b/c.go", "/a/b/c.go", "a/./b/../b/c.go"}, 10, "a/b/c.go"},
		{[]string{"c", "a", "b"}, 2, "a,b"},
		{[]string{"../escape.go"}, 10, "escape.go"},
	}
	for _, tt := range tests {
		got := normalizePaths(tt.in, tt.max)
		if got == nil {
			t.Fatalf("normalizePaths(%q) returned nil", tt.in)
		}
		if s := strings.Join(got, ","); s != tt.want {
			t.Errorf("normalizePaths(%q, %d) = %q, want %q", tt.in, tt.max, s, tt.want)
		}
	}
}
