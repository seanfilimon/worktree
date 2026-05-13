package httpapi

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/ramizik/worktree/server-go/internal/audit"
	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/iam"
	"github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"
	"github.com/zeebo/blake3"
)

type stagedStoreStub struct {
	add  func(context.Context, staged.Snapshot) (staged.AddResult, error)
	list func(context.Context, staged.ListFilter) ([]staged.Snapshot, error)
}

func (s stagedStoreStub) Add(ctx context.Context, snapshot staged.Snapshot) (staged.AddResult, error) {
	if s.add == nil {
		return staged.AddResult{Status: staged.AddStatusCreated, Snapshot: snapshot}, nil
	}
	return s.add(ctx, snapshot)
}

func (s stagedStoreStub) List(ctx context.Context, filter staged.ListFilter) ([]staged.Snapshot, error) {
	if s.list == nil {
		return nil, nil
	}
	return s.list(ctx, filter)
}

func TestStagedUploadPersistsVerifiedObject(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	var recorded staged.Snapshot
	auditRecorder := &auditRecorderStub{}
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) (staged.AddResult, error) {
			recorded = snapshot
			return staged.AddResult{Status: staged.AddStatusCreated, Snapshot: snapshot}, nil
		},
	}, auditRecorder, iam.AllowAllAuthorizer{})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})

	content := []byte("file contents")
	hash := blake3.Sum256(content)
	body := stagedUploadRequest{
		SnapshotID: "snap-1",
		Tenant:     "acme",
		Worktree:   "api",
		TreeID:     "tree-1",
		Branch:     "main",
		Objects: []stagedObjectUpload{{
			Path:    "src/main.rs",
			Hash:    hex.EncodeToString(hash[:]),
			Size:    len(content),
			Content: content,
		}},
	}
	data, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/staged", bytes.NewReader(data))
	req.Header.Set("X-WT-Tenant", "acme")
	req.Header.Set("X-WT-Account", "alice")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusAccepted {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
	if recorded.SnapshotID != "snap-1" {
		t.Fatalf("recorded snapshot = %q", recorded.SnapshotID)
	}
	if len(recorded.ObjectIDs) != 1 {
		t.Fatalf("recorded objects = %d", len(recorded.ObjectIDs))
	}
	exists, err := objects.Exists(context.Background(), recorded.ObjectIDs[0])
	if err != nil {
		t.Fatal(err)
	}
	if !exists {
		t.Fatal("expected staged object to be stored")
	}
	if auditRecorder.last.Decision != "allow" || auditRecorder.last.Action != "staged:create" {
		t.Fatalf("audit event = %#v", auditRecorder.last)
	}
}

func TestStagedUploadRejectsHashMismatch(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) (staged.AddResult, error) {
			t.Fatal("staged store should not be called")
			return staged.AddResult{}, nil
		},
	}, nil, iam.AllowAllAuthorizer{})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})

	hash := blake3.Sum256([]byte("expected"))
	body := stagedUploadRequest{
		SnapshotID: "snap-1",
		Tenant:     "acme",
		Worktree:   "api",
		TreeID:     "tree-1",
		Branch:     "main",
		Objects: []stagedObjectUpload{{
			Path:    "src/main.rs",
			Hash:    hex.EncodeToString(hash[:]),
			Size:    len("actual"),
			Content: []byte("actual"),
		}},
	}
	data, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/staged", bytes.NewReader(data))
	req.Header.Set("X-WT-Tenant", "acme")
	req.Header.Set("X-WT-Account", "alice")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
}

func TestStagedUploadRejectsObjectSizeLimit(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	auditRecorder := &auditRecorderStub{}
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) (staged.AddResult, error) {
			t.Fatal("staged store should not be called")
			return staged.AddResult{}, nil
		},
	}, auditRecorder, iam.AllowAllAuthorizer{}, StagedLimits{MaxObjectBytes: 4, MaxObjects: 10})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})

	content := []byte("too-large")
	hash := blake3.Sum256(content)
	body := stagedUploadRequest{
		SnapshotID: "snap-1",
		Tenant:     "acme",
		Worktree:   "api",
		TreeID:     "tree-1",
		Branch:     "main",
		Objects: []stagedObjectUpload{{
			Path:    "src/main.rs",
			Hash:    hex.EncodeToString(hash[:]),
			Size:    len(content),
			Content: content,
		}},
	}
	data, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/staged", bytes.NewReader(data))
	req.Header.Set("X-WT-Tenant", "acme")
	req.Header.Set("X-WT-Account", "alice")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
	if auditRecorder.last.Decision != "deny" || auditRecorder.last.Reason != "staged upload object size exceeds configured limit" {
		t.Fatalf("audit event = %#v", auditRecorder.last)
	}
}

