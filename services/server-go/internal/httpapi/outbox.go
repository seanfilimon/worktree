package httpapi

// Server operations (docs/ENGINEERING_PLAN.md §16.11): the durable email
// outbox (GET /admin/outbox, retry, delete) and background retention
// (GET /admin/retention, POST /admin/retention/run). Server admins only.

import (
	"context"
	"errors"
	"net/http"
	"strconv"

	"github.com/seanfilimon/worktree/services/server-go/internal/notify"
	"github.com/seanfilimon/worktree/services/server-go/internal/retention"
)

// registerServerOpsRoutes registers the §16.11 routes (called from
// registerBackupRoutes).
func (a *api) registerServerOpsRoutes() {
	a.handle("GET /api/v1/admin/outbox", a.handleListOutbox)
	a.handle("POST /api/v1/admin/outbox/retry", a.handleRetryDeadEmails)
	a.handle("POST /api/v1/admin/outbox/{id}/retry", a.handleRetryEmail)
	a.handle("DELETE /api/v1/admin/outbox/{id}", a.handleDeleteEmail)
	a.handle("GET /api/v1/admin/retention", a.handleRetentionStatus)
	a.handle("POST /api/v1/admin/retention/run", a.handleRunRetention)
}

// RetryDeadResponse is the response of POST /api/v1/admin/outbox/retry.
type RetryDeadResponse struct {
	// Retried is the number of dead-lettered emails queued again.
	Retried int `json:"retried"`
}

// Audit actions of the outbox and retention routes.
const (
	actionOutboxRetry  = "outbox.retry"
	actionOutboxDelete = "outbox.delete"
)

func outboxErr(err error) error {
	if errors.Is(err, notify.ErrAlreadySent) {
		return &apiError{status: http.StatusConflict, code: "already_sent", msg: "the email was already sent"}
	}
	return err
}

// handleListOutbox answers GET /api/v1/admin/outbox?status=&limit=.
func (a *api) handleListOutbox(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Notify == nil {
		return errNotImplemented("the email outbox")
	}
	limit, err := queryInt(r, "limit", notify.DefaultOutboxListLimit, notify.MaxOutboxListLimit)
	if err != nil {
		return err
	}
	list, err := a.d.Notify.Outbox(r.Context(), r.URL.Query().Get("status"), limit)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, list)
	return nil
}

// handleRetryEmail answers POST /api/v1/admin/outbox/{id}/retry.
func (a *api) handleRetryEmail(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Notify == nil {
		return errNotImplemented("the email outbox")
	}
	e, err := a.d.Notify.RetryEmail(r.Context(), r.PathValue("id"))
	if err != nil {
		return outboxErr(err)
	}
	a.audit(r, actionOutboxRetry, "", "email", e.ID, nil)
	writeJSON(w, http.StatusOK, e)
	return nil
}

// handleRetryDeadEmails answers POST /api/v1/admin/outbox/retry.
func (a *api) handleRetryDeadEmails(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Notify == nil {
		return errNotImplemented("the email outbox")
	}
	n, err := a.d.Notify.RetryDeadEmails(r.Context())
	if err != nil {
		return err
	}
	if n > 0 {
		a.audit(r, actionOutboxRetry, "", "email", "", map[string]string{"dead": strconv.Itoa(n)})
	}
	writeJSON(w, http.StatusOK, RetryDeadResponse{Retried: n})
	return nil
}

// handleDeleteEmail answers DELETE /api/v1/admin/outbox/{id} (204).
func (a *api) handleDeleteEmail(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Notify == nil {
		return errNotImplemented("the email outbox")
	}
	id := r.PathValue("id")
	if err := a.d.Notify.DeleteEmail(r.Context(), id); err != nil {
		return err
	}
	a.audit(r, actionOutboxDelete, "", "email", id, nil)
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleRetentionStatus answers GET /api/v1/admin/retention.
func (a *api) handleRetentionStatus(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Retention == nil {
		return errNotImplemented("retention")
	}
	writeJSON(w, http.StatusOK, a.d.Retention.Status())
	return nil
}

// handleRunRetention answers POST /api/v1/admin/retention/run with the
// pass's report.
func (a *api) handleRunRetention(w http.ResponseWriter, r *http.Request) error {
	if err := a.requireServerAdmin(r); err != nil {
		return err
	}
	if a.d.Retention == nil {
		return errNotImplemented("retention")
	}
	rep, err := a.d.Retention.Run(context.WithoutCancel(r.Context()))
	if errors.Is(err, retention.ErrRunning) {
		return &apiError{status: http.StatusConflict, code: "retention_running", msg: "a retention pass is already running"}
	}
	if err != nil && rep == nil {
		return err
	}
	// A partial failure still reports what was pruned; the error is in
	// GET /admin/retention's last_error and the server log.
	writeJSON(w, http.StatusOK, rep)
	return nil
}
