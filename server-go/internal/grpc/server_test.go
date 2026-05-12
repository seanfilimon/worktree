package grpcserver_test

import (
	"context"
	"encoding/hex"
	"fmt"
	"net"
	"testing"
	"time"

	"github.com/zeebo/blake3"
	"google.golang.org/grpc"
	"google.golang.org/grpc/credentials/insecure"

	"github.com/ramizik/worktree/server-go/internal/audit"
	grpcserver "github.com/ramizik/worktree/server-go/internal/grpc"
	worktreepb "github.com/ramizik/worktree/server-go/internal/grpc/worktreepb/worktree/v1"
	"github.com/ramizik/worktree/server-go/internal/iam"
	"github.com/ramizik/worktree/server-go/internal/staged"
	"github.com/ramizik/worktree/server-go/internal/storage"
)

func startTestGRPCServer(t *testing.T) worktreepb.SyncServiceClient {
	t.Helper()
	lis, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("Listen: %v", err)
	}
	tmpDir := t.TempDir()
	srv := grpcserver.NewSyncServer(
		storage.NewLocalObjectStore(tmpDir),
		staged.NewFileStore(tmpDir),
		audit.NoopRecorder{},
		iam.AllowAllAuthorizer{},
	)
	gs := grpc.NewServer()
	worktreepb.RegisterSyncServiceServer(gs, srv)
	go gs.Serve(lis) //nolint:errcheck
	t.Cleanup(gs.GracefulStop)

	conn, err := grpc.NewClient(lis.Addr().String(), grpc.WithTransportCredentials(insecure.NewCredentials()))
	if err != nil {
		t.Fatalf("grpc.NewClient: %v", err)
	}
	t.Cleanup(func() { conn.Close() })
	return worktreepb.NewSyncServiceClient(conn)
}

func blake3Hex(data []byte) string {
	h := blake3.Sum256(data)
	return fmt.Sprintf("%x", h)
}

func TestSyncServer_StageSnapshot_ValidHash(t *testing.T) {
	client := startTestGRPCServer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	content := []byte("hello grpc")
	hash := blake3Hex(content)

	resp, err := client.StageSnapshot(ctx, &worktreepb.StageSnapshotRequest{
		SnapshotId: "grpc-snap-valid",
		Tenant:     "acme",
		Worktree:   "wt",
		TreeId:     "tree-1",
		Branch:     "main",
		Objects: []*worktreepb.StagedObject{
			{Path: "foo.rs", Hash: hash, Size: int64(len(content)), Content: content},
		},
	})
	if err != nil {
		t.Fatalf("StageSnapshot: %v", err)
	}
	if resp.Status != "staged" {
		t.Errorf("status = %q, want staged", resp.Status)
	}
	if resp.SnapshotId != "grpc-snap-valid" {
		t.Errorf("snapshot_id = %q, want grpc-snap-valid", resp.SnapshotId)
	}
	if resp.Objects != 1 {
		t.Errorf("objects = %d, want 1", resp.Objects)
	}
}

func TestSyncServer_StageSnapshot_HashMismatch(t *testing.T) {
	client := startTestGRPCServer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	content := []byte("real content")
	wrongHash := hex.EncodeToString(make([]byte, 32)) // all zeros, wrong

	_, err := client.StageSnapshot(ctx, &worktreepb.StageSnapshotRequest{
		SnapshotId: "grpc-snap-bad",
		Tenant:     "acme",
		Worktree:   "wt",
		TreeId:     "tree-1",
		Branch:     "main",
		Objects: []*worktreepb.StagedObject{
			{Path: "foo.rs", Hash: wrongHash, Size: int64(len(content)), Content: content},
		},
	})
	if err == nil {
		t.Fatal("expected error for hash mismatch, got nil")
	}
}

func TestSyncServer_StageSnapshot_MissingFields(t *testing.T) {
	client := startTestGRPCServer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	_, err := client.StageSnapshot(ctx, &worktreepb.StageSnapshotRequest{
		// Missing snapshot_id, tenant, etc.
		Objects: []*worktreepb.StagedObject{},
	})
	if err == nil {
		t.Fatal("expected error for missing fields, got nil")
	}
}

func TestSyncServer_ListStagedSnapshots(t *testing.T) {
	client := startTestGRPCServer(t)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()

	// First stage a snapshot
	content := []byte("list test content")
	hash := blake3Hex(content)
	_, err := client.StageSnapshot(ctx, &worktreepb.StageSnapshotRequest{
		SnapshotId: "list-test-snap",
		Tenant:     "list-tenant",
		Worktree:   "wt",
		TreeId:     "tree-1",
		Branch:     "feat",
		Objects: []*worktreepb.StagedObject{
			{Path: "a.rs", Hash: hash, Size: int64(len(content)), Content: content},
		},
	})
	if err != nil {
		t.Fatalf("StageSnapshot: %v", err)
	}

	// List and verify
	resp, err := client.ListStagedSnapshots(ctx, &worktreepb.ListStagedSnapshotsRequest{
		Tenant: "list-tenant",
	})
	if err != nil {
		t.Fatalf("ListStagedSnapshots: %v", err)
	}
	found := false
	for _, s := range resp.Snapshots {
		if s.SnapshotId == "list-test-snap" {
			found = true
			if s.Branch != "feat" {
				t.Errorf("branch = %q, want feat", s.Branch)
			}
		}
	}
	if !found {
		t.Error("snapshot not found in list response")
	}
}