func TestStagedUploadRejectsObjectCountLimit(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) (staged.AddResult, error) {
			t.Fatal("staged store should not be called")
			return staged.AddResult{}, nil
		},
	}, nil, iam.AllowAllAuthorizer{}, StagedLimits{MaxObjectBytes: 100, MaxObjects: 1})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})

	content := []byte("file")
	hash := blake3.Sum256(content)
	body := stagedUploadRequest{
		SnapshotID: "snap-1",
		Tenant:     "acme",
		Worktree:   "api",
		TreeID:     "tree-1",
		Branch:     "main",
		Objects: []stagedObjectUpload{
			{Path: "a.txt", Hash: hex.EncodeToString(hash[:]), Size: len(content), Content: content},
			{Path: "b.txt", Hash: hex.EncodeToString(hash[:]), Size: len(content), Content: content},
		},
	}
	data, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/staged", bytes.NewReader(data))
	req.Header.Set("X-WT-Tenant", "acme")
	req.Header.Set("X-WT-Account", "alice")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusRequestEntityTooLarge {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
}

func TestStagedUploadRejectsTenantMismatch(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	auditRecorder := &auditRecorderStub{}
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) (staged.AddResult, error) {
			t.Fatal("staged store should not be called")
			return staged.AddResult{}, nil
		},
	}, auditRecorder, iam.AllowAllAuthorizer{})
	jwtAuth := auth.NewJWTAuthenticator("secret")
	router := NewRouter(RouterConfig{Version: "test", Staged: service, Authenticator: jwtAuth})

	content := []byte("file contents")
	hash := blake3.Sum256(content)
	body := stagedUploadRequest{
		SnapshotID: "snap-1",
		Tenant:     "acme",
		Worktree:   "api",
		TreeID:     "tree-1",
		Branch:     "main",
		Objects: []stagedObjectUpload{{
			Path:    "src/main.rs",
			Hash:    hex.EncodeToString(hash[:]),
			Size:    len(content),
			Content: content,
		}},
	}
	data, err := json.Marshal(body)
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/staged", bytes.NewReader(data))
	token, _ := jwtAuth.GenerateToken(auth.Principal{
		Tenant:  "other",
		Account: "alice",
	})
	req.Header.Set("Authorization", "Bearer "+token)
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
	if auditRecorder.last.Decision != "deny" || auditRecorder.last.Reason != "tenant mismatch" {
		t.Fatalf("audit event = %#v", auditRecorder.last)
	}
}

func TestStagedListFiltersToAuthenticatedTenant(t *testing.T) {
	var received staged.ListFilter
	auditRecorder := &auditRecorderStub{}
	service := NewStagedService(nil, stagedStoreStub{
		list: func(ctx context.Context, filter staged.ListFilter) ([]staged.Snapshot, error) {
			received = filter
			return []staged.Snapshot{{
				SnapshotID: "snap-1",
				Tenant:     "acme",
				Worktree:   "api",
				Branch:     "main",
			}}, nil
		},
	}, auditRecorder, iam.AllowAllAuthorizer{})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})
	req := httptest.NewRequest(http.MethodGet, "/staged?worktree=api", nil)
	req.Header.Set("X-WT-Tenant", "acme")
	req.Header.Set("X-WT-Account", "alice")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
	if received.Tenant != "acme" || received.Worktree != "api" {
		t.Fatalf("filter = %#v", received)
	}
	if auditRecorder.last.Decision != "allow" || auditRecorder.last.Action != "staged:list" {
		t.Fatalf("audit event = %#v", auditRecorder.last)
	}
}

func TestStagedListRejectsTenantMismatch(t *testing.T) {
	service := NewStagedService(nil, stagedStoreStub{
		list: func(ctx context.Context, filter staged.ListFilter) ([]staged.Snapshot, error) {
			t.Fatal("staged store should not be called")
			return nil, nil
		},
	}, nil, iam.AllowAllAuthorizer{})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})
	req := httptest.NewRequest(http.MethodGet, "/staged?tenant=other", nil)
	req.Header.Set("X-WT-Tenant", "acme")
	req.Header.Set("X-WT-Account", "alice")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
}

func TestHandleUpload_IAMDeny(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	service := NewStagedService(objects, stagedStoreStub{}, nil, iam.DenyAllAuthorizer{})

	content := []byte("hello")
	hash := blake3.Sum256(content)
	body := stagedUploadRequest{
		SnapshotID: "snap-1",
		Tenant:     "acme",
		Worktree:   "wt",
		TreeID:     "tree-1",
		Branch:     "main",
		Objects: []stagedObjectUpload{{
			Path:    "a.rs",
			Hash:    hex.EncodeToString(hash[:]),
			Size:    len(content),
			Content: content,
		}},
	}
	data, _ := json.Marshal(body)
	req := httptest.NewRequest(http.MethodPost, "/staged", bytes.NewReader(data))
	req = req.WithContext(auth.WithPrincipal(req.Context(), auth.Principal{Tenant: "acme", Account: "alice"}))
	rec := httptest.NewRecorder()
	service.HandleUpload(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Errorf("status = %d, want 403", rec.Code)
	}
}

type auditRecorderStub struct {
	last audit.Event
}

func (r *auditRecorderStub) Record(ctx context.Context, event audit.Event) error {
	r.last = event
	return nil
}
