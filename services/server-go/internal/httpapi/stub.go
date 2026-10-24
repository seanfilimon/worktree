// Package httpapi implements the REST + SSE API. STUB: replaced by the
// go-http workstream (delete this file when implementing).
package httpapi

import (
	"net/http"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the HTTP API.
type Deps struct {
	Store     domain.MetaStore
	Objects   objstore.Store
	History   domain.History
	Auth      domain.Authenticator
	Authz     domain.Authorizer
	Review    domain.ReviewService
	License   domain.LicenseChecker
	Staged    domain.StagedService
	Bus       domain.EventBus
	Audit     domain.AuditLog
	Version   string
	AdminDir  string
	StartedAt time.Time
}

// New returns the HTTP handler for /healthz, /metrics, /api/v1 and /admin.
func New(d Deps) http.Handler { return http.NotFoundHandler() }
