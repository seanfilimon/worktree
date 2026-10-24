package audit

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"regexp"
	"sort"
	"strings"
	"sync"
	"testing"
	"time"

	"google.golang.org/grpc/peer"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// fakeStore implements only the audit methods of domain.MetaStore.
type fakeStore struct {
	domain.MetaStore

	mu       sync.Mutex
	events   []domain.AuditEvent
	ctxErrs  []error // ctx.Err() observed at append time
	appendFn func(ctx context.Context, e *domain.AuditEvent) error
	gate     chan struct{} // if non-nil, AppendAudit blocks until closed

	lastFilter domain.AuditFilter
	listResult []domain.AuditEvent
	listErr    error
}

func (s *fakeStore) AppendAudit(ctx context.Context, e *domain.AuditEvent) error {
	if s.gate != nil {
		<-s.gate
	}
	if s.appendFn != nil {
		if err := s.appendFn(ctx, e); err != nil {
			return err
		}
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.events = append(s.events, *e)
	s.ctxErrs = append(s.ctxErrs, ctx.Err())
	return nil
}

func (s *fakeStore) ListAudit(_ context.Context, f domain.AuditFilter) ([]domain.AuditEvent, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.lastFilter = f
	return s.listResult, s.listErr
}

func (s *fakeStore) recorded() []domain.AuditEvent {
	s.mu.Lock()
	defer s.mu.Unlock()
	return append([]domain.AuditEvent(nil), s.events...)
}

func testLogger(buf *bytes.Buffer) *slog.Logger {
	return slog.New(slog.NewTextHandler(buf, &slog.HandlerOptions{Level: slog.LevelDebug}))
}

var uuidV7 = regexp.MustCompile(`^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`)

func TestRecordDefaults(t *testing.T) {
	fixed := time.Date(2026, 3, 4, 5, 6, 7, 0, time.FixedZone("X", 3600))
	store := &fakeStore{}
	l := New(store, nil, WithClock(func() time.Time { return fixed }))

	details := map[string]string{"branch": "main", "password": "hunter2"}
	ctx := WithRemoteAddr(context.Background(), "10.0.0.1:5555")
	l.Record(ctx, domain.AuditEvent{Action: "branch.push", Details: details})

	got := store.recorded()
	if len(got) != 1 {
		t.Fatalf("recorded %d events, want 1", len(got))
	}
	e := got[0]
	if !uuidV7.MatchString(e.ID) {
		t.Errorf("ID %q is not a UUIDv7", e.ID)
	}
	if !e.At.Equal(fixed) || e.At.Location() != time.UTC {
		t.Errorf("At = %v, want %v in UTC", e.At, fixed)
	}
	if e.Actor != ActorAnonymous {
		t.Errorf("Actor = %q, want anonymous", e.Actor)
	}
	if e.RemoteAddr != "10.0.0.1" {
		t.Errorf("RemoteAddr = %q, want 10.0.0.1", e.RemoteAddr)
	}
	if e.Details["password"] != Redacted || e.Details["branch"] != "main" {
		t.Errorf("Details = %v", e.Details)
	}
	if details["password"] != "hunter2" {
		t.Errorf("caller's details map was mutated: %v", details)
	}
}

func TestRecordPreservesProvidedFields(t *testing.T) {
	store := &fakeStore{}
	l := New(store, nil)
	at := time.Date(2025, 1, 1, 0, 0, 0, 0, time.UTC)
	in := domain.AuditEvent{
		ID: "custom-id", TenantID: "t1", ActorID: "a1", Actor: "alice",
		Action: "tree.create", ResourceType: "tree", ResourceID: "tr1",
		RemoteAddr: "192.0.2.9", At: at,
	}
	l.Record(WithRemoteAddr(context.Background(), "10.9.9.9"), in)
	got := store.recorded()[0]
	if fmt.Sprint(got) != fmt.Sprint(in) {
		t.Errorf("event changed:\n got %+v\nwant %+v", got, in)
	}
}

func TestRecordSurvivesCanceledContext(t *testing.T) {
	store := &fakeStore{}
	l := New(store, nil)
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	l.Record(ctx, domain.AuditEvent{Action: "auth.logout"})
	if n := len(store.recorded()); n != 1 {
		t.Fatalf("recorded %d, want 1", n)
	}
	if err := store.ctxErrs[0]; err != nil {
		t.Errorf("store saw canceled ctx: %v", err)
	}
}

func TestRecordNilContext(t *testing.T) {
	store := &fakeStore{}
	l := New(store, nil)
	l.Record(nil, domain.AuditEvent{Action: "x"})
	if len(store.recorded()) != 1 {
		t.Fatal("event not recorded")
	}
}

func TestRecordStoreErrorIsLoggedNotReturned(t *testing.T) {
	var buf bytes.Buffer
	store := &fakeStore{appendFn: func(context.Context, *domain.AuditEvent) error {
		return errors.New("disk full")
	}}
	l := New(store, testLogger(&buf))
	l.Record(context.Background(), domain.AuditEvent{Action: "branch.push"})
	out := buf.String()
	if !strings.Contains(out, "audit: append failed") || !strings.Contains(out, "disk full") {
		t.Errorf("error not logged; log output:\n%s", out)
	}
}

func TestRecordStorePanicIsContained(t *testing.T) {
	var buf bytes.Buffer
	store := &fakeStore{appendFn: func(context.Context, *domain.AuditEvent) error { panic("boom") }}
	l := New(store, testLogger(&buf))
	l.Record(context.Background(), domain.AuditEvent{Action: "x"})
	if !strings.Contains(buf.String(), "boom") {
		t.Errorf("panic not logged: %s", buf.String())
	}
}

func TestRecordNilStore(t *testing.T) {
	var buf bytes.Buffer
	l := New(nil, testLogger(&buf))
	l.Record(context.Background(), domain.AuditEvent{Action: "x"})
	if !strings.Contains(buf.String(), "no store") {
		t.Errorf("missing log: %s", buf.String())
	}
	if _, err := l.Query(context.Background(), domain.AuditFilter{}); !errors.Is(err, ErrNoStore) {
		t.Errorf("Query err = %v, want ErrNoStore", err)
	}
}

func TestWriteTimeoutApplied(t *testing.T) {
	var deadline bool
	store := &fakeStore{appendFn: func(ctx context.Context, _ *domain.AuditEvent) error {
		_, deadline = ctx.Deadline()
		return nil
	}}
	New(store, nil, WithWriteTimeout(time.Second)).Record(context.Background(), domain.AuditEvent{})
	if !deadline {
		t.Error("expected a deadline on the append context")
	}
	New(store, nil, WithWriteTimeout(0)).Record(context.Background(), domain.AuditEvent{})
	if deadline {
		t.Error("expected no deadline when timeout disabled")
	}
}

func TestIDsAreTimeSortableAndUnique(t *testing.T) {
	g := newIDGenerator()
	base := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	var ids []string
	// Many IDs in the same millisecond (counter overflow), then a clock
	// step backwards, then forward again.
	for i := 0; i < 10000; i++ {
		ids = append(ids, g.next(base))
	}
	ids = append(ids, g.next(base.Add(-time.Hour)))
	ids = append(ids, g.next(base.Add(time.Hour)))
	seen := map[string]bool{}
	for i, id := range ids {
		if !uuidV7.MatchString(id) {
			t.Fatalf("id %d %q not UUIDv7", i, id)
		}
		if seen[id] {
			t.Fatalf("duplicate id %q", id)
		}
		seen[id] = true
		if i > 0 && ids[i-1] >= id {
			t.Fatalf("ids not increasing at %d: %q >= %q", i, ids[i-1], id)
		}
	}
	// The leading 48 bits encode the timestamp.
	later := g.next(base.Add(2 * time.Hour))
	if !strings.HasPrefix(later, fmt.Sprintf("%08x", uint64(base.Add(2*time.Hour).UnixMilli())>>16)) {
		t.Errorf("id %q does not lead with timestamp", later)
	}
}

func TestQueryLimits(t *testing.T) {
	tests := []struct {
		in, want int
	}{
		{0, DefaultQueryLimit},
		{-5, DefaultQueryLimit},
		{1, 1},
		{250, 250},
		{MaxQueryLimit, MaxQueryLimit},
		{MaxQueryLimit + 1, MaxQueryLimit},
		{1 << 30, MaxQueryLimit},
	}
	for _, tt := range tests {
		t.Run(fmt.Sprint(tt.in), func(t *testing.T) {
			store := &fakeStore{}
			l := New(store, nil)
			f := domain.AuditFilter{TenantID: "t", ActorID: "a", Action: "x", Limit: tt.in}
			evs, err := l.Query(context.Background(), f)
			if err != nil {
				t.Fatal(err)
			}
			if evs == nil {
				t.Error("Query returned nil slice, want empty")
			}
			f.Limit = tt.want
			if store.lastFilter != f {
				t.Errorf("store filter = %+v, want %+v", store.lastFilter, f)
			}
		})
	}
}

func TestQueryTruncatesAndWrapsErrors(t *testing.T) {
	store := &fakeStore{listResult: make([]domain.AuditEvent, 10)}
	l := New(store, nil)
	evs, err := l.Query(context.Background(), domain.AuditFilter{Limit: 3})
	if err != nil || len(evs) != 3 {
		t.Fatalf("got %d events, err %v; want 3", len(evs), err)
	}
	store.listErr = fmt.Errorf("wrapped: %w", domain.ErrInvalid)
	if _, err := l.Query(context.Background(), domain.AuditFilter{}); !errors.Is(err, domain.ErrInvalid) {
		t.Errorf("err = %v, want wrapping ErrInvalid", err)
	}
}

func TestAsyncCloseDrains(t *testing.T) {
	store := &fakeStore{gate: make(chan struct{})}
	l := New(store, nil, WithAsync(64))
	const n = 50
	for i := 0; i < n; i++ {
		l.Record(context.Background(), domain.AuditEvent{Action: fmt.Sprintf("a.%d", i)})
	}
	// Writer is blocked on the gate: nothing persisted yet, Record did not block.
	if got := len(store.recorded()); got != 0 {
		t.Fatalf("persisted %d before gate opened", got)
	}
	close(store.gate)
	if err := l.Close(); err != nil {
		t.Fatal(err)
	}
	got := store.recorded()
	if len(got) != n {
		t.Fatalf("after Close persisted %d, want %d", len(got), n)
	}
	for i, e := range got { // single writer preserves order
		if e.Action != fmt.Sprintf("a.%d", i) {
			t.Fatalf("event %d action %q out of order", i, e.Action)
		}
	}
	if err := l.Close(); err != nil { // idempotent
		t.Fatal(err)
	}
	// After Close, Record falls back to synchronous writes.
	l.Record(context.Background(), domain.AuditEvent{Action: "after"})
	if got := store.recorded(); len(got) != n+1 || got[n].Action != "after" {
		t.Fatalf("post-close record missing")
	}
}

func TestAsyncBufferFullFallsBackToSync(t *testing.T) {
	var buf bytes.Buffer
	gate := make(chan struct{})
	var calls sync.WaitGroup
	store := &fakeStore{}
	first := true
	var firstMu sync.Mutex
	calls.Add(1)
	store.appendFn = func(context.Context, *domain.AuditEvent) error {
		firstMu.Lock()
		isFirst := first
		first = false
		firstMu.Unlock()
		if isFirst { // block the background writer on its first event
			calls.Done()
			<-gate
		}
		return nil
	}
	l := New(store, testLogger(&buf), WithAsync(1))
	l.Record(context.Background(), domain.AuditEvent{Action: "1"}) // taken by writer, blocks
	calls.Wait()
	l.Record(context.Background(), domain.AuditEvent{Action: "2"}) // fills the buffer
	l.Record(context.Background(), domain.AuditEvent{Action: "3"}) // buffer full -> sync
	if got := store.recorded(); len(got) != 1 || got[0].Action != "3" {
		t.Fatalf("expected synchronous write of event 3, got %+v", got)
	}
	if !strings.Contains(buf.String(), "buffer full") {
		t.Errorf("expected buffer-full warning, log: %s", buf.String())
	}
	close(gate)
	_ = l.Close()
	var actions []string
	for _, e := range store.recorded() {
		actions = append(actions, e.Action)
	}
	sort.Strings(actions)
	if strings.Join(actions, ",") != "1,2,3" {
		t.Fatalf("actions = %v, want 1,2,3", actions)
	}
}

func TestAsyncConcurrentRecordAndClose(t *testing.T) {
	store := &fakeStore{}
	l := New(store, nil, WithAsync(8))
	var wg sync.WaitGroup
	const workers, per = 16, 100
	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := 0; i < per; i++ {
				l.Record(context.Background(), domain.AuditEvent{Action: "x"})
			}
		}()
	}
	wg.Wait()
	_ = l.Close()
	if got := len(store.recorded()); got != workers*per {
		t.Fatalf("persisted %d, want %d", got, workers*per)
	}
}

