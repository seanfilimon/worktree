package httpapi

// server-ops (docs/ENGINEERING_PLAN.md §16.11): email outbox and
// retention routes, and backups kept in an S3 bucket.

import (
	"bytes"
	"context"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/notify"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/retention"
)

// server-ops (§16.11): component schemas checked by
// TestOpenAPISchemasMatchGoTypes.
func init() {
	openAPITypes["OutboxEmail"] = notify.OutboxEmail{}
	openAPITypes["OutboxCounts"] = notify.OutboxCounts{}
	openAPITypes["OutboxList"] = notify.OutboxList{}
	openAPITypes["RetryDeadEmailsResponse"] = RetryDeadResponse{}
	openAPITypes["RetentionStatus"] = retention.Status{}
	openAPITypes["RetentionConfig"] = retention.ConfigInfo{}
	openAPITypes["RetentionRule"] = retention.RuleInfo{}
	openAPITypes["RetentionAge"] = retention.AgeInfo{}
	openAPITypes["RetentionReport"] = retention.Report{}
}

// outboxFixture has a notify service whose outbox holds a pending, a sent
// and two dead emails.
func outboxFixture(t *testing.T) (*fixture, *notify.Service) {
	t.Helper()
	ctx := context.Background()
	store := meta.NewMemory()
	at := time.Date(2026, 10, 26, 10, 0, 0, 0, time.UTC)
	next := at.Add(time.Minute)
	for i, e := range []notify.OutboxEmail{
		{ID: "em-20261026T100000.000000001Z-0000000a", Status: notify.OutboxPending, To: "a@example.com", Subject: "pending", Attempts: 1, LastError: "dial tcp: refused", NextAttemptAt: &next},
		{ID: "em-20261026T100000.000000002Z-0000000b", Status: notify.OutboxSent, To: "b@example.com", Subject: "sent", Attempts: 1, SentAt: &at},
		{ID: "em-20261026T100000.000000003Z-0000000c", Status: notify.OutboxDead, To: "c@example.com", Subject: "dead 1", Attempts: 9, DeadAt: &at},
		{ID: "em-20261026T100000.000000004Z-0000000d", Status: notify.OutboxDead, To: "d@example.com", Subject: "dead 2", Attempts: 9, DeadAt: &at},
	} {
		e.CreatedAt, e.UpdatedAt, e.DeadlineAt = at.Add(time.Duration(i)), at, at.Add(24*time.Hour)
		must(t, store.PutDoc(ctx, notify.OutboxCollection, e.ID, e))
	}
	svc := notify.New(notify.Deps{Store: store, Logger: quietLog})
	return newFixture(t, func(d *Deps) { d.Notify = svc }), svc
}

func TestServerOpsRoutesRequireServerAdmin(t *testing.T) {
	f, _ := outboxFixture(t)
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, "/api/v1/admin/outbox"},
		{http.MethodPost, "/api/v1/admin/outbox/retry"},
		{http.MethodPost, "/api/v1/admin/outbox/em-20261026T100000.000000001Z-0000000a/retry"},
		{http.MethodDelete, "/api/v1/admin/outbox/em-20261026T100000.000000001Z-0000000a"},
		{http.MethodGet, "/api/v1/admin/retention"},
		{http.MethodPost, "/api/v1/admin/retention/run"},
	} {
		if rec := f.do(t, tc.method, tc.path, "", nil); rec.Code != http.StatusUnauthorized {
			t.Errorf("%s %s anonymous = %d", tc.method, tc.path, rec.Code)
		}
		if rec := f.do(t, tc.method, tc.path, "alice", nil); rec.Code != http.StatusForbidden {
			t.Errorf("%s %s as tenant owner = %d", tc.method, tc.path, rec.Code)
		}
	}
}

