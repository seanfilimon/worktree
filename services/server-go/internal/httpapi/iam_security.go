package httpapi

import (
	"errors"
	"net/http"
	"sort"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

type loginMFARequest struct {
	MFAToken string `json:"mfa_token"`
	Code     string `json:"code"`
}

type sessionView struct {
	domain.APIToken
	Current bool `json:"current"`
}

type revokedResponse struct {
	Revoked int `json:"revoked"`
}

type mfaEnrollResponse struct {
	Secret     string `json:"secret"`
	OTPAuthURL string `json:"otpauth_url"`
}

type mfaCodeRequest struct {
	Code string `json:"code"`
}

type mfaDisableRequest struct {
	Password string `json:"password"`
}

type recoveryCodesResponse struct {
	RecoveryCodes []string `json:"recovery_codes"`
}

// secondFactorError maps a rejected code or password from AccountSecurity
// to 403: a 401 would make clients drop their (valid) session.
func secondFactorError(err error, msg string) error {
	if errors.Is(err, domain.ErrUnauthenticated) || errors.Is(err, domain.ErrForbidden) {
		return forbiddenf("%s", msg)
	}
	return err
}

// normalizeCode strips the spaces users type into one-time codes.
func normalizeCode(s string) string { return strings.ReplaceAll(strings.TrimSpace(s), " ", "") }

// handleLoginMFA answers POST /api/v1/auth/login/mfa: the second login
// step, exchanging the mfa_token and a TOTP or recovery code for a session.
func (a *api) handleLoginMFA(w http.ResponseWriter, r *http.Request) error {
	sec, err := a.security()
	if err != nil {
		return err
	}
	var req loginMFARequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.MFAToken = strings.TrimSpace(req.MFAToken)
	req.Code = normalizeCode(req.Code)
	if req.MFAToken == "" || req.Code == "" {
		return badRequestf("mfa_token and code are required")
	}
	if len(req.Code) > 64 || len(req.MFAToken) > 512 {
		return badRequestf("mfa_token or code is too long")
	}
	token, acct, err := sec.CompleteMFALogin(r.Context(), req.MFAToken, req.Code, sessionMeta(r))
	if err != nil {
		if errors.Is(err, domain.ErrUnauthenticated) || errors.Is(err, domain.ErrNotFound) || errors.Is(err, domain.ErrInvalid) {
			a.auditAs(r, &domain.Account{Username: "anonymous"}, "auth.login_failed", "", "account", "",
				map[string]string{"step": "mfa"})
			return &apiError{status: http.StatusUnauthorized, code: codeUnauthenticated, msg: "invalid or expired code"}
		}
		return err
	}
	if acct == nil {
		return errors.New("mfa login returned no account")
	}
	a.auditAs(r, acct, "auth.login", acct.TenantID, "account", acct.ID, map[string]string{"mfa": "true"})
	writeJSON(w, http.StatusOK, sessionResponse{Token: token, Account: acct})
	return nil
}

// handleListSessions answers GET /api/v1/auth/sessions: the caller's
// sessions, newest first, with the one used by this request marked current.
func (a *api) handleListSessions(w http.ResponseWriter, r *http.Request) error {
	p, err := requireUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	toks, err := sec.ListSessions(r.Context(), p.AccountID)
	if err != nil {
		return err
	}
	cur := iam.TokenHashFrom(r.Context())
	out := make([]sessionView, 0, len(toks))
	for _, t := range toks {
		out = append(out, sessionView{APIToken: t, Current: cur != "" && t.TokenHash == cur})
	}
	sort.SliceStable(out, func(i, j int) bool { return out[i].CreatedAt.After(out[j].CreatedAt) })
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleRevokeSession answers DELETE /api/v1/auth/sessions/{id}.
func (a *api) handleRevokeSession(w http.ResponseWriter, r *http.Request) error {
	p, err := requireFullUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	ctx := r.Context()
	id := r.PathValue("id")
	toks, err := sec.ListSessions(ctx, p.AccountID)
	if err != nil {
		return err
	}
	found := false
	for _, t := range toks {
		if t.ID == id {
			found = true
			break
		}
	}
	if !found {
		return notFoundf("session %q not found", id)
	}
	if err := a.d.Auth.RevokeToken(ctx, p.AccountID, id); err != nil {
		return err
	}
	a.audit(r, "session.revoke", p.TenantID, "token", id, nil)
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleRevokeOtherSessions answers POST /api/v1/auth/sessions/revoke-others.
func (a *api) handleRevokeOtherSessions(w http.ResponseWriter, r *http.Request) error {
	p, err := requireFullUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	n, err := sec.RevokeOtherSessions(r.Context(), p.AccountID, iam.TokenHashFrom(r.Context()))
	if err != nil {
		return err
	}
	a.audit(r, "session.revoke_others", p.TenantID, "account", p.AccountID, map[string]string{"revoked": strconv.Itoa(n)})
	writeJSON(w, http.StatusOK, revokedResponse{Revoked: n})
	return nil
}

// handleMFAStatus answers GET /api/v1/auth/mfa.
func (a *api) handleMFAStatus(w http.ResponseWriter, r *http.Request) error {
	p, err := requireUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	st, err := sec.MFAStatus(r.Context(), p.AccountID)
	if err != nil {
		return err
	}
	if st == nil {
		st = &domain.MFAState{AccountID: p.AccountID}
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// handleMFAEnroll answers POST /api/v1/auth/mfa/totp/enroll. The secret is
// shown once; it takes effect after /confirm.
func (a *api) handleMFAEnroll(w http.ResponseWriter, r *http.Request) error {
	p, err := requireFullUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	secret, url, err := sec.BeginTOTPEnroll(r.Context(), p.AccountID)
	if err != nil {
		return err
	}
	a.audit(r, "mfa.enroll", p.TenantID, "account", p.AccountID, nil)
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, mfaEnrollResponse{Secret: secret, OTPAuthURL: url})
	return nil
}

// decodeCode reads a {code} body.
func decodeCode(w http.ResponseWriter, r *http.Request) (string, error) {
	var req mfaCodeRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return "", err
	}
	code := normalizeCode(req.Code)
	if code == "" {
		return "", badRequestf("code is required")
	}
	if len(code) > 64 {
		return "", badRequestf("code is too long")
	}
	return code, nil
}

// handleMFAConfirm answers POST /api/v1/auth/mfa/totp/confirm, enabling TOTP
// and returning the recovery codes (shown once).
func (a *api) handleMFAConfirm(w http.ResponseWriter, r *http.Request) error {
	p, err := requireFullUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	code, err := decodeCode(w, r)
	if err != nil {
		return err
	}
	codes, err := sec.ConfirmTOTP(r.Context(), p.AccountID, code)
	if err != nil {
		return secondFactorError(err, "invalid code")
	}
	a.audit(r, "mfa.enable", p.TenantID, "account", p.AccountID, nil)
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, recoveryCodesResponse{RecoveryCodes: nonNil(codes)})
	return nil
}

// handleMFADisable answers DELETE /api/v1/auth/mfa/totp {password}.
func (a *api) handleMFADisable(w http.ResponseWriter, r *http.Request) error {
	p, err := requireFullUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	var req mfaDisableRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if req.Password == "" {
		return badRequestf("password is required")
	}
	if err := sec.DisableTOTP(r.Context(), p.AccountID, req.Password); err != nil {
		return secondFactorError(err, "invalid password")
	}
	a.audit(r, "mfa.disable", p.TenantID, "account", p.AccountID, nil)
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleMFARecoveryCodes answers POST /api/v1/auth/mfa/recovery-codes
// {code}: replaces the recovery codes after verifying a current code.
func (a *api) handleMFARecoveryCodes(w http.ResponseWriter, r *http.Request) error {
	p, err := requireFullUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	code, err := decodeCode(w, r)
	if err != nil {
		return err
	}
	codes, err := sec.RegenerateRecoveryCodes(r.Context(), p.AccountID, code)
	if err != nil {
		return secondFactorError(err, "invalid code")
	}
	a.audit(r, "mfa.recovery_codes", p.TenantID, "account", p.AccountID, nil)
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, recoveryCodesResponse{RecoveryCodes: nonNil(codes)})
	return nil
}
