package httpapi

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// maxTokenTTLHours caps API token lifetimes (5 years).
const maxTokenTTLHours = 5 * 365 * 24

type registerRequest struct {
	Username string `json:"username"`
	Email    string `json:"email"`
	Password string `json:"password"`
}

type loginRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

type sessionResponse struct {
	Token   string          `json:"token"`
	Account *domain.Account `json:"account"`
}

type membershipView struct {
	Tenant domain.Tenant `json:"tenant"`
	Role   string        `json:"role"`
}

type meResponse struct {
	Account     *domain.Account  `json:"account"`
	Memberships []membershipView `json:"memberships"`
}

type createTokenRequest struct {
	Name     string `json:"name"`
	TTLHours int    `json:"ttl_hours"`
}

type createTokenResponse struct {
	Token    string           `json:"token"`
	APIToken *domain.APIToken `json:"api_token"`
}

// handleRegister answers POST /api/v1/auth/register.
func (a *api) handleRegister(w http.ResponseWriter, r *http.Request) error {
	var req registerRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Username = strings.TrimSpace(req.Username)
	req.Email = strings.TrimSpace(req.Email)
	if err := validateSlug(req.Username); err != nil {
		return badRequestf("username must be 1-63 characters of lowercase letters, digits and '-', not starting or ending with '-'")
	}
	if err := validateText("email", req.Email, 254, true); err != nil {
		return err
	}
	if at := strings.LastIndexByte(req.Email, '@'); at < 1 || at == len(req.Email)-1 {
		return badRequestf("email must be a valid address")
	}
	if len(req.Password) < 8 || len(req.Password) > 1024 {
		return badRequestf("password must be 8-1024 bytes")
	}
	ctx := r.Context()
	acct, err := a.d.Auth.Register(ctx, req.Username, req.Email, req.Password)
	if err != nil {
		return err
	}
	token, acct2, err := a.d.Auth.Login(ctx, req.Username, req.Password)
	if err != nil {
		return err
	}
	if acct2 != nil {
		acct = acct2
	}
	a.auditAs(r, acct, "auth.register", acct.TenantID, "account", acct.ID, nil)
	writeJSON(w, http.StatusCreated, sessionResponse{Token: token, Account: acct})
	return nil
}

// handleLogin answers POST /api/v1/auth/login.
func (a *api) handleLogin(w http.ResponseWriter, r *http.Request) error {
	var req loginRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Username = strings.TrimSpace(req.Username)
	if req.Username == "" || req.Password == "" {
		return badRequestf("username and password are required")
	}
	token, acct, err := a.d.Auth.Login(r.Context(), req.Username, req.Password)
	if err != nil {
		if errors.Is(err, domain.ErrUnauthenticated) || errors.Is(err, domain.ErrNotFound) {
			a.auditAs(r, &domain.Account{Username: req.Username}, "auth.login_failed", "", "account", "", nil)
			// Never reveal whether the username exists.
			return &apiError{status: http.StatusUnauthorized, code: codeUnauthenticated, msg: "invalid username or password"}
		}
		return err
	}
	a.auditAs(r, acct, "auth.login", acct.TenantID, "account", acct.ID, nil)
	writeJSON(w, http.StatusOK, sessionResponse{Token: token, Account: acct})
	return nil
}

// auditAs records an audit event for an actor that is not (yet) the
// request principal (register / login).
func (a *api) auditAs(r *http.Request, acct *domain.Account, action, tenantID, resType, resID string, details map[string]string) {
	if a.d.Audit == nil {
		return
	}
	a.d.Audit.Record(r.Context(), domain.AuditEvent{
		ID:           newID(),
		TenantID:     tenantID,
		ActorID:      acct.ID,
		Actor:        acct.Username,
		Action:       action,
		ResourceType: resType,
		ResourceID:   resID,
		Details:      details,
		RemoteAddr:   remoteHost(r),
		At:           time.Now().UTC(),
	})
}

// bearerToken extracts the raw bearer token from the Authorization header.
func bearerToken(r *http.Request) string {
	h := r.Header.Get("Authorization")
	if len(h) > 7 && strings.EqualFold(h[:7], "bearer ") {
		return strings.TrimSpace(h[7:])
	}
	return ""
}

// handleLogout answers POST /api/v1/auth/logout by revoking the token used
// to authenticate the request.
func (a *api) handleLogout(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	raw := bearerToken(r)
	if raw == "" {
		return errLoginRequired
	}
	sum := sha256.Sum256([]byte(raw))
	tok, err := a.d.Store.GetTokenByHash(r.Context(), hex.EncodeToString(sum[:]))
	switch {
	case errors.Is(err, domain.ErrNotFound):
		// Already revoked or not a stored token: logout is idempotent.
	case err != nil:
		return err
	case tok.AccountID != p.AccountID:
		return forbiddenf("token does not belong to the caller")
	default:
		if err := a.d.Auth.RevokeToken(r.Context(), p.AccountID, tok.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
		a.audit(r, "auth.logout", p.TenantID, "token", tok.ID, nil)
	}
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleMe answers GET /api/v1/auth/me.
func (a *api) handleMe(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	acct, err := a.d.Store.GetAccount(ctx, p.AccountID)
	if err != nil {
		return err
	}
	ms, err := a.d.Store.ListAccountMemberships(ctx, p.AccountID)
	if err != nil {
		return err
	}
	views := make([]membershipView, 0, len(ms))
	for _, m := range ms {
		t, err := a.d.Store.GetTenant(ctx, m.TenantID)
		if errors.Is(err, domain.ErrNotFound) {
			continue // dangling membership
		}
		if err != nil {
			return err
		}
		views = append(views, membershipView{Tenant: *t, Role: m.Role})
	}
	sort.Slice(views, func(i, j int) bool { return views[i].Tenant.Slug < views[j].Tenant.Slug })
	writeJSON(w, http.StatusOK, meResponse{Account: acct, Memberships: views})
	return nil
}

// handleListTokens answers GET /api/v1/auth/tokens.
func (a *api) handleListTokens(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	toks, err := a.d.Store.ListTokens(r.Context(), p.AccountID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(toks))
	return nil
}

// handleCreateToken answers POST /api/v1/auth/tokens.
func (a *api) handleCreateToken(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	var req createTokenRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Name = strings.TrimSpace(req.Name)
	if err := validateText("name", req.Name, 100, true); err != nil {
		return err
	}
	if req.TTLHours < 0 || req.TTLHours > maxTokenTTLHours {
		return badRequestf("ttl_hours must be between 0 (no expiry) and %d", maxTokenTTLHours)
	}
	token, t, err := a.d.Auth.CreateToken(r.Context(), p.AccountID, req.Name, time.Duration(req.TTLHours)*time.Hour)
	if err != nil {
		return err
	}
	a.audit(r, "token.create", p.TenantID, "token", t.ID, map[string]string{"name": t.Name})
	writeJSON(w, http.StatusOK, createTokenResponse{Token: token, APIToken: t})
	return nil
}

// handleRevokeToken answers DELETE /api/v1/auth/tokens/{id}.
func (a *api) handleRevokeToken(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	id := r.PathValue("id")
	if err := a.d.Auth.RevokeToken(r.Context(), p.AccountID, id); err != nil {
		return err
	}
	a.audit(r, "token.revoke", p.TenantID, "token", id, nil)
	w.WriteHeader(http.StatusNoContent)
	return nil
}
