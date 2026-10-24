// Package syncsvc implements the gRPC worktree.sync.v1.SyncService. STUB:
// replaced by the go-sync workstream (delete this file when implementing).
package syncsvc

import (
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Deps are the collaborators of the sync service.
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
}

// Service implements syncpb.SyncServiceServer.
type Service struct {
	syncpb.UnimplementedSyncServiceServer
	d Deps
}

// New returns a sync service.
func New(d Deps) *Service { return &Service{d: d} }
