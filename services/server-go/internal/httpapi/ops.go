package httpapi

import (
	"context"
	"errors"
	"math"
	"net/http"
	"os"
	"strconv"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/gc"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
)

// Operations endpoints (docs/ENGINEERING_PLAN.md §13.3): garbage
// collection, tenant limits and usage, rate limiting and health probes.

// registerOpsRoutes registers the v0.5 routes for garbage collection,
// retention and tenant limits (§13.3). The audit export lives in
// handleAudit.
func (a *api) registerOpsRoutes() {
	a.handle("POST /api/v1/admin/gc", a.handleRunGC)
	a.handle("GET /api/v1/admin/gc", a.handleGCStatus)
	a.handle("GET /api/v1/admin/tenants/{tenant}/limits", a.handleGetLimits)
	a.handle("PUT /api/v1/admin/tenants/{tenant}/limits", a.handlePutLimits)
	a.handle("GET /api/v1/tenants/{tenant}/usage", a.handleUsage)
}

func errNotImplemented(feature string) error {
	return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
		msg: feature + " is not available on this server"}
}

// errQuota is the REST rendering of a tenant quota violation.
func errQuota(qe *limits.QuotaError) error {
	return &apiError{status: http.StatusConflict, code: "quota_exceeded",
		msg: "tenant quota exceeded: " + qe.Limit + " (limit " + strconv.FormatInt(qe.Max, 10) + ")"}
}

func (a *api) requireServerAdmin(r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	return a.d.Authz.Authorize(r.Context(), principal(r), domain.ActServerAdmin, "", nil)
}

// handleRunGC answers POST /api/v1/admin/gc?dry_run=true|false.
func (a *api) handleRunGC(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.GC == nil {
		return errNotImplemented("garbage collection")
	}
	dry := false
	if s := r.URL.Query().Get("dry_run"); s != "" {
		b, err := strconv.ParseBool(s)
		if err != nil {
			return badRequestf("dry_run must be true or false")
		}
		dry = b
	}
	// The run outlives a client that disconnects: deleting half-way is
	// fine, but the report should still be recorded.
	rep, err := a.d.GC.Run(context.WithoutCancel(r.Context()), dry)
	if errors.Is(err, gc.ErrRunning) {
		return &apiError{status: http.StatusConflict, code: "gc_running", msg: err.Error()}
	}
	if err != nil && rep == nil {
		return err
	}
	if err != nil {
		a.log.ErrorContext(r.Context(), "gc run failed", "error", err)
		writeJSON(w, http.StatusInternalServerError, rep)
		return nil
	}
	writeJSON(w, http.StatusOK, rep)
	return nil
}

type gcConfigView struct {
	Interval               string `json:"interval"`
	IntervalSeconds        int64  `json:"interval_seconds"`
	Grace                  string `json:"grace"`
	GraceSeconds           int64  `json:"grace_seconds"`
	StagedRetention        string `json:"staged_retention"`
	StagedRetentionSeconds int64  `json:"staged_retention_seconds"`
	AuditRetention         string `json:"audit_retention"`
	AuditRetentionSeconds  int64  `json:"audit_retention_seconds"`
}

type gcStatusView struct {
	Config    gcConfigView      `json:"config"`
	Running   bool              `json:"running"`
	NextRunAt *time.Time        `json:"next_run_at,omitempty"`
	Runs      []domain.GCReport `json:"runs"`
}

// handleGCStatus answers GET /api/v1/admin/gc.
func (a *api) handleGCStatus(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.GC == nil {
		return errNotImplemented("garbage collection")
	}
	runs, err := a.d.GC.Runs(r.Context())
	if err != nil {
		return err
	}
	c := a.d.GC.Config()
	v := gcStatusView{
		Config: gcConfigView{
			Interval: c.Interval.String(), IntervalSeconds: int64(c.Interval.Seconds()),
			Grace: c.Grace.String(), GraceSeconds: int64(c.Grace.Seconds()),
			StagedRetention: c.StagedRetention.String(), StagedRetentionSeconds: int64(c.StagedRetention.Seconds()),
			AuditRetention: c.AuditRetention.String(), AuditRetentionSeconds: int64(c.AuditRetention.Seconds()),
		},
		Running: a.d.GC.Running(),
		Runs:    nonNil(runs),
	}
	if next := a.d.GC.NextRun(); !next.IsZero() {
		next = next.UTC()
		v.NextRunAt = &next
	}
	writeJSON(w, http.StatusOK, v)
	return nil
}

// adminTenant resolves {tenant} for a server-admin endpoint.
func (a *api) adminTenant(r *http.Request) (*domain.Tenant, error) {
	if err := a.requireServerAdmin(r); err != nil {
		return nil, err
	}
	return a.tenantByRef(r.Context(), r.PathValue("tenant"))
}

// handleGetLimits answers GET /api/v1/admin/tenants/{tenant}/limits.
func (a *api) handleGetLimits(w http.ResponseWriter, r *http.Request) error {
	t, err := a.adminTenant(r)
	if err != nil {
		return err
	}
	var l domain.TenantLimits
	if t.Limits != nil {
		l = *t.Limits
	}
	writeJSON(w, http.StatusOK, l)
	return nil
}

