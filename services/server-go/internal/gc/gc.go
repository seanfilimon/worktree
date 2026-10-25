// Package gc garbage-collects unreachable objects and expired staged snapshots (§13.3).
//
// v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
// this file with the implementation.
package gc

import (
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the gc package. Store is required;
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

// Collector is the gc component.
type Collector struct {
	d Deps
}

// New returns a Collector.
func New(d Deps) *Collector {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Collector{d: d}
}