func TestOutboxRoutes(t *testing.T) {
	f, _ := outboxFixture(t)
	rec := f.do(t, http.MethodGet, "/api/v1/admin/outbox", "admin", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("list = %d: %s", rec.Code, rec.Body.String())
	}
	l := decode[notify.OutboxList](t, rec)
	if l.Enabled || l.Counts != (notify.OutboxCounts{Pending: 1, Sent: 1, Dead: 2}) || len(l.Emails) != 4 ||
		l.Emails[0].Subject != "dead 2" || l.DeadAfter != "24h0m0s" {
		t.Fatalf("list = %+v", l)
	}
	if bytes.Contains(rec.Body.Bytes(), []byte(`"html"`)) {
		t.Fatalf("bodies leaked: %s", rec.Body.String())
	}
	rec = f.do(t, http.MethodGet, "/api/v1/admin/outbox?status=dead&limit=1", "admin", nil)
	if l := decode[notify.OutboxList](t, rec); len(l.Emails) != 1 || l.Emails[0].Status != notify.OutboxDead || l.Counts.Dead != 2 {
		t.Fatalf("filtered = %+v", l)
	}
	for _, q := range []string{"?status=nope", "?limit=0", "?limit=x"} {
		if rec := f.do(t, http.MethodGet, "/api/v1/admin/outbox"+q, "admin", nil); rec.Code != http.StatusBadRequest {
			t.Errorf("GET outbox%s = %d", q, rec.Code)
		}
	}

	// Retry one dead email, then all remaining dead ones.
	rec = f.do(t, http.MethodPost, "/api/v1/admin/outbox/em-20261026T100000.000000003Z-0000000c/retry", "admin", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("retry = %d: %s", rec.Code, rec.Body.String())
	}
	if e := decode[notify.OutboxEmail](t, rec); e.Status != notify.OutboxPending || e.DeadAt != nil || e.NextAttemptAt == nil {
		t.Fatalf("retried = %+v", e)
	}
	hasAudit(t, f, "outbox.retry")
	rec = f.do(t, http.MethodPost, "/api/v1/admin/outbox/retry", "admin", nil)
	if rec.Code != http.StatusOK || decode[RetryDeadResponse](t, rec).Retried != 1 {
		t.Fatalf("retry dead = %d: %s", rec.Code, rec.Body.String())
	}
	rec = f.do(t, http.MethodPost, "/api/v1/admin/outbox/em-20261026T100000.000000002Z-0000000b/retry", "admin", nil)
	if rec.Code != http.StatusConflict || decode[errorBody](t, rec).Error.Code != "already_sent" {
		t.Fatalf("retry sent = %d: %s", rec.Code, rec.Body.String())
	}
	if rec := f.do(t, http.MethodPost, "/api/v1/admin/outbox/em-nope/retry", "admin", nil); rec.Code != http.StatusNotFound {
		t.Fatalf("retry unknown = %d", rec.Code)
	}

	// Delete.
	if rec := f.do(t, http.MethodDelete, "/api/v1/admin/outbox/em-20261026T100000.000000002Z-0000000b", "admin", nil); rec.Code != http.StatusNoContent {
		t.Fatalf("delete = %d: %s", rec.Code, rec.Body.String())
	}
	hasAudit(t, f, "outbox.delete")
	if rec := f.do(t, http.MethodDelete, "/api/v1/admin/outbox/em-20261026T100000.000000002Z-0000000b", "admin", nil); rec.Code != http.StatusNotFound {
		t.Fatalf("second delete = %d", rec.Code)
	}
	rec = f.do(t, http.MethodGet, "/api/v1/admin/outbox", "admin", nil)
	if l := decode[notify.OutboxList](t, rec); l.Counts != (notify.OutboxCounts{Pending: 3}) {
		t.Fatalf("after changes = %+v", l.Counts)
	}
}

