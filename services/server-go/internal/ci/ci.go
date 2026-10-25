// Package ci stores CI check results reported for snapshots and evaluates required checks for merges (§13.4).
//
// v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
// this file with the implementation.
package ci

import (
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the ci package. Store is required;
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

// Service is the ci component.
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