// handlePutLimits answers PUT /api/v1/admin/tenants/{tenant}/limits: the
// body replaces the tenant's limits ({} clears them).
func (a *api) handlePutLimits(w http.ResponseWriter, r *http.Request) error {
	t, err := a.adminTenant(r)
	if err != nil {
		return err
	}
	var l domain.TenantLimits
	if err := decodeJSON(w, r, &l); err != nil {
		return err
	}
	if l.MaxTrees < 0 || l.MaxStorageBytes < 0 || l.MaxBranchesPerTree < 0 || l.MaxMembers < 0 || l.RequestsPerMinute < 0 {
		return badRequestf("limits must not be negative")
	}
	if l == (domain.TenantLimits{}) {
		t.Limits = nil
	} else {
		t.Limits = &l
	}
	if err := a.d.Store.UpdateTenant(r.Context(), t); err != nil {
		return err
	}
	if a.d.Limits != nil {
		a.d.Limits.ForgetTenant(t.ID)
	}
	a.audit(r, audit.ActionLimitsPut, t.ID, "tenant", t.ID, map[string]string{
		"slug":                  t.Slug,
		"max_trees":             strconv.Itoa(l.MaxTrees),
		"max_storage_bytes":     strconv.FormatInt(l.MaxStorageBytes, 10),
		"max_branches_per_tree": strconv.Itoa(l.MaxBranchesPerTree),
		"max_members":           strconv.Itoa(l.MaxMembers),
		"requests_per_minute":   strconv.Itoa(l.RequestsPerMinute),
	})
	writeJSON(w, http.StatusOK, l)
	return nil
}

// handleUsage answers GET /api/v1/tenants/{tenant}/usage (tenant:read).
func (a *api) handleUsage(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return err
	}
	if a.d.Limits == nil {
		return errNotImplemented("tenant limits")
	}
	u, err := a.d.Limits.Usage(r.Context(), t)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, u)
	return nil
}

// ---- quota hooks used by the create handlers ------------------------------

// checkTreeQuota enforces MaxTrees before a tree is created.
func (a *api) checkTreeQuota(ctx context.Context, t *domain.Tenant) error {
	if a.d.Limits == nil {
		return nil
	}
	return quotaErr(a.d.Limits.CheckTreeCreate(ctx, t))
}

// checkMemberQuota enforces MaxMembers before a new member is added.
func (a *api) checkMemberQuota(ctx context.Context, t *domain.Tenant) error {
	if a.d.Limits == nil {
		return nil
	}
	return quotaErr(a.d.Limits.CheckMemberAdd(ctx, t))
}

func quotaErr(err error) error {
	var qe *limits.QuotaError
	if errors.As(err, &qe) {
		return errQuota(qe)
	}
	return err
}

// ---- rate limiting ----------------------------------------------------------

// Rate-limit response headers.
const (
	headerRateLimit     = "X-RateLimit-Limit"
	headerRateRemaining = "X-RateLimit-Remaining"
)

// rateLimitMiddleware charges every request (except /healthz and /metrics)
// to the caller's token bucket and answers 429 rate_limited when it is
// empty. It runs inside the authentication middleware so that
// authenticated callers are limited per principal.
func (a *api) rateLimitMiddleware(next http.Handler) http.Handler {
	if a.d.Limits == nil {
		return next
	}
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "/healthz" || r.URL.Path == "/metrics" || r.Method == http.MethodOptions {
			next.ServeHTTP(w, r)
			return
		}
		d := a.d.Limits.Allow(r.Context(), principal(r), remoteHost(r))
		if d.Limit > 0 {
			w.Header().Set(headerRateLimit, strconv.Itoa(d.Limit))
			w.Header().Set(headerRateRemaining, strconv.Itoa(d.Remaining))
		}
		if !d.Allowed {
			opsmetrics.RateLimited.Inc("")
			setRoute(r.Context(), "RATE LIMITED")
			secs := int(math.Ceil(d.RetryAfter.Seconds()))
			if secs < 1 {
				secs = 1
			}
			w.Header().Set("Retry-After", strconv.Itoa(secs))
			writeJSON(w, http.StatusTooManyRequests, errorBody{Error: errorDetail{Code: "rate_limited",
				Message: "rate limit of " + strconv.Itoa(d.Limit) + " requests per minute exceeded; retry in " + strconv.Itoa(secs) + "s"}})
			return
		}
		next.ServeHTTP(w, r)
	})
}

// ---- health -----------------------------------------------------------------

// healthProbeTimeout bounds each /healthz probe.
const healthProbeTimeout = 2 * time.Second

// probeMeta checks that the metadata store answers a point read.
func (a *api) probeMeta(ctx context.Context) error {
	_, err := a.d.Store.GetTenant(ctx, "healthz-probe")
	if err == nil || errors.Is(err, domain.ErrNotFound) {
		return nil
	}
	return err
}

// probeObjects checks that the object store answers (and that its root
// directory still exists for disk stores).
func (a *api) probeObjects(ctx context.Context) error {
	if a.d.Objects == nil {
		return errors.New("no object store")
	}
	if rs, ok := a.d.Objects.(interface{ Root() string }); ok {
		fi, err := os.Stat(rs.Root())
		if err != nil {
			return err
		}
		if !fi.IsDir() {
			return errors.New("object store root is not a directory")
		}
	}
	_, err := a.d.Objects.Has(ctx, hash.Hash{})
	return err
}

// healthChecks runs every probe; it reports whether all passed.
func (a *api) healthChecks(ctx context.Context) (map[string]string, bool) {
	probes := map[string]func(context.Context) error{"meta": a.probeMeta, "objects": a.probeObjects}
	out := make(map[string]string, len(probes))
	ok := true
	for name, probe := range probes {
		pctx, cancel := context.WithTimeout(ctx, healthProbeTimeout)
		err := func() (err error) {
			defer func() {
				if v := recover(); v != nil {
					err = errors.New("probe panicked")
				}
			}()
			return probe(pctx)
		}()
		cancel()
		if err != nil {
			a.log.WarnContext(ctx, "health probe failed", "probe", name, "error", err)
			out[name] = "error"
			ok = false
			continue
		}
		out[name] = "ok"
	}
	return out, ok
}
