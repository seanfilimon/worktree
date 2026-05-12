package httpapi

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"
	"github.com/zeebo/blake3"
)

type stagedStoreStub struct {
	add  func(context.Context, staged.Snapshot) error
	list func(context.Context, staged.ListFilter) ([]staged.Snapshot, error)
}

func (s stagedStoreStub) Add(ctx context.Context, snapshot staged.Snapshot) error {
	if s.add == nil {
		return nil
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
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) error {
			recorded = snapshot
			return nil
		},
	})
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
}

func TestStagedUploadRejectsHashMismatch(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) error {
			t.Fatal("staged store should not be called")
			return nil
		},
	})
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
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusUnprocessableEntity {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
}

func TestStagedUploadRejectsTenantMismatch(t *testing.T) {
	root := t.TempDir()
	objects := storage.NewLocalObjectStore(root)
	service := NewStagedService(objects, stagedStoreStub{
		add: func(ctx context.Context, snapshot staged.Snapshot) error {
			t.Fatal("staged store should not be called")
			return nil
		},
	})
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
	req.Header.Set("X-WT-Tenant", "other")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
}

func TestStagedListFiltersToAuthenticatedTenant(t *testing.T) {
	var received staged.ListFilter
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
	})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})
	req := httptest.NewRequest(http.MethodGet, "/staged?worktree=api", nil)
	req.Header.Set("X-WT-Tenant", "acme")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
	if received.Tenant != "acme" || received.Worktree != "api" {
		t.Fatalf("filter = %#v", received)
	}
}

func TestStagedListRejectsTenantMismatch(t *testing.T) {
	service := NewStagedService(nil, stagedStoreStub{
		list: func(ctx context.Context, filter staged.ListFilter) ([]staged.Snapshot, error) {
			t.Fatal("staged store should not be called")
			return nil, nil
		},
	})
	router := NewRouter(RouterConfig{Version: "test", Staged: service})
	req := httptest.NewRequest(http.MethodGet, "/staged?tenant=other", nil)
	req.Header.Set("X-WT-Tenant", "acme")
	rec := httptest.NewRecorder()

	router.ServeHTTP(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Fatalf("status = %d, body = %s", rec.Code, rec.Body.String())
	}
}
