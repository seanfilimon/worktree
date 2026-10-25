package ci

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

var ctx = context.Background()

// grantAuthz allows the listed actions per username.
type grantAuthz map[string][]domain.Action

func (g grantAuthz) Authorize(_ context.Context, p *domain.Principal, act domain.Action, _ string, _ *domain.Tree) error {
	if p == nil {
		return fmt.Errorf("sign in: %w", domain.ErrUnauthenticated)
	}
	if !slices.Contains(g[p.Username], act) {
		return fmt.Errorf("%s may not %s: %w", p.Username, act, domain.ErrForbidden)
	}
	return nil
}

func (grantAuthz) RoleOf(context.Context, string, string) (string, error) { return "", nil }

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

var (
	bot    = &domain.Principal{AccountID: "acc-bot", Username: "bot"}
	viewer = &domain.Principal{AccountID: "acc-viewer", Username: "viewer"}
	tree   = &domain.Tree{ID: "tree-1", TenantID: "ten-1", Name: "web"}
)

func newService(t *testing.T) (*Service, *meta.Memory, *events.Bus, *recAudit) {
	t.Helper()
	store := meta.NewMemory()
	for _, s := range []domain.Snapshot{
		{ID: "snap-1", TreeID: tree.ID},
		{ID: "snap-2", TreeID: tree.ID},
		{ID: "snap-x", TreeID: "tree-other"},
	} {
		if err := store.PutSnapshot(ctx, &s); err != nil {
			t.Fatal(err)
		}
	}
	bus := events.NewBus()
	audit := &recAudit{}
	svc := New(Deps{
		Store: store,
		Authz: grantAuthz{"bot": {domain.ActCIReport, domain.ActTreeRead}, "viewer": {domain.ActTreeRead}},
		Bus:   bus,
		Audit: audit,
	})
	svc.now = func() time.Time { return time.Date(2026, 10, 25, 0, 0, 0, 0, time.UTC) }
	return svc, store, bus, audit
}

func TestCombine(t *testing.T) {
	st := func(states ...string) []domain.CIStatus {
		out := []domain.CIStatus{}
		for _, s := range states {
			out = append(out, domain.CIStatus{State: s})
		}
		return out
	}
	tests := []struct {
		in   []domain.CIStatus
		want string
	}{
		{nil, ""},
		{st(domain.CISuccess), domain.CISuccess},
		{st(domain.CISuccess, domain.CIPending), domain.CIPending},
		{st(domain.CIPending, domain.CIError, domain.CISuccess), domain.CIFailure},
		{st(domain.CIFailure), domain.CIFailure},
	}
	for _, tt := range tests {
		if got := Combine(tt.in); got != tt.want {
			t.Errorf("Combine(%v) = %q, want %q", tt.in, got, tt.want)
		}
	}
}

