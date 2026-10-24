// Package staged implements domain.StagedService. STUB: replaced by the
// go-staged-events workstream (delete this file when implementing).
package staged

import "github.com/seanfilimon/worktree/services/server-go/internal/domain"

// Service manages team-visible staged snapshots.
type Service struct{ domain.StagedService }

// New returns a staged-snapshot service.
func New(store domain.MetaStore, authz domain.Authorizer, bus domain.EventBus) *Service {
	return &Service{}
}