func TestSyncCloseNoop(t *testing.T) {
	if err := New(&fakeStore{}, nil).Close(); err != nil {
		t.Fatal(err)
	}
	if err := New(&fakeStore{}, nil, WithAsync(0)).Close(); err != nil {
		t.Fatal(err)
	}
}

func TestIsSensitiveKey(t *testing.T) {
	tests := []struct {
		key  string
		want bool
	}{
		{"password", true},
		{"Password", true},
		{"new_password", true},
		{"newPassword", true},
		{"old-passwd", true},
		{"pwd", true},
		{"token", true},
		{"access_token", true},
		{"refreshToken", true},
		{"accesstoken", true},
		{"secret", true},
		{"client_secret", true},
		{"secretValue", true},
		{"Authorization", true},
		{"api_key", true},
		{"X-Api-Key", true},
		{"APIKey", true},
		{"apikey", true},
		{"private_key", true},
		{"credentials", true},
		{"cookie", true},
		{"token_id", false},
		{"token_name", false},
		{"tokenId", false},
		{"branch", false},
		{"key", false},
		{"public_key_fingerprint", false},
		{"username", false},
		{"", false},
		{"---", false},
	}
	for _, tt := range tests {
		if got := IsSensitiveKey(tt.key); got != tt.want {
			t.Errorf("IsSensitiveKey(%q) = %v, want %v (words %q)", tt.key, got, tt.want, splitWords(tt.key))
		}
	}
}

