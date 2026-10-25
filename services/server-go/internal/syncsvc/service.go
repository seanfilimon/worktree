// Package syncsvc implements the gRPC worktree.sync.v1.SyncService, the
// wire protocol between W0rkTree clients (worktree-sdk / worktree-bg) and
// the Go server.
//
// The service is a thin orchestration layer: every piece of state lives
// behind the domain interfaces supplied in [Deps] (metadata store, object
// store, history walker, authorizer, policy checkers, event bus, audit log).
// It never keeps per-request state of its own, so a single *Service is safe
// for concurrent use by any number of RPCs.
//
// # Authentication
//
// The caller's identity is read with iam.PrincipalFrom; the gRPC
// interceptors from internal/iam attach it from the `authorization: Bearer`
// metadata. Anonymous callers may only Ping and read public trees (as
// decided by the Authorizer).
//
// # Error model
//
// Transport-level failures are returned as gRPC status errors with
// canonical codes (see errors.go). Policy rejections of a push are not
// errors: Push returns PushResponse{accepted:false, rejection_reason}. In
// the same spirit Stage, PushTag and SyncAccessConfig report domain-level
// refusals in their response bodies.
package syncsvc

import (
	"context"
	"log/slog"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"google.golang.org/protobuf/proto"
)

// MaxObjectSize is the largest object (blob or manifest) accepted by
// UploadObjects. It matches the server's 64 MiB gRPC message limit.
const MaxObjectSize = 64 << 20

// TreeIDMetadataKey is the optional gRPC metadata key that scopes a
// DownloadObjects call to one tree (see DownloadObjects).
const TreeIDMetadataKey = "x-wt-tree-id"

// Deps are the collaborators of the sync service.
//
// Store, Objects, History and Authz are required. Review and License may be
// nil, which disables the corresponding push check. Staged may be nil, in
// which case Stage/ListStaged return Unimplemented. Bus and Audit may be nil
// (events / audit records are then dropped).
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Review  domain.ReviewService
	License domain.LicenseChecker
	Staged  domain.StagedService
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Version string

	// v0.5 (docs/ENGINEERING_PLAN.md §13). PushChecks run in order after
	// the license check of Push; nil/empty = none.
	PushChecks []domain.PushChecker
	// AccessCfg applies SyncAccessConfig documents; nil = documents are
	// only stored (v0.4 behaviour).
	AccessCfg *accesscfg.Service
	// TreeQuota enforces the tenant MaxTrees quota (§13.3) when ResolveTree
	// creates a missing tree; nil = unlimited.
	TreeQuota TreeQuota

	// v0.5 daemon (§13.11): Done, when non-nil, is closed when the server
	// starts shutting down; open WatchBranches streams then end with
	// Unavailable so graceful shutdown is not held up by idle watchers.
	Done <-chan struct{}
}

// TreeQuota checks whether a tenant may create another tree (implemented by
// *limits.Limiter: it returns a *limits.QuotaError when MaxTrees is reached).
type TreeQuota interface {
	CheckTreeCreate(ctx context.Context, t *domain.Tenant) error
}

// Service implements syncpb.SyncServiceServer.
type Service struct {
	syncpb.UnimplementedSyncServiceServer
	d   Deps
	log *slog.Logger
	now func() time.Time
}

var _ syncpb.SyncServiceServer = (*Service)(nil)

// New returns a sync service.
func New(d Deps) *Service {
	return &Service{
		d:   d,
		log: slog.Default().With("component", "syncsvc"),
		now: func() time.Time { return time.Now().UTC() },
	}
}

// Ping is a liveness and identity probe. It is available anonymously; when
// the caller is authenticated the response also identifies the account.
func (s *Service) Ping(ctx context.Context, _ *syncpb.PingRequest) (*syncpb.PingResponse, error) {
	resp := &syncpb.PingResponse{ServerVersion: s.d.Version}
	p := iam.PrincipalFrom(ctx)
	if p == nil {
		return resp, nil
	}
	resp.AccountId = &syncpb.AccountId{Value: p.AccountID}
	resp.Username = proto.String(p.Username)
	if p.TenantID != "" {
		// Best effort: a missing personal tenant must not fail a ping.
		if t, err := s.d.Store.GetTenant(ctx, p.TenantID); err == nil {
			resp.TenantSlug = proto.String(t.Slug)
		} else {
			s.log.DebugContext(ctx, "ping: personal tenant lookup failed", "tenant_id", p.TenantID, "err", err)
		}
	}
	return resp, nil
}

// publish sends e on the event bus, if one is configured.
func (s *Service) publish(e domain.Event) {
	if s.d.Bus == nil {
		return
	}
	if e.At.IsZero() {
		e.At = s.now()
	}
	s.d.Bus.Publish(e)
}

// audit records e in the audit log, if one is configured, filling in the
// actor, remote address and timestamp from the request context.
func (s *Service) audit(ctx context.Context, p *domain.Principal, e domain.AuditEvent) {
	if s.d.Audit == nil {
		return
	}
	if p != nil {
		e.ActorID = p.AccountID
		e.Actor = p.Username
	} else {
		e.Actor = "anonymous"
	}
	if e.RemoteAddr == "" {
		e.RemoteAddr = remoteAddr(ctx)
	}
	if e.At.IsZero() {
		e.At = s.now()
	}
	s.d.Audit.Record(ctx, e)
}

// actorName returns the username of p, or "" for anonymous callers.
func actorName(p *domain.Principal) string {
	if p == nil {
		return ""
	}
	return p.Username
}
