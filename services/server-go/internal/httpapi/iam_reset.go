package httpapi

import (
	"errors"
	"net/http"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

const (
	defaultResetTTLHours = 1
	maxResetTTLHours     = 7 * 24
)

type resetCodeRequest struct {
	TTLHours int `json:"ttl_hours"`
}

type resetCodeResponse struct {
	Code      string    `json:"code"`
	ExpiresAt time.Time `json:"expires_at"`
}

type passwordResetRequest struct {
	Code        string `json:"code"`
	NewPassword string `json:"new_password"`
}

// handleAdminResetCode answers POST
// /api/v1/admin/accounts/{username}/reset-code {ttl_hours?} (server admin).
// The one-time code is returned once; there is no email delivery.
func (a *api) handleAdminResetCode(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	if err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	req := resetCodeRequest{TTLHours: defaultResetTTLHours}
	if err := decodeOptionalJSON(w, r, &req); err != nil {
		return err
	}
	if req.TTLHours == 0 {
		req.TTLHours = defaultResetTTLHours
	}
	if req.TTLHours < 1 || req.TTLHours > maxResetTTLHours {
		return badRequestf("ttl_hours must be between 1 and %d", maxResetTTLHours)
	}
	acct, err := a.accountByUsername(ctx, r.PathValue("username"))
	if err != nil {
		return err
	}
	code, exp, err := sec.CreatePasswordReset(ctx, acct.ID, p.Username, time.Duration(req.TTLHours)*time.Hour)
	if err != nil {
		return err
	}
	a.audit(r, "password.reset_code", acct.TenantID, "account", acct.ID,
		map[string]string{"username": acct.Username, "expires_at": exp.UTC().Format(time.RFC3339)})
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, resetCodeResponse{Code: code, ExpiresAt: exp.UTC()})
	return nil
}

// handlePasswordReset answers POST /api/v1/auth/password-reset (public):
// redeems a reset code and sets a new password.
func (a *api) handlePasswordReset(w http.ResponseWriter, r *http.Request) error {
	sec, err := a.security()
	if err != nil {
		return err
	}
	var req passwordResetRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Code = strings.TrimSpace(req.Code)
	if req.Code == "" || len(req.Code) > 256 {
		return badRequestf("code is required")
	}
	if err := iam.ValidatePassword(req.NewPassword); err != nil {
		return err
	}
	ctx := r.Context()
	// Look the code up first (best effort) to attribute the audit record;
	// ResetPassword consumes it.
	var acct *domain.Account
	if pr, err := a.d.Store.GetPasswordReset(ctx, hashResetCode(req.Code)); err == nil {
		acct, _ = a.d.Store.GetAccount(ctx, pr.AccountID)
	}
	if err := sec.ResetPassword(ctx, req.Code, req.NewPassword); err != nil {
		if errors.Is(err, domain.ErrNotFound) || errors.Is(err, domain.ErrUnauthenticated) || errors.Is(err, domain.ErrForbidden) {
			return badRequestf("invalid or expired reset code")
		}
		return err
	}
	if acct != nil {
		a.auditAs(r, acct, "password.reset", acct.TenantID, "account", acct.ID, nil)
	} else {
		a.audit(r, "password.reset", "", "account", "", nil)
	}
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// hashResetCode is the storage hash of a one-time code
// (domain.PasswordReset.CodeHash: hex sha256).
func hashResetCode(code string) string { return iam.HashToken(code) }
