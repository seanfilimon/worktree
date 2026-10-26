// Package httpapi implements the server's HTTP surface: the JSON REST API
// under /api/v1, the Server-Sent Events stream, /healthz, the Prometheus
// /metrics endpoint and the static admin SPA under /admin/.
//
// The package is a thin transport layer. Business rules live in the domain
// services passed in through Deps; handlers decode and validate input,
// authorize the caller via Deps.Authz, call the services, and map domain
// errors onto HTTP status codes and the JSON error envelope
//
//	{"error":{"code":"not_found","message":"..."}}
//
// Every mutating call that the HTTP layer performs directly against the
// MetaStore is recorded in the audit log. Operations delegated to a service
// that owns its own auditing (merge requests via Deps.Review, staged
// snapshots via Deps.Staged) are not audited twice.
package httpapi

import (
	"context"
	"log/slog"
	"net/http"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/backup"
	"github.com/seanfilimon/worktree/services/server-go/internal/ci"
	"github.com/seanfilimon/worktree/services/server-go/internal/codesearch"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork"
	"github.com/seanfilimon/worktree/services/server-go/internal/gc"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/mirror"
	"github.com/seanfilimon/worktree/services/server-go/internal/notify"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/release"
	"github.com/seanfilimon/worktree/services/server-go/internal/scan"
	"github.com/seanfilimon/worktree/services/server-go/internal/signing"
	"github.com/seanfilimon/worktree/services/server-go/internal/todo"
	"github.com/seanfilimon/worktree/services/server-go/internal/treeconfig"
	"github.com/seanfilimon/worktree/services/server-go/internal/webhooks"
)

// Deps are the collaborators of the HTTP API.
//
// Store, Objects, History, Auth, Authz, Review, License and Staged are
// required. Bus and Audit may be nil (events are then not published /
// streamed and audit records are dropped). The remaining fields are
// optional.
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

	// Logger receives request logs and internal errors. nil = slog.Default().
	Logger *slog.Logger
	// CORSOrigins are additional browser origins allowed to call the API
	// (http://localhost:5173 is always allowed). "*" allows any origin.
	CORSOrigins []string
	// Done, when non-nil, is closed when the server starts shutting down.
	// Long-lived responses (the SSE stream) end when it is closed so that
	// graceful shutdown is not held up by idle subscribers.
	Done <-chan struct{}

	// AllowRegistration mirrors the server's self-service registration
	// setting; it is reported by GET /api/v1/admin/system.
	AllowRegistration bool
	// DataDir is the server data directory (holding meta.db and objects/).
	// GET /api/v1/admin/system reports it and the size of <DataDir>/meta.db.
	// "" = unknown.
	DataDir string

	// v0.4 IAM (docs/ENGINEERING_PLAN.md §12).
	// Security handles MFA, sessions, scoped tokens, service-account keys,
	// invitations and password resets.
	Security domain.AccountSecurity
	// Access is the full policy engine (simulator, effective permissions).
	// nil = the IAM simulator endpoints answer 501.
	Access domain.AccessEvaluator
	// SSO is the OpenID Connect provider. nil = SSO disabled.
	SSO domain.SSOProvider

	// v0.5 (docs/ENGINEERING_PLAN.md §13). Each may be nil, in which case
	// the feature endpoints answer 501 not_implemented.
	Todos     *todo.Service
	Releases  *release.Service
	CI        *ci.Service
	Signing   *signing.Service
	GC        *gc.Collector
	Limits    *limits.Limiter
	AccessCfg *accesscfg.Service

	// v0.6 (docs/ENGINEERING_PLAN.md §14). nil = endpoints answer 501.
	Webhooks   *webhooks.Service
	CodeSearch *codesearch.Index

	// v0.7 (docs/ENGINEERING_PLAN.md §15). nil = endpoints answer 501.
	Notify  *notify.Service
	Mirrors *mirror.Service
	Backup  *backup.Service

	// v0.8 (docs/ENGINEERING_PLAN.md §16). nil = endpoints answer 501.
	Forks      *fork.Service
	Scan       *scan.Service
	TreeConfig *treeconfig.Service

	// hardening (§15.10): Pprof serves the Go runtime profiles at
	// /debug/pprof to server admins; false = those routes answer 404.
	Pprof bool
}

// defaultCORSOrigin is the Vite dev server of apps/admin.
const defaultCORSOrigin = "http://localhost:5173"

// api holds the wired handler state.
type api struct {
	d       Deps
	log     *slog.Logger
	mux     *http.ServeMux
	metrics *metrics
	stats   *statsCache

	// openapi (§14.10): every pattern registered through handle, in
	// registration order; the OpenAPI coverage test compares it with
	// openapi.yaml.
	patterns []string
}

