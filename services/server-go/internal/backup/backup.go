// Package backup creates and restores server backups of the metadata database and objects (§15.2).
//
// v0.7 stub (docs/ENGINEERING_PLAN.md §15): the owning workstream replaces
// this file with the implementation.
package backup

import (
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the backup package. Store is required;
// the others may be nil unless documented otherwise.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Service is the backup component.
type Service struct {
	d Deps
}

// New returns a Service.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d}
}