func TestRedact(t *testing.T) {
	if Redact(nil) != nil || Redact(map[string]string{}) != nil {
		t.Error("empty input should yield nil")
	}
	in := map[string]string{
		"token":  "abc",
		"header": "Bearer xyz123",
		"bare":   "bearer",
		"branch": "main",
	}
	out := Redact(in)
	want := map[string]string{"token": Redacted, "header": Redacted, "bare": "bearer", "branch": "main"}
	if fmt.Sprint(out) != fmt.Sprint(want) {
		t.Errorf("Redact = %v, want %v", out, want)
	}
	if in["token"] != "abc" {
		t.Error("input mutated")
	}
}

func TestFromPrincipal(t *testing.T) {
	d := map[string]string{"k": "v"}
	e := FromPrincipal(&domain.Principal{AccountID: "acc1", Username: "alice", TenantID: "personal"},
		ActionBranchPush, "branch", "tree1/main", d)
	if e.ActorID != "acc1" || e.Actor != "alice" || e.Action != ActionBranchPush ||
		e.ResourceType != "branch" || e.ResourceID != "tree1/main" || e.TenantID != "" {
		t.Errorf("unexpected event %+v", e)
	}
	d["k"] = "changed"
	if e.Details["k"] != "v" {
		t.Error("details not copied")
	}

	anon := FromPrincipal(nil, "auth.login_failed", "account", "bob", nil)
	if anon.Actor != ActorAnonymous || anon.ActorID != "" || anon.Details != nil {
		t.Errorf("unexpected anonymous event %+v", anon)
	}
	if sys := System("server.start", "", "", nil); sys.Actor != ActorSystem {
		t.Errorf("System actor = %q", sys.Actor)
	}
}