func TestReport(t *testing.T) {
	t.Run("validation and authorization", func(t *testing.T) {
		svc, _, _, _ := newService(t)
		ok := Report{Snapshot: "snap-1", Context: "ci/build", State: domain.CISuccess}
		tests := []struct {
			name string
			p    *domain.Principal
			edit func(r *Report)
			want error
		}{
			{"anonymous", nil, nil, domain.ErrUnauthenticated},
			{"viewer", viewer, nil, domain.ErrForbidden},
			{"no snapshot", bot, func(r *Report) { r.Snapshot = "" }, domain.ErrInvalid},
			{"bad state", bot, func(r *Report) { r.State = "green" }, domain.ErrInvalid},
			{"empty context", bot, func(r *Report) { r.Context = " " }, domain.ErrInvalid},
			{"control char", bot, func(r *Report) { r.Context = "a\x00b" }, domain.ErrInvalid},
			{"long context", bot, func(r *Report) { r.Context = strings.Repeat("c", MaxContextLen+1) }, domain.ErrInvalid},
			{"long description", bot, func(r *Report) { r.Description = strings.Repeat("d", MaxDescriptionLen+1) }, domain.ErrInvalid},
			{"bad url", bot, func(r *Report) { r.TargetURL = "javascript:alert(1)" }, domain.ErrInvalid},
			{"unknown snapshot", bot, func(r *Report) { r.Snapshot = "nope" }, domain.ErrNotFound},
			{"foreign snapshot", bot, func(r *Report) { r.Snapshot = "snap-x" }, domain.ErrNotFound},
		}
		for _, tt := range tests {
			t.Run(tt.name, func(t *testing.T) {
				r := ok
				if tt.edit != nil {
					tt.edit(&r)
				}
				if _, err := svc.Report(ctx, tt.p, tree, r); !errors.Is(err, tt.want) {
					t.Fatalf("err = %v, want %v", err, tt.want)
				}
			})
		}
	})

	t.Run("stores, replaces, publishes and audits", func(t *testing.T) {
		svc, _, bus, audit := newService(t)
		ch, cancel := bus.Subscribe(domain.EventFilter{Types: []string{domain.EventCIStatus}})
		defer cancel()
		st, err := svc.Report(ctx, bot, tree, Report{Snapshot: "snap-1", Context: "build", State: domain.CIPending, TargetURL: "https://ci.example/1"})
		if err != nil {
			t.Fatal(err)
		}
		if st.Reporter != "bot" || st.TreeID != tree.ID || st.SnapshotID != "snap-1" {
			t.Fatalf("status = %+v", st)
		}
		select {
		case e := <-ch:
			if e.TreeID != tree.ID || e.Payload.(domain.CIStatus).Context != "build" {
				t.Fatalf("event = %+v", e)
			}
		case <-time.After(time.Second):
			t.Fatal("no ci.status event")
		}
		if _, err := svc.Report(ctx, bot, tree, Report{Snapshot: "snap-1", Context: "build", State: domain.CISuccess}); err != nil {
			t.Fatal(err)
		}
		if _, err := svc.Report(ctx, bot, tree, Report{Snapshot: "snap-1", Context: "test", State: domain.CIFailure, Description: "2 failed"}); err != nil {
			t.Fatal(err)
		}
		if _, err := svc.Report(ctx, bot, tree, Report{Snapshot: "snap-2", Context: "build", State: domain.CISuccess}); err != nil {
			t.Fatal(err)
		}
		got, err := svc.Status(ctx, viewer, tree, "snap-1")
		if err != nil {
			t.Fatal(err)
		}
		if got.State != domain.CIFailure || len(got.Checks) != 2 || got.Checks[0].Context != "build" || got.Checks[0].State != domain.CISuccess {
			t.Fatalf("status = %+v", got)
		}
		if got, _ := svc.Status(ctx, viewer, tree, "snap-2"); got.State != domain.CISuccess || len(got.Checks) != 1 {
			t.Fatalf("snap-2 = %+v", got)
		}
		if got, _ := svc.Status(ctx, viewer, tree, "snap-none"); got.State != "" || len(got.Checks) != 0 || got.Checks == nil {
			t.Fatalf("empty = %+v", got)
		}
		if _, err := svc.Status(ctx, nil, tree, "snap-1"); !errors.Is(err, domain.ErrUnauthenticated) {
			t.Fatalf("anonymous status err = %v", err)
		}
		if len(audit.events) != 4 || audit.events[0].Action != "ci.report" {
			t.Fatalf("audit = %+v", audit.events)
		}
	})
}

func TestTreeDeletedRemovesStatuses(t *testing.T) {
	svc, store, bus, _ := newService(t)
	cctx, cancel := context.WithCancel(ctx)
	defer cancel()
	svc.Start(cctx)
	if _, err := svc.Report(ctx, bot, tree, Report{Snapshot: "snap-1", Context: "build", State: domain.CISuccess}); err != nil {
		t.Fatal(err)
	}
	if err := store.PutDoc(ctx, Collection, domain.DocKey("tree-10", "s", "c"), domain.CIStatus{}); err != nil {
		t.Fatal(err)
	}
	bus.Publish(domain.Event{Type: domain.EventTreeDeleted, TreeID: tree.ID})
	deadline := time.Now().Add(5 * time.Second)
	for {
		left, _ := store.ListDocs(ctx, Collection, meta.DocPrefix(tree.ID))
		if len(left) == 0 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("statuses not removed")
		}
		time.Sleep(10 * time.Millisecond)
	}
	if other, _ := store.ListDocs(ctx, Collection, meta.DocPrefix("tree-10")); len(other) != 1 {
		t.Fatalf("other tree's statuses = %d", len(other))
	}
}
