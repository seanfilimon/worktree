// Package treeconfig validates and applies the .wt-tree configuration of pushed snapshots (§16.3).
//
// v0.8 stub (docs/ENGINEERING_PLAN.md §16): the owning workstream replaces
// this file with the implementation.
package treeconfig

import (
	"context"
	"log/slog"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the treeconfig package. Store is required;
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

// Service is the treeconfig component.
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

// CheckPushPlan implements domain.PushChecker. The stub accepts every push.
func (s *Service) CheckPushPlan(ctx context.Context, p *domain.Principal, tree *domain.Tree, plan *domain.PushPlan) error {
	return nil
}
