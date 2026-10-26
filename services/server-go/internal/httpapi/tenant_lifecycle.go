package httpapi

// Tenant lifecycle routes (docs/ENGINEERING_PLAN.md §16.4): tenant and
// account email verification, slug renames (old slugs answer
// tenant_moved), deletion with a recovery window and restore.

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// codeTenantMoved answers requests naming a renamed tenant's old slug.
const codeTenantMoved = "tenant_moved"

// registerTenantLifecycleRoutes registers the v0.8 routes for tenant email
// verification, renames and deletion (§16.4).
func (a *api) registerTenantLifecycleRoutes() {
	const t = "/api/v1/tenants/{tenant}"
	a.handle("POST "+t+"/email/verify", a.handleVerifyTenantEmail)
	a.handle("POST "+t+"/email/resend", a.handleResendTenantEmail)
	a.handle("POST "+t+"/rename", a.handleRenameTenant)
	a.handle("DELETE "+t, a.handleDeleteTenant)
	a.handle("GET "+t+"/deletion", a.handleGetTenantDeletion)
	a.handle("POST "+t+"/restore", a.handleRestoreTenant)
	a.handle("POST /api/v1/me/email/verify", a.handleVerifyMyEmail)
	a.handle("POST /api/v1/me/email/resend", a.handleResendMyEmail)
}

type emailTokenRequest struct {
	Token string `json:"token"`
}

type renameTenantRequest struct {
	Slug string `json:"slug"`
}

type renameTenantResponse struct {
	Tenant      *domain.Tenant       `json:"tenant"`
	Reservation *iam.SlugReservation `json:"reservation"`
}

// lifecycle returns the service or 501 not_implemented.
func (a *api) lifecycle() (*iam.TenantLifecycle, error) {
	if a.d.TenantLifecycle == nil {
		return nil, errNotImplemented("tenant lifecycle")
	}
	return a.d.TenantLifecycle, nil
}

// lifecycleError renders iam.LifecycleError with its code.
func lifecycleError(err error) error {
	var le *iam.LifecycleError
	if !errors.As(err, &le) {
		return err
	}
	status := http.StatusConflict
	switch {
	case errors.Is(err, domain.ErrNotFound):
		status = http.StatusNotFound
	case errors.Is(err, domain.ErrInvalid):
		status = http.StatusBadRequest
	case errors.Is(err, domain.ErrForbidden):
		status = http.StatusForbidden
	}
	return &apiError{status: status, code: le.Code, msg: le.Msg}
}

func actorName(r *http.Request) string {
	if p := principal(r); p != nil {
		return p.Username
	}
	return "anonymous"
}

// errTenantMoved is the "301-style" answer for a renamed tenant's old
// slug: status 301 without Location (clients do not follow it), code
// tenant_moved and details {old_slug, new_slug}.
func errTenantMoved(oldSlug, newSlug string) error {
	return &apiError{
		status:  http.StatusMovedPermanently,
		code:    codeTenantMoved,
		msg:     fmt.Sprintf("tenant %q was renamed to %q; use the new slug (update worktree remotes with `wt tenant switch %s`)", oldSlug, newSlug, newSlug),
		details: map[string]string{"old_slug": oldSlug, "new_slug": newSlug},
		headers: map[string]string{"Cache-Control": "no-store", "X-WT-Tenant-Moved": newSlug},
	}
}

// tenantByEmailOrMoved backs tenantByRef for references that are neither a
// slug nor an id: a verified tenant email resolves to its tenant, an old
// slug of a renamed tenant answers errTenantMoved. (nil, nil) = unknown.
func (a *api) tenantByEmailOrMoved(ctx context.Context, ref string) (*domain.Tenant, error) {
	if a.d.TenantLifecycle == nil || ref == "" {
		return nil, nil
	}
	if strings.Contains(ref, "@") {
		if t, err := iam.TenantByVerifiedEmail(ctx, a.d.Store, ref); err == nil {
			return t, nil
		}
		return nil, nil
	}
	newSlug, ok, err := iam.MovedSlug(ctx, a.d.Store, ref)
	if err != nil {
		return nil, err
	}
	if !ok {
		return nil, nil
	}
	return nil, errTenantMoved(ref, newSlug)
}

// verifiedEmailTenant resolves a verified tenant email (license grantees).
func (a *api) verifiedEmailTenant(ctx context.Context, email string) (*domain.Tenant, error) {
	return iam.TenantByVerifiedEmail(ctx, a.d.Store, email)
}

// checkSlugNotReserved refuses creating a tenant whose slug is reserved by
// a renamed or purged tenant (409 slug_reserved).
func (a *api) checkSlugNotReserved(ctx context.Context, slug string) error {
	if a.d.TenantLifecycle == nil {
		return nil
	}
	reserved, err := iam.SlugReserved(ctx, a.d.Store, slug, "")
	if err != nil {
		return err
	}
	if reserved {
		return &apiError{status: http.StatusConflict, code: iam.CodeSlugReserved,
			msg: fmt.Sprintf("slug %q is reserved: it was used by a renamed or deleted tenant in the last %d days", slug, int(iam.SlugReservationPeriod.Hours()/24))}
	}
	return nil
}