// New returns the HTTP handler for /healthz, /metrics, /api/v1 and /admin.
func New(d Deps) http.Handler {
	if d.StartedAt.IsZero() {
		d.StartedAt = time.Now()
	}
	a := &api{
		d:       d,
		log:     d.Logger,
		mux:     http.NewServeMux(),
		metrics: newMetrics(),
	}
	if a.log == nil {
		a.log = slog.Default()
	}
	a.stats = &statsCache{ttl: statsCacheTTL, compute: a.computeStats}
	a.routes()

	// Middleware, outermost first: request id -> logging/metrics -> panic
	// recovery -> CORS -> authentication -> router.
	var h http.Handler = a.mux
	// ops (§13.3): rate limiting runs inside authentication so that
	// callers are limited per principal.
	h = a.rateLimitMiddleware(h)
	h = iam.Middleware(d.Auth)(h)
	h = corsMiddleware(append([]string{defaultCORSOrigin}, d.CORSOrigins...))(h)
	h = a.recoverMiddleware(h)
	h = a.instrumentMiddleware(h)
	h = requestIDMiddleware(h)
	return h
}

// handlerFunc is an HTTP handler that reports failures by returning an
// error; the adapter in handle renders it with the error envelope.
type handlerFunc func(w http.ResponseWriter, r *http.Request) error

// handle registers fn under pattern. The pattern doubles as the metrics
// "route" label, which keeps label cardinality bounded.
func (a *api) handle(pattern string, fn handlerFunc) {
	a.patterns = append(a.patterns, pattern) // openapi (§14.10)
	a.mux.HandleFunc(pattern, func(w http.ResponseWriter, r *http.Request) {
		setRoute(r.Context(), pattern)
		if err := fn(w, r); err != nil {
			a.writeError(w, r, err)
		}
	})
}

func (a *api) routes() {
	// Operational endpoints (no /api/v1 prefix).
	a.handle("GET /healthz", a.handleHealth)
	a.handle("GET /metrics", a.handleMetrics)

	// Auth.
	a.handle("POST /api/v1/auth/register", a.handleRegister)
	a.handle("POST /api/v1/auth/login", a.handleLogin)
	a.handle("POST /api/v1/auth/logout", a.handleLogout)
	a.handle("GET /api/v1/auth/me", a.handleMe)
	a.handle("GET /api/v1/auth/tokens", a.handleListTokens)
	a.handle("POST /api/v1/auth/tokens", a.handleCreateToken)
	a.handle("DELETE /api/v1/auth/tokens/{id}", a.handleRevokeToken)

	// Tenants and members.
	a.handle("GET /api/v1/tenants", a.handleListTenants)
	a.handle("POST /api/v1/tenants", a.handleCreateTenant)
	a.handle("GET /api/v1/tenants/{tenant}", a.handleGetTenant)
	a.handle("GET /api/v1/tenants/{tenant}/members", a.handleListMembers)
	a.handle("PUT /api/v1/tenants/{tenant}/members/{username}", a.handlePutMember)
	a.handle("DELETE /api/v1/tenants/{tenant}/members/{username}", a.handleDeleteMember)
	a.handle("GET /api/v1/tenants/{tenant}/trees", a.handleListTrees)
	a.handle("POST /api/v1/tenants/{tenant}/trees", a.handleCreateTree)

	// Trees.
	const tree = "/api/v1/trees/{tenant}/{tree}"
	a.handle("GET "+tree, a.handleGetTree)
	a.handle("PATCH "+tree, a.handlePatchTree)
	a.handle("DELETE "+tree, a.handleDeleteTree)
	a.handle("GET "+tree+"/branches", a.handleListBranches)
	a.handle("GET "+tree+"/snapshots", a.handleListSnapshots)
	a.handle("GET "+tree+"/snapshots/{id}", a.handleGetSnapshot)
	a.handle("GET "+tree+"/blob/{snapshot}/{path...}", a.handleBlob)
	a.handle("GET "+tree+"/tags", a.handleListTags)
	a.handle("GET "+tree+"/staged", a.handleListStaged)
	a.handle("GET "+tree+"/protections", a.handleListProtections)
	a.handle("PUT "+tree+"/protections", a.handlePutProtection)
	a.handle("DELETE "+tree+"/protections", a.handleDeleteProtection)
	a.handle("GET "+tree+"/licenses", a.handleListLicenses)
	a.handle("PUT "+tree+"/licenses", a.handlePutLicense)
	a.handle("DELETE "+tree+"/licenses", a.handleDeleteLicense)

	// Merge requests.
	a.handle("GET "+tree+"/merge-requests", a.handleListMRs)
	a.handle("POST "+tree+"/merge-requests", a.handleCreateMR)
	a.handle("GET "+tree+"/merge-requests/{n}", a.handleGetMR)
	a.handle("POST "+tree+"/merge-requests/{n}/approve", a.mrAction("approve"))
	a.handle("POST "+tree+"/merge-requests/{n}/merge", a.mrAction("merge"))
	a.handle("POST "+tree+"/merge-requests/{n}/close", a.mrAction("close"))

	// Audit, events, server administration.
	a.handle("GET /api/v1/audit", a.handleAudit)
	a.handle("GET /api/v1/events", a.handleEvents)
	a.handle("GET /api/v1/admin/stats", a.handleAdminStats)
	a.handle("GET /api/v1/admin/accounts", a.handleAdminListAccounts)
	a.handle("PATCH /api/v1/admin/accounts/{username}", a.handleAdminPatchAccount)

	// v0.3 admin expansion (docs/ENGINEERING_PLAN.md §11).
	a.registerInsightRoutes(tree)
	a.registerManageRoutes(tree)

	// v0.4 IAM (docs/ENGINEERING_PLAN.md §12).
	a.registerIAMRoutes(tree)
	a.registerSSORoutes()

	// v0.5 spec completeness (docs/ENGINEERING_PLAN.md §13).
	a.registerTodoRoutes(tree)
	a.registerReleaseRoutes(tree)
	a.registerCIRoutes(tree)
	a.registerReviewRoutes(tree)
	a.registerKeyRoutes()
	a.registerOpsRoutes()
	a.registerAccessRoutes(tree)
	a.registerExportRoutes(tree)
	a.registerStagedRoutes(tree)

	// v0.6 feature expansion (docs/ENGINEERING_PLAN.md §14).
	a.registerWebhookRoutes(tree)
	a.registerCodeSearchRoutes()
	a.registerBlameRoutes(tree)
	a.registerFsckRoutes()
	a.registerPolicyHistoryRoutes()

	// v0.7 build-out (docs/ENGINEERING_PLAN.md §15).
	a.registerNotificationRoutes(tree)
	a.registerMirrorRoutes(tree)
	a.registerBackupRoutes()
	a.registerOwnerRoutes(tree)

	// v0.8 (docs/ENGINEERING_PLAN.md §16).
	a.registerForkRoutes(tree)
	a.registerTreeConfigRoutes(tree)
	a.registerTenantLifecycleRoutes()
	a.registerAttributeRoutes(tree)
	a.registerDeviceRoutes()
	a.registerComplianceRoutes(tree)
	a.registerResolveRoutes(tree)
	// history-v2 (§16.10): GET {tree}/reflog.
	a.registerReflogRoutes(tree)

	a.registerPprofRoutes()   // hardening (§15.10): GET /debug/pprof[/{profile}]
	a.registerOpenAPIRoutes() // openapi (§14.10): GET /api/v1/openapi.yaml

	// Unknown API paths answer with the JSON envelope (404 or 405).
	a.handle("/api/", a.handleAPINotFound)

	// Static admin SPA.
	admin := newAdminHandler(a.d.AdminDir, a.log)
	a.mux.Handle("GET /admin/", routeHandler("GET /admin/", admin))
	a.mux.Handle("GET /admin", routeHandler("GET /admin", http.RedirectHandler("/admin/", http.StatusMovedPermanently)))
	a.mux.Handle("GET /{$}", routeHandler("GET /{$}", http.RedirectHandler("/admin/", http.StatusFound)))
}

