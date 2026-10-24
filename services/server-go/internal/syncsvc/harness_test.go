package syncsvc

import (
	"context"
	"errors"
	"io"
	"net"
	"sort"
	"strings"
	"testing"
	"time"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/credentials/insecure"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"
	"google.golang.org/grpc/test/bufconn"
	"google.golang.org/protobuf/proto"
	"google.golang.org/protobuf/types/known/timestamppb"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

const (
	tenantAcme = "tenant-acme"
	grpcMaxMsg = 80 << 20
)

// Test principals, keyed by bearer token.
var (
	alice = &domain.Principal{AccountID: "acct-alice", Username: "alice", TenantID: "tenant-alice"}
	bob   = &domain.Principal{AccountID: "acct-bob", Username: "bob", TenantID: "tenant-bob"}
	carol = &domain.Principal{AccountID: "acct-carol", Username: "carol", TenantID: "tenant-carol"}
)

// harness runs a Service behind a real gRPC server over bufconn.
type harness struct {
	t       *testing.T
	svc     *Service
	store   *fakeStore
	objects *objstore.DiskStore
	authz   *fakeAuthz
	review  *fakeReview
	license *fakeLicense
	staged  *fakeStaged
	bus     *fakeBus
	audit   *fakeAudit
	client  syncpb.SyncServiceClient
}

func newHarness(t *testing.T) *harness {
	t.Helper()
	h := &harness{
		t:       t,
		store:   newFakeStore(),
		objects: objstore.New(t.TempDir()),
		authz:   &fakeAuthz{},
		review:  &fakeReview{},
		license: &fakeLicense{},
		staged:  &fakeStaged{},
		bus:     &fakeBus{},
		audit:   &fakeAudit{},
	}
	h.store.tenants[tenantAcme] = &domain.Tenant{ID: tenantAcme, Slug: "acme", Name: "Acme", Kind: domain.TenantOrg}
	h.store.tenants[alice.TenantID] = &domain.Tenant{ID: alice.TenantID, Slug: "alice", Kind: domain.TenantUser}
	h.authz.setRole(tenantAcme, alice.AccountID, domain.RoleOwner)
	h.authz.setRole(tenantAcme, bob.AccountID, domain.RoleViewer)

	h.svc = New(Deps{
		Store:   h.store,
		Objects: h.objects,
		History: fakeHistory{st: h.store},
		Authz:   h.authz,
		Review:  h.review,
		License: h.license,
		Staged:  h.staged,
		Bus:     h.bus,
		Audit:   h.audit,
		Version: "test-1.0",
	})

	tokens := map[string]*domain.Principal{"alice": alice, "bob": bob, "carol": carol}
	resolve := func(ctx context.Context) (context.Context, error) {
		md, _ := metadata.FromIncomingContext(ctx)
		v := md.Get("authorization")
		if len(v) == 0 {
			return ctx, nil
		}
		p, ok := tokens[strings.TrimPrefix(v[0], "Bearer ")]
		if !ok {
			return nil, status.Error(codes.Unauthenticated, "bad token")
		}
		return iam.WithPrincipal(ctx, p), nil
	}

	lis := bufconn.Listen(1 << 20)
	srv := grpc.NewServer(
		grpc.MaxRecvMsgSize(grpcMaxMsg),
		grpc.MaxSendMsgSize(grpcMaxMsg),
		grpc.UnaryInterceptor(func(ctx context.Context, req any, _ *grpc.UnaryServerInfo, next grpc.UnaryHandler) (any, error) {
			ctx, err := resolve(ctx)
			if err != nil {
				return nil, err
			}
			return next(ctx, req)
		}),
		grpc.StreamInterceptor(func(srv any, ss grpc.ServerStream, _ *grpc.StreamServerInfo, next grpc.StreamHandler) error {
			ctx, err := resolve(ss.Context())
			if err != nil {
				return err
			}
			return next(srv, ctxStream{ServerStream: ss, ctx: ctx})
		}),
	)
	syncpb.RegisterSyncServiceServer(srv, h.svc)
	go func() { _ = srv.Serve(lis) }()

	conn, err := grpc.NewClient("passthrough:///bufnet",
		grpc.WithContextDialer(func(ctx context.Context, _ string) (net.Conn, error) { return lis.DialContext(ctx) }),
		grpc.WithTransportCredentials(insecure.NewCredentials()),
		grpc.WithDefaultCallOptions(grpc.MaxCallRecvMsgSize(grpcMaxMsg), grpc.MaxCallSendMsgSize(grpcMaxMsg)),
	)
	if err != nil {
		t.Fatalf("dial: %v", err)
	}
	t.Cleanup(func() {
		_ = conn.Close()
		srv.Stop()
	})
	h.client = syncpb.NewSyncServiceClient(conn)
	return h
}

type ctxStream struct {
	grpc.ServerStream
	ctx context.Context
}

func (c ctxStream) Context() context.Context { return c.ctx }

// as returns a context authenticated as user ("" = anonymous).
func (h *harness) as(user string) context.Context {
	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	h.t.Cleanup(cancel)
	if user == "" {
		return ctx
	}
	return metadata.AppendToOutgoingContext(ctx, "authorization", "Bearer "+user)
}

// newTree creates tree name in tenant acme as alice and returns its id.
func (h *harness) newTree(name string) string {
	h.t.Helper()
	info, err := h.client.ResolveTree(h.as("alice"), &syncpb.ResolveTreeRequest{TenantSlug: "acme", TreeName: name, CreateIfMissing: true})
	if err != nil {
		h.t.Fatalf("create tree: %v", err)
	}
	return info.GetTreeId().GetValue()
}

// commit is a client-side snapshot plus the objects it needs.
type commit struct {
	id       string
	rec      *syncpb.SnapshotRecord
	manifest hash.Hash
	objects  []*syncpb.ObjectData
}

var commitClock = time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)