func TestRemoteAddr(t *testing.T) {
	bg := context.Background()
	tests := []struct {
		name string
		ctx  context.Context
		want string
	}{
		{"none", bg, ""},
		{"host:port", WithRemoteAddr(bg, "203.0.113.5:443"), "203.0.113.5"},
		{"ipv6", WithRemoteAddr(bg, "[2001:db8::1]:8080"), "2001:db8::1"},
		{"bare host", WithRemoteAddr(bg, "203.0.113.5"), "203.0.113.5"},
		{"grpc peer", peer.NewContext(bg, &peer.Peer{Addr: &net.TCPAddr{IP: net.ParseIP("198.51.100.7"), Port: 9}}), "198.51.100.7"},
		{"explicit beats peer", WithRemoteAddr(peer.NewContext(bg, &peer.Peer{Addr: &net.TCPAddr{IP: net.ParseIP("198.51.100.7")}}), "10.1.1.1:1"), "10.1.1.1"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if got := RemoteAddr(tt.ctx); got != tt.want {
				t.Errorf("RemoteAddr = %q, want %q", got, tt.want)
			}
		})
	}
}

func TestMiddleware(t *testing.T) {
	var got string
	h := Middleware(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got = RemoteAddr(r.Context())
	}))
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.RemoteAddr = "192.0.2.44:12345"
	req.Header.Set("X-Forwarded-For", "1.2.3.4")
	h.ServeHTTP(httptest.NewRecorder(), req)
	if got != "192.0.2.44" {
		t.Errorf("remote addr = %q, want 192.0.2.44", got)
	}
}
