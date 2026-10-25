// Package limits enforces per-tenant quotas (as a push check) and request rate limits (§13.3).
//
// v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
// this file with the implementation.
package limits

import (
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the limits package. Store is required;
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

// Limiter is the limits component.
type Limiter struct {
	d Deps
}

// New returns a Limiter.
func New(d Deps) *Limiter {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Limiter{d: d}
}
