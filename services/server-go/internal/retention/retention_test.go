package retention

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/policyhist"
)

type fakeHooks struct{ n int }

func (f *fakeHooks) PruneDeliveries(context.Context) (int, error) { return f.n, nil }

type fakeBackups struct {
	ids []string
	err error
}

func (f *fakeBackups) PruneAudited(context.Context) ([]string, error) { return f.ids, f.err }

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

func (a *recAudit) count() int {
	a.mu.Lock()
	defer a.mu.Unlock()
	return len(a.events)
}

var quiet = slog.New(slog.NewTextHandler(io.Discard, nil))

func TestRunAndStatus(t *testing.T) {
	ctx := context.Background()
	store := meta.NewMemory()
	// Two policy versions (the older one is pruned by keep 1) and one old
	// audit event.
	rec := policyhist.New(store, nil, quiet)
	p := domain.Policy{ID: "p1", TenantID: "t1", Name: "x", Enabled: true}
	for _, change := range []string{policyhist.ChangeCreate, policyhist.ChangeUpdate} {
		if _, err := rec.Record(ctx, policyhist.Entry{Change: change, Actor: "ada", Policy: p}); err != nil {
			t.Fatal(err)
		}
	}
	if err := store.AppendAudit(ctx, &domain.AuditEvent{ID: "ev-1", At: time.Now().Add(-48 * time.Hour), Action: "x", Actor: "ada"}); err != nil {
		t.Fatal(err)
	}
	audit := &recAudit{}
	backups := &fakeBackups{ids: []string{"bk-1"}}
	svc := New(Deps{Store: store, Audit: audit, Logger: quiet, Webhooks: &fakeHooks{n: 4}, Backups: backups}, Config{
		PolicyVersions:    Rule{Keep: 1},
		Audit:             24 * time.Hour,
		WebhookDeliveries: Rule{MaxAge: 720 * time.Hour, Keep: 200},
		Backups:           Rule{MaxAge: 72 * time.Hour, Keep: 7},
	}, 0)

	st := svc.Status()
	if st.Interval != "1h0m0s" || st.LastRunAt != nil || st.Running || st.Config.PolicyVersions.Keep != 1 ||
		st.Config.Audit.MaxAgeSeconds != 86400 || st.Config.WebhookDeliveries.Keep != 200 || st.Config.Backups.MaxAge != "72h0m0s" {
		t.Fatalf("status = %+v", st)
	}
	rep, err := svc.Run(ctx)
	if err != nil {
		t.Fatal(err)
	}
	if rep.PolicyVersionsPruned != 1 || rep.AuditPruned != 1 || rep.WebhookDeliveriesPruned != 4 || rep.BackupsPruned != 1 {
		t.Fatalf("report = %+v", rep)
	}
	if audit.count() != 1 || audit.events[0].Action != ActionRun || audit.events[0].Details["audit_pruned"] != "1" {
		t.Fatalf("audit = %+v", audit.events)
	}
	st = svc.Status()
	if st.LastRunAt == nil || st.LastReport == nil || st.LastReport.WebhookDeliveriesPruned != 4 || st.LastError != "" {
		t.Fatalf("status after run = %+v", st)
	}

	// A failing category is reported; the others still run. Nothing pruned
	// means no audit event.
	backups.ids, backups.err = nil, errors.New("bucket gone")
	svc.d.Webhooks = &fakeHooks{}
	rep, err = svc.Run(ctx)
	if err == nil || rep == nil || rep.PolicyVersionsPruned != 0 {
		t.Fatalf("failing run = %+v, %v", rep, err)
	}
	if st := svc.Status(); st.LastError == "" || audit.count() != 1 {
		t.Fatalf("status after failure = %+v (audit %d)", st, audit.count())
	}

	// Concurrent runs are refused.
	svc.run.Lock()
	if _, err := svc.Run(ctx); !errors.Is(err, ErrRunning) {
		t.Fatalf("concurrent run = %v", err)
	}
	if !svc.Status().Running {
		t.Fatal("running not reported")
	}
	svc.run.Unlock()
}

func TestStartRunsImmediately(t *testing.T) {
	hooks := &countingHooks{}
	svc := New(Deps{Store: meta.NewMemory(), Logger: quiet, Webhooks: hooks}, Config{}, 20*time.Millisecond)
	stop := svc.Start(context.Background())
	deadline := time.Now().Add(5 * time.Second)
	for hooks.calls() < 2 {
		if time.Now().After(deadline) {
			t.Fatal("no periodic passes")
		}
		time.Sleep(5 * time.Millisecond)
	}
	stop()
	n := hooks.calls()
	time.Sleep(50 * time.Millisecond)
	if hooks.calls() != n {
		t.Fatal("passes after stop")
	}
}

type countingHooks struct {
	mu sync.Mutex
	n  int
}

func (c *countingHooks) PruneDeliveries(context.Context) (int, error) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.n++
	return 0, nil
}

func (c *countingHooks) calls() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.n
}
