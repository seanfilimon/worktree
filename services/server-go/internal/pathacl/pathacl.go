// Package pathacl enforces path-scoped access policies on push (deny-only tree:write per changed path, §13.1).
//
// v0.5 stub (docs/ENGINEERING_PLAN.md §13): the owning workstream replaces
// this file with the implementation.
package pathacl

import (
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the pathacl package. Store is required;
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

// Checker is the pathacl component.
type Checker struct {
	d Deps
}

// New returns a Checker.
func New(d Deps) *Checker {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Checker{d: d}
}
