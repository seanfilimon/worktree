// Package review implements domain.ReviewService (branch protection and
// merge requests). STUB: replaced by the go-review workstream (delete this
// file when implementing).
package review

import "github.com/seanfilimon/worktree/services/server-go/internal/domain"

// Service enforces branch protection and manages merge requests.
type Service struct{ domain.ReviewService }

// New returns a review service.
func New(store domain.MetaStore, hist domain.History, authz domain.Authorizer, bus domain.EventBus, audit domain.AuditLog) *Service {
	return &Service{}
}
