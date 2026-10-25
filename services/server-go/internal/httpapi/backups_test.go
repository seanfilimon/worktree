package httpapi

import (
	"bytes"
	"context"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// storage-backends (§15.2): component schemas checked by
// TestOpenAPISchemasMatchGoTypes (registered here to keep
// openapi_schema_test.go conflict-free).
func init() {
	openAPITypes["BackupInfo"] = domain.BackupInfo{}
	openAPITypes["BackupStatus"] = backup.Status{}
	openAPITypes["BackupConfig"] = backup.Config{}
	openAPITypes["BackupStoreInfo"] = backup.StoreInfo{}
	openAPITypes["CreateBackupRequest"] = CreateBackupRequest{}
}

// backupFixture is the standard fixture with a real backup service over a
// bbolt store and a disk object store.
func backupFixture(t *testing.T) (*fixture, *backup.Service) {
	t.Helper()
	dir := t.TempDir()
	db, err := meta.NewBolt(filepath.Join(dir, "meta.db"))
	must(t, err)
	t.Cleanup(func() { _ = db.Close() })
	objs := objstore.New(dir)
	data := []byte("backed up object")
	must(t, objs.Put(context.Background(), hash.Sum(data), data))
	svc := backup.New(backup.Deps{Store: db, Objects: objs, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	svc.Configure(backup.Options{Dir: filepath.Join(dir, "backups"), Keep: 3, Version: "test"})
	f := newFixture(t, func(d *Deps) { d.Backup = svc })
	return f, svc
}

func TestBackupRoutesRequireServerAdmin(t *testing.T) {
	f, _ := backupFixture(t)
	for _, tc := range []struct{ method, path string }{
		{http.MethodPost, "/api/v1/admin/backups"},
		{http.MethodGet, "/api/v1/admin/backups"},
		{http.MethodGet, "/api/v1/admin/backups/bk-20260101T000000Z-abcdef/download"},
		{http.MethodDelete, "/api/v1/admin/backups/bk-20260101T000000Z-abcdef"},
	} {
		if rec := f.do(t, tc.method, tc.path, "", nil); rec.Code != http.StatusUnauthorized {
			t.Errorf("%s %s anonymous = %d", tc.method, tc.path, rec.Code)
		}
		if rec := f.do(t, tc.method, tc.path, "alice", nil); rec.Code != http.StatusForbidden {
			t.Errorf("%s %s as tenant owner = %d", tc.method, tc.path, rec.Code)
		}
	}
}

func TestBackupLifecycle(t *testing.T) {
	f, _ := backupFixture(t)

	rec := f.do(t, http.MethodPost, "/api/v1/admin/backups", "admin", CreateBackupRequest{Note: "pre-upgrade"})
	if rec.Code != http.StatusCreated {
		t.Fatalf("create = %d: %s", rec.Code, rec.Body.String())
	}
	b := decode[domain.BackupInfo](t, rec)
	if !backup.ValidID(b.ID) || b.Note != "pre-upgrade" || b.CreatedBy != "admin" || b.Objects != 1 || b.SizeBytes == 0 {
		t.Fatalf("backup = %+v", b)
	}
	hasAudit(t, f, backup.ActionCreate)

	// The body is optional.
	if rec := f.do(t, http.MethodPost, "/api/v1/admin/backups", "admin", nil); rec.Code != http.StatusCreated {
		t.Fatalf("create without body = %d: %s", rec.Code, rec.Body.String())
	}
	if rec := f.do(t, http.MethodPost, "/api/v1/admin/backups", "admin", `{"bogus":1}`); rec.Code != http.StatusBadRequest {
		t.Fatalf("unknown field = %d", rec.Code)
	}
	if rec := f.do(t, http.MethodPost, "/api/v1/admin/backups", "admin", CreateBackupRequest{Note: strings.Repeat("n", 501)}); rec.Code != http.StatusBadRequest {
		t.Fatalf("long note = %d", rec.Code)
	}

	rec = f.do(t, http.MethodGet, "/api/v1/admin/backups", "admin", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("list = %d: %s", rec.Code, rec.Body.String())
	}
	st := decode[backup.Status](t, rec)
	if len(st.Backups) != 2 || st.Backups[1].ID != b.ID || !st.Supported || st.Running ||
		st.Store.Kind != "disk" || !st.Store.Compress || st.Config.Keep != 3 || st.Config.Interval != "0s" {
		t.Fatalf("status = %+v", st)
	}
	if !strings.Contains(rec.Body.String(), `"backups":[{`) || !strings.Contains(rec.Body.String(), `"store":{"kind":"disk"`) {
		t.Fatalf("body = %s", rec.Body.String())
	}

	// Download (and a range of it).
	rec = f.do(t, http.MethodGet, "/api/v1/admin/backups/"+b.ID+"/download", "admin", nil)
	if rec.Code != http.StatusOK {
		t.Fatalf("download = %d: %s", rec.Code, rec.Body.String())
	}
	if cd := rec.Header().Get("Content-Disposition"); cd != `attachment; filename="`+b.ID+`.wtbak"` {
		t.Errorf("Content-Disposition = %q", cd)
	}
	if !bytes.HasPrefix(rec.Body.Bytes(), []byte{0x28, 0xb5, 0x2f, 0xfd}) || int64(rec.Body.Len()) != b.SizeBytes {
		t.Fatalf("download body: %d bytes, want a %d byte zstd stream", rec.Body.Len(), b.SizeBytes)
	}
	req := httptest.NewRequest(http.MethodGet, "/api/v1/admin/backups/"+b.ID+"/download", nil)
	req.Header.Set("Authorization", "Bearer admin")
	req.Header.Set("Range", "bytes=0-3")
	rr := httptest.NewRecorder()
	f.h.ServeHTTP(rr, req)
	if rr.Code != http.StatusPartialContent || rr.Body.Len() != 4 {
		t.Fatalf("range download = %d, %d bytes", rr.Code, rr.Body.Len())
	}

	// Delete.
	if rec := f.do(t, http.MethodDelete, "/api/v1/admin/backups/"+b.ID, "admin", nil); rec.Code != http.StatusNoContent {
		t.Fatalf("delete = %d: %s", rec.Code, rec.Body.String())
	}
	hasAudit(t, f, backup.ActionDelete)
	for _, p := range []string{"/api/v1/admin/backups/" + b.ID + "/download", "/api/v1/admin/backups/..%2Fmeta.db/download"} {
		if rec := f.do(t, http.MethodGet, p, "admin", nil); rec.Code != http.StatusNotFound {
			t.Errorf("GET %s = %d", p, rec.Code)
		}
	}
	if rec := f.do(t, http.MethodDelete, "/api/v1/admin/backups/"+b.ID, "admin", nil); rec.Code != http.StatusNotFound {
		t.Fatalf("second delete = %d", rec.Code)
	}
}

func TestBackupsNotImplemented(t *testing.T) {
	f := newFixture(t) // no backup service
	if rec := f.do(t, http.MethodGet, "/api/v1/admin/backups", "admin", nil); rec.Code != http.StatusNotImplemented {
		t.Fatalf("list without service = %d", rec.Code)
	}
	// A service whose stores cannot be backed up (memory metadata).
	svc := backup.New(backup.Deps{Store: meta.NewMemory(), Objects: objstore.New(t.TempDir())})
	svc.Configure(backup.Options{Dir: t.TempDir()})
	f = newFixture(t, func(d *Deps) { d.Backup = svc })
	rec := f.do(t, http.MethodPost, "/api/v1/admin/backups", "admin", nil)
	if rec.Code != http.StatusNotImplemented {
		t.Fatalf("create unsupported = %d: %s", rec.Code, rec.Body.String())
	}
	rec = f.do(t, http.MethodGet, "/api/v1/admin/backups", "admin", nil)
	if rec.Code != http.StatusOK || decode[backup.Status](t, rec).Supported {
		t.Fatalf("list unsupported = %d: %s", rec.Code, rec.Body.String())
	}
}

func TestHealthzPingsRemoteStores(t *testing.T) {
	fake := objstore.NewFakeS3()
	srv := httptest.NewServer(fake)
	defer srv.Close()
	s3, err := objstore.NewS3(context.Background(), objstore.S3Options{Endpoint: srv.URL, Bucket: "health", Region: "us-east-1", AccessKey: "a", SecretKey: "b", MaxRetries: 1})
	must(t, err)
	f := newFixture(t, func(d *Deps) { d.Objects = s3 })
	if rec := f.do(t, http.MethodGet, "/healthz", "", nil); rec.Code != http.StatusOK {
		t.Fatalf("healthz = %d: %s", rec.Code, rec.Body.String())
	}
	srv.Close() // the endpoint goes away
	if rec := f.do(t, http.MethodGet, "/healthz", "", nil); rec.Code != http.StatusServiceUnavailable || !strings.Contains(rec.Body.String(), `"objects":"error"`) {
		t.Fatalf("healthz with S3 down = %d: %s", rec.Code, rec.Body.String())
	}
}