// onTenantSuspended revokes the tenant's service-account keys when a
// server admin suspends or deactivates it (it was active before).
func (a *api) onTenantSuspended(r *http.Request, t *domain.Tenant, prev, cur string) {
	if a.d.TenantLifecycle == nil || cur == domain.TenantActive ||
		(prev != domain.TenantActive && prev != "") {
		return
	}
	n, err := a.d.TenantLifecycle.RevokeServiceAccountKeys(r.Context(), t.ID)
	if err != nil {
		a.log.WarnContext(r.Context(), "revoking service-account keys of a suspended tenant failed", "tenant", t.Slug, "err", err)
	}
	if n > 0 {
		a.audit(r, "service_account.keys_revoked", t.ID, "tenant", t.ID,
			map[string]string{"slug": t.Slug, "status": cur, "revoked": strconv.Itoa(n)})
	}
}

// requireTenantOwner allows tenant owners and server admins (whose token
// scopes must permit tenant:admin). It does not depend on the tenant's
// status, so owners can restore a tenant scheduled for deletion.
func (a *api) requireTenantOwner(r *http.Request, t *domain.Tenant, what string) (*domain.Principal, error) {
	p, err := requireLogin(r)
	if err != nil {
		return nil, err
	}
	if p.IsServiceAccount() {
		return nil, forbiddenf("service accounts cannot %s a tenant", what)
	}
	if len(p.Scopes) > 0 && !iam.ScopeAllows(p.Scopes, domain.ActTenantAdmin) {
		return nil, forbiddenf("token scopes do not include tenant:admin")
	}
	ctx := r.Context()
	if p.IsAdmin {
		if err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err == nil {
			return p, nil
		}
	}
	role, err := a.d.Authz.RoleOf(ctx, p.AccountID, t.ID)
	if err != nil {
		return nil, err
	}
	if role != domain.RoleOwner {
		return nil, forbiddenf("only owners of %s (or server admins) can %s it", t.Slug, what)
	}
	return p, nil
}

// ---- email ---------------------------------------------------------------

// checkTenantEmailPatch validates PATCH /tenants/{t} {email} before any
// change is written.
func (a *api) checkTenantEmailPatch(email string) error {
	if _, err := a.lifecycle(); err != nil {
		return err
	}
	email = strings.TrimSpace(email)
	if email == "" {
		return nil
	}
	if err := iam.ValidateEmail(email); err != nil {
		return badRequestf("email must be a valid address (local@domain)")
	}
	return nil
}

// patchTenantEmail applies PATCH /tenants/{t} {email}: a new address is
// stored unverified and receives a verification code.
func (a *api) patchTenantEmail(r *http.Request, t *domain.Tenant, email string) (*domain.Tenant, error) {
	l, err := a.lifecycle()
	if err != nil {
		return nil, err
	}
	prev := t.Email
	out, sent, err := l.SetTenantEmail(r.Context(), actorName(r), t, email)
	if err != nil {
		return nil, lifecycleError(err)
	}
	if out.Email != prev {
		details := map[string]string{"slug": out.Slug, "email": out.Email, "previous_email": prev}
		if sent != nil {
			details["delivery"] = sent.Delivery
		}
		a.audit(r, "tenant.email", out.ID, "tenant", out.ID, details)
	}
	return out, nil
}

// handleVerifyTenantEmail answers POST /tenants/{t}/email/verify {token}.
// The code itself proves control of the mailbox, so no sign-in is needed.
func (a *api) handleVerifyTenantEmail(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	t, err := a.tenantByRef(r.Context(), r.PathValue("tenant"))
	if err != nil {
		return err
	}
	var req emailTokenRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if strings.TrimSpace(req.Token) == "" {
		return badRequestf("token is required")
	}
	out, err := l.VerifyTenantEmail(r.Context(), t, req.Token)
	if err != nil {
		return lifecycleError(err)
	}
	a.audit(r, "tenant.email_verified", out.ID, "tenant", out.ID, map[string]string{"slug": out.Slug, "email": out.Email})
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleResendTenantEmail answers POST /tenants/{t}/email/resend
// (tenant:admin): a new code for the unverified tenant email.
func (a *api) handleResendTenantEmail(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantAdmin)
	if err != nil {
		return err
	}
	sent, err := l.ResendTenantVerification(r.Context(), actorName(r), t)
	if err != nil {
		return lifecycleError(err)
	}
	a.audit(r, "tenant.email_verification_sent", t.ID, "tenant", t.ID, map[string]string{"slug": t.Slug, "email": sent.Email, "delivery": sent.Delivery})
	writeJSON(w, http.StatusAccepted, sent)
	return nil
}