// makeCommit builds a snapshot of files (path -> content) for treeID.
func makeCommit(treeID string, files map[string]string, parents ...commit) commit {
	paths := make([]string, 0, len(files))
	for p := range files {
		paths = append(paths, p)
	}
	sort.Strings(paths)
	m := &syncpb.ManifestRecord{}
	var objs []*syncpb.ObjectData
	for _, p := range paths {
		data := []byte(files[p])
		bh := hash.Sum(data)
		m.Entries = append(m.Entries, &syncpb.ManifestEntry{Path: p, BlobHash: hashToPB(bh), Size: uint64(len(data))})
		objs = append(objs, &syncpb.ObjectData{Hash: hashToPB(bh), Kind: syncpb.ObjectKind_OBJECT_KIND_BLOB, Data: data})
	}
	mdata, err := proto.MarshalOptions{Deterministic: true}.Marshal(m)
	if err != nil {
		panic(err)
	}
	mh := hash.Sum(mdata)
	objs = append(objs, &syncpb.ObjectData{Hash: hashToPB(mh), Kind: syncpb.ObjectKind_OBJECT_KIND_MANIFEST, Data: mdata})

	commitClock = commitClock.Add(time.Minute)
	id := newID()
	rec := &syncpb.SnapshotRecord{
		Id:           &syncpb.SnapshotId{Value: id},
		TreeId:       &syncpb.TreeId{Value: treeID},
		BranchName:   "main",
		ManifestHash: hashToPB(mh),
		Author:       "alice",
		Message:      "commit " + id[:8],
		CreatedAt:    timestamppb.New(commitClock),
	}
	for _, p := range parents {
		rec.Parents = append(rec.Parents, &syncpb.SnapshotId{Value: p.id})
	}
	return commit{id: id, rec: rec, manifest: mh, objects: objs}
}

// upload sends the objects of commits.
func (h *harness) upload(ctx context.Context, commits ...commit) *syncpb.UploadObjectsResponse {
	h.t.Helper()
	var objs []*syncpb.ObjectData
	for _, c := range commits {
		objs = append(objs, c.objects...)
	}
	resp, err := h.uploadObjects(ctx, objs...)
	if err != nil {
		h.t.Fatalf("upload: %v", err)
	}
	return resp
}

func (h *harness) uploadObjects(ctx context.Context, objs ...*syncpb.ObjectData) (*syncpb.UploadObjectsResponse, error) {
	stream, err := h.client.UploadObjects(ctx)
	if err != nil {
		return nil, err
	}
	for _, o := range objs {
		if err := stream.Send(o); err != nil {
			if errors.Is(err, io.EOF) {
				break // server closed early; CloseAndRecv reports why
			}
			return nil, err
		}
	}
	return stream.CloseAndRecv()
}

// pushReq builds a push of commits (oldest first) to branch.
func pushReq(treeID, branch, oldTip string, commits ...commit) *syncpb.PushRequest {
	req := &syncpb.PushRequest{
		TreeId:     &syncpb.TreeId{Value: treeID},
		BranchName: branch,
		OldTip:     snapshotIDToPB(oldTip),
	}
	for _, c := range commits {
		req.SnapshotChain = append(req.SnapshotChain, &syncpb.SnapshotId{Value: c.id})
		req.Snapshots = append(req.Snapshots, c.rec)
	}
	if len(commits) > 0 {
		req.NewTip = &syncpb.SnapshotId{Value: commits[len(commits)-1].id}
	}
	return req
}

// mustPush pushes and requires acceptance.
func (h *harness) mustPush(ctx context.Context, req *syncpb.PushRequest) {
	h.t.Helper()
	resp, err := h.client.Push(ctx, req)
	if err != nil {
		h.t.Fatalf("push: %v", err)
	}
	if !resp.GetAccepted() {
		h.t.Fatalf("push rejected: %v", resp.GetRejectionReason())
	}
}

func (h *harness) tip(treeID, branch string) string {
	h.t.Helper()
	b, err := h.store.GetBranch(context.Background(), treeID, branch)
	if err != nil {
		return ""
	}
	return b.Tip
}

// wantCode asserts that err is a status error with code c.
func wantCode(t *testing.T, err error, c codes.Code) {
	t.Helper()
	if got := status.Code(err); got != c {
		t.Fatalf("status code = %v (%v), want %v", got, err, c)
	}
}

func ids(xs []*syncpb.SnapshotId) []string {
	out := make([]string, len(xs))
	for i, x := range xs {
		out[i] = x.GetValue()
	}
	return out
}

func hexes(xs []*syncpb.ContentHash) map[string]bool {
	out := make(map[string]bool, len(xs))
	for _, x := range xs {
		out[hash.Hash(x.GetValue()).Hex()] = true
	}
	return out
}