func TestRetentionRoutes(t *testing.T) {
	ret := retention.New(retention.Deps{Store: meta.NewMemory(), Logger: quietLog}, retention.Config{
		PolicyVersions: retention.Rule{MaxAge: 90 * 24 * time.Hour, Keep: 100}, Audit: 720 * time.Hour,
	}, 0)
	f := newFixture(t, func(d *Deps) { d.Retention = ret })
	rec := f.do(t, http.MethodGet, "/api/v1/admin/retention", "admin", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d: %s", rec.Code, rec.Body.String())
	}
	st := decode[retention.Status](t, rec)
	if st.Interval != "1h0m0s" || st.Config.PolicyVersions.Keep != 100 || st.Config.Audit.MaxAge != "720h0m0s" || st.LastRunAt != nil {
		t.Fatalf("status = %+v", st)
	}
	rec = f.do(t, http.MethodPost, "/api/v1/admin/retention/run", "admin", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("run = %d: %s", rec.Code, rec.Body.String())
	}
	if rep := decode[retention.Report](t, rec); rep.At.IsZero() || rep.AuditPruned != 0 {
		t.Fatalf("report = %+v", rep)
	}
	if st := decode[retention.Status](t, f.do(t, http.MethodGet, "/api/v1/admin/retention", "admin", nil)); st.LastReport == nil {
		t.Fatalf("status after run = %+v", st)
	}
}

func TestServerOpsNotImplemented(t *testing.T) {
	f := newFixture(t)
	for _, tc := range []struct{ method, path string }{
		{http.MethodGet, "/api/v1/admin/outbox"},
		{http.MethodPost, "/api/v1/admin/outbox/retry"},
		{http.MethodGet, "/api/v1/admin/retention"},
		{http.MethodPost, "/api/v1/admin/retention/run"},
	} {
		if rec := f.do(t, tc.method, tc.path, "admin", nil); rec.Code != http.StatusNotImplemented {
			t.Errorf("%s %s = %d", tc.method, tc.path, rec.Code)
		}
	}
}

func TestBackupRoutesS3Target(t *testing.T) {
	ctx := context.Background()
	fake := objstore.NewFakeS3()
	srv := httptest.NewServer(fake)
	defer srv.Close()
	b, err := objstore.NewS3Bucket(ctx, objstore.S3Options{Endpoint: srv.URL, Bucket: "wtbk", Region: "us-east-1", AccessKey: "a", SecretKey: "b"})
	must(t, err)
	dir := t.TempDir()
	db, err := meta.NewBolt(filepath.Join(dir, "meta.db"))
	must(t, err)
	t.Cleanup(func() { _ = db.Close() })
	objs := objstore.New(dir)
	data := []byte("backed up object")
	must(t, objs.Put(ctx, hash.Sum(data), data))
	svc := backup.New(backup.Deps{Store: db, Objects: objs, Logger: quietLog})
	svc.Configure(backup.Options{Dir: filepath.Join(dir, "backups"), Target: backup.NewS3Target(b, "wt/backups"), MaxAge: 48 * time.Hour})
	f := newFixture(t, func(d *Deps) { d.Backup = svc })

	rec := f.do(t, http.MethodPost, "/api/v1/admin/backups", "admin", nil)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create = %d: %s", rec.Code, rec.Body.String())
	}
	info := decode[domain.BackupInfo](t, rec)
	if info.Path != "s3://wtbk/wt/backups/"+info.ID+".wtbak" {
		t.Fatalf("path = %s", info.Path)
	}
	st := decode[backup.Status](t, f.do(t, http.MethodGet, "/api/v1/admin/backups", "admin", nil))
	if st.Config.Target != "s3" || st.Config.Location != "s3://wtbk/wt/backups/" || st.Config.MaxAge != "48h0m0s" ||
		st.Config.MaxAgeSeconds != 172800 || len(st.Backups) != 1 {
		t.Fatalf("status = %+v", st)
	}
	rec = f.do(t, http.MethodGet, "/api/v1/admin/backups/"+info.ID+"/download", "admin", nil)
	stored, _ := fake.Object("wtbk", "wt/backups/"+info.ID+".wtbak")
	if rec.Code != http.StatusOK || !bytes.Equal(rec.Body.Bytes(), stored) || rec.Header().Get("Content-Length") == "" {
		t.Fatalf("download = %d, %d bytes (stored %d)", rec.Code, rec.Body.Len(), len(stored))
	}
	if rec := f.do(t, http.MethodDelete, "/api/v1/admin/backups/"+info.ID, "admin", nil); rec.Code != http.StatusNoContent {
		t.Fatalf("delete = %d", rec.Code)
	}
	if _, ok := fake.Object("wtbk", "wt/backups/"+info.ID+".wtbak"); ok {
		t.Fatal("object kept after delete")
	}
}