// handleVerifyMyEmail answers POST /me/email/verify {token}: verifies the
// caller's account email.
func (a *api) handleVerifyMyEmail(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	if p.IsServiceAccount() || p.AccountID == "" {
		return forbiddenf("service accounts have no email address")
	}
	var req emailTokenRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if strings.TrimSpace(req.Token) == "" {
		return badRequestf("token is required")
	}
	acct, err := l.VerifyAccountEmail(r.Context(), p.AccountID, req.Token)
	if err != nil {
		return lifecycleError(err)
	}
	a.audit(r, "account.email_verified", acct.TenantID, "account", acct.ID, map[string]string{"username": acct.Username, "email": acct.Email})
	writeJSON(w, http.StatusOK, acct)
	return nil
}

// handleResendMyEmail answers POST /me/email/resend: a verification code
// for the caller's account email.
func (a *api) handleResendMyEmail(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	if p.IsServiceAccount() || p.AccountID == "" {
		return forbiddenf("service accounts have no email address")
	}
	sent, err := l.ResendAccountVerification(r.Context(), p.AccountID)
	if err != nil {
		return lifecycleError(err)
	}
	a.audit(r, "account.email_verification_sent", p.TenantID, "account", p.AccountID, map[string]string{"email": sent.Email, "delivery": sent.Delivery})
	writeJSON(w, http.StatusAccepted, sent)
	return nil
}

// ---- rename --------------------------------------------------------------

// handleRenameTenant answers POST /tenants/{t}/rename {slug} (tenant:admin
// and owner): the old slug is reserved for 90 days and answers
// tenant_moved.
func (a *api) handleRenameTenant(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantAdmin)
	if err != nil {
		return err
	}
	if _, err := a.requireTenantOwner(r, t, "rename"); err != nil {
		return err
	}
	var req renameTenantRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Slug = strings.TrimSpace(req.Slug)
	if err := validateSlug(req.Slug); err != nil {
		return err
	}
	old := t.Slug
	out, res, err := l.Rename(r.Context(), actorName(r), t, req.Slug)
	if err != nil {
		return lifecycleError(err)
	}
	a.audit(r, "tenant.rename", out.ID, "tenant", out.ID, map[string]string{
		"slug": out.Slug, "previous_slug": old, "reserved_until": res.ExpiresAt.Format(time.RFC3339)})
	writeJSON(w, http.StatusOK, renameTenantResponse{Tenant: out, Reservation: res})
	return nil
}

// ---- deletion ------------------------------------------------------------

// handleDeleteTenant answers DELETE /tenants/{t} (owner): 202 with the
// scheduled deletion. The tenant is suspended (read-only) until the
// recovery window ends and the purge job deletes it.
func (a *api) handleDeleteTenant(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, err := a.tenantByRef(r.Context(), r.PathValue("tenant"))
	if err != nil {
		return err
	}
	if err := a.d.Authz.Authorize(r.Context(), principal(r), domain.ActTenantRead, t.ID, nil); err != nil {
		return err
	}
	if _, err := a.requireTenantOwner(r, t, "delete"); err != nil {
		return err
	}
	d, err := l.Delete(r.Context(), actorName(r), t)
	if err != nil {
		return lifecycleError(err)
	}
	a.audit(r, "tenant.delete", t.ID, "tenant", t.ID, map[string]string{
		"slug": t.Slug, "purge_at": d.PurgeAt.Format(time.RFC3339), "revoked_keys": strconv.Itoa(d.RevokedKeys)})
	writeJSON(w, http.StatusAccepted, d)
	return nil
}

// handleGetTenantDeletion answers GET /tenants/{t}/deletion (tenant:read):
// the scheduled deletion, or 404 deletion_not_scheduled.
func (a *api) handleGetTenantDeletion(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return err
	}
	d, err := l.Deletion(r.Context(), t.ID)
	if err != nil {
		return lifecycleError(err)
	}
	writeJSON(w, http.StatusOK, d)
	return nil
}

// handleRestoreTenant answers POST /tenants/{t}/restore (owner): cancels a
// scheduled deletion during the recovery window.
func (a *api) handleRestoreTenant(w http.ResponseWriter, r *http.Request) error {
	l, err := a.lifecycle()
	if err != nil {
		return err
	}
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, err := a.tenantByRef(r.Context(), r.PathValue("tenant"))
	if err != nil {
		return err
	}
	if err := a.d.Authz.Authorize(r.Context(), principal(r), domain.ActTenantRead, t.ID, nil); err != nil {
		return err
	}
	if _, err := a.requireTenantOwner(r, t, "restore"); err != nil {
		return err
	}
	out, err := l.Restore(r.Context(), t)
	if err != nil {
		return lifecycleError(err)
	}
	a.audit(r, "tenant.restore", out.ID, "tenant", out.ID, map[string]string{"slug": out.Slug})
	writeJSON(w, http.StatusOK, out)
	return nil
}