// routeHandler records pattern as the metrics route of requests served by h.
func routeHandler(pattern string, h http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		setRoute(r.Context(), pattern)
		h.ServeHTTP(w, r)
	})
}

// handleAPINotFound answers unmatched /api/ requests. When the path exists
// for other methods it answers 405 with an Allow header, mirroring
// ServeMux, but with the JSON error envelope.
func (a *api) handleAPINotFound(w http.ResponseWriter, r *http.Request) error {
	var allowed []string
	for _, m := range []string{http.MethodGet, http.MethodPost, http.MethodPut, http.MethodPatch, http.MethodDelete} {
		if m == r.Method {
			continue
		}
		probe := r.Clone(context.Background())
		probe.Method = m
		if _, pattern := a.mux.Handler(probe); pattern != "" && pattern != "/api/" {
			allowed = append(allowed, m)
		}
	}
	if len(allowed) > 0 {
		for _, m := range allowed {
			w.Header().Add("Allow", m)
		}
		return &apiError{status: http.StatusMethodNotAllowed, code: "method_not_allowed",
			msg: "method " + r.Method + " not allowed on " + r.URL.Path}
	}
	return notFoundf("no such endpoint: %s %s", r.Method, r.URL.Path)
}

// handleHealth answers GET /healthz: "ok" with 200 when every probe (meta,
// objects) passes, else "degraded" with 503 (§13.3).
func (a *api) handleHealth(w http.ResponseWriter, r *http.Request) error {
	w.Header().Set("Cache-Control", "no-store")
	checks, ok := a.healthChecks(r.Context())
	resp := healthResponse{
		Status:        "ok",
		Version:       a.d.Version,
		UptimeSeconds: int64(time.Since(a.d.StartedAt).Seconds()),
		Checks:        checks,
	}
	status := http.StatusOK
	if !ok {
		resp.Status, status = "degraded", http.StatusServiceUnavailable
	}
	writeJSON(w, status, resp)
	return nil
}

type healthResponse struct {
	Status        string            `json:"status"`
	Version       string            `json:"version"`
	UptimeSeconds int64             `json:"uptime_seconds"`
	Checks        map[string]string `json:"checks"`
}
