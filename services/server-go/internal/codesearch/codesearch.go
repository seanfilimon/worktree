// Package codesearch indexes the default branch of trees and answers code searches (§14.3).
//
// v0.6 stub (docs/ENGINEERING_PLAN.md §14): the owning workstream replaces
// this file with the implementation.
package codesearch

import (
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the codesearch package. Store is required;
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

// Index is the codesearch component.
type Index struct {
	d Deps
}

// New returns a Index.
func New(d Deps) *Index {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Index{d: d}
}
