package httpapi

import (
	"crypto/subtle"
	"log/slog"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/sso"
)

// ssoLoginPage is where failed single sign-on attempts land; the admin
// panel reads the message from the #sso_error fragment.
const ssoLoginPage = "/admin/login"

// ssoStateCookie binds a started login to the browser that started it, so
// that a callback URL captured by an attacker cannot sign a victim in to
// the attacker's account (login CSRF).
const (
	ssoStateCookie     = "wt_sso_state"
	ssoStateCookiePath = "/api/v1/auth/sso/"
	ssoStateCookieTTL  = 10 * time.Minute
)

// registerSSORoutes registers the OpenID Connect endpoints of
// ENGINEERING_PLAN §12: /auth/providers, /auth/sso/start,
// /auth/sso/callback and /admin/sso.
func (a *api) registerSSORoutes() {
	a.handle("GET /api/v1/auth/providers", a.handleAuthProviders)
	a.handle("GET /api/v1/auth/sso/start", a.handleSSOStart)
	a.handle("GET /api/v1/auth/sso/callback", a.handleSSOCallback)
	a.handle("GET /api/v1/admin/sso", a.handleGetSSOConfig)
	a.handle("PUT /api/v1/admin/sso", a.handlePutSSOConfig)
}

type ssoProviderInfo struct {
	Enabled bool   `json:"enabled"`
	Name    string `json:"name"`
}

type authProvidersResponse struct {
	Password     bool            `json:"password"`
	Registration bool            `json:"registration"`
	SSO          ssoProviderInfo `json:"sso"`
}

// handleAuthProviders answers GET /api/v1/auth/providers (public).
func (a *api) handleAuthProviders(w http.ResponseWriter, r *http.Request) error {
	resp := authProvidersResponse{Password: true, Registration: a.d.AllowRegistration}
	if a.d.SSO != nil {
		cfg, err := a.d.SSO.Config(r.Context())
		if err != nil {
			// The login page must keep working; report SSO as unavailable.
			a.log.WarnContext(r.Context(), "loading sso configuration failed", slog.Any("error", err))
		} else if cfg.Enabled && cfg.Issuer != "" && cfg.ClientID != "" {
			resp.SSO = ssoProviderInfo{Enabled: true, Name: cfg.Name}
			if resp.SSO.Name == "" {
				resp.SSO.Name = sso.DefaultName
			}
		}
	}
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, resp)
	return nil
}

// handleSSOStart answers GET /api/v1/auth/sso/start?redirect=/admin/ with a
// redirect to the identity provider. Failures redirect to the login page
// with #sso_error, since this endpoint is navigated to by browsers.
func (a *api) handleSSOStart(w http.ResponseWriter, r *http.Request) error {
	ssoNoStore(w)
	if a.d.SSO == nil {
		a.ssoFail(w, r, "single sign-on is not enabled")
		return nil
	}
	redirect, ok := sso.SafeRedirect(r.URL.Query().Get("redirect"))
	if !ok {
		a.ssoFail(w, r, "redirect must be a path under /admin/")
		return nil
	}
	authURL, err := a.d.SSO.AuthURL(r.Context(), redirect)
	if err != nil {
		a.ssoFail(w, r, a.ssoMessage(r, err))
		return nil
	}
	u, err := url.Parse(authURL)
	if err != nil || u.Query().Get("state") == "" {
		a.log.ErrorContext(r.Context(), "sso provider returned an unusable authorization URL")
		a.ssoFail(w, r, "internal server error")
		return nil
	}
	http.SetCookie(w, &http.Cookie{
		Name:     ssoStateCookie,
		Value:    u.Query().Get("state"),
		Path:     ssoStateCookiePath,
		MaxAge:   int(ssoStateCookieTTL / time.Second),
		HttpOnly: true,
		Secure:   ssoSecureRequest(r),
		// Lax: the cookie must accompany the top-level GET navigation
		// back from the identity provider.
		SameSite: http.SameSiteLaxMode,
	})
	http.Redirect(w, r, authURL, http.StatusFound)
	return nil
}

// handleSSOCallback answers GET /api/v1/auth/sso/callback?code=&state= by
// completing the login and redirecting to <redirect>#sso_token=<token>.
func (a *api) handleSSOCallback(w http.ResponseWriter, r *http.Request) error {
	ssoNoStore(w)
	q := r.URL.Query()
	state := q.Get("state")
	// The binding cookie is single use, whatever the outcome.
	cookie, _ := r.Cookie(ssoStateCookie)
	http.SetCookie(w, &http.Cookie{
		Name: ssoStateCookie, Value: "", Path: ssoStateCookiePath, MaxAge: -1,
		HttpOnly: true, Secure: ssoSecureRequest(r), SameSite: http.SameSiteLaxMode,
	})
	if a.d.SSO == nil {
		a.ssoFail(w, r, "single sign-on is not enabled")
		return nil
	}
	if e := q.Get("error"); e != "" {
		msg := "the identity provider reported an error"
		if c := ssoOAuthCode(e); c != "" {
			msg += " (" + c + ")"
		}
		a.auditAs(r, &domain.Account{Username: "anonymous"}, "sso.login_failed", "", "sso", "", map[string]string{"reason": msg})
		a.ssoFail(w, r, msg)
		return nil
	}
	if state == "" || cookie == nil || subtle.ConstantTimeCompare([]byte(cookie.Value), []byte(state)) != 1 {
		a.ssoFail(w, r, "this sign-in was not started from this browser; please start again")
		return nil
	}
	meta := domain.SessionMeta{RemoteIP: remoteHost(r), UserAgent: ssoUserAgent(r.UserAgent())}
	token, acct, redirect, err := a.d.SSO.Callback(r.Context(), q.Get("code"), state, meta)
	if err != nil {
		msg := a.ssoMessage(r, err)
		a.auditAs(r, &domain.Account{Username: "anonymous"}, "sso.login_failed", "", "sso", "", map[string]string{"reason": msg})
		a.ssoFail(w, r, msg)
		return nil
	}
	target, ok := sso.SafeRedirect(redirect)
	if !ok {
		target = sso.DefaultRedirect
	}
	a.auditAs(r, acct, "sso.login", acct.TenantID, "account", acct.ID, map[string]string{"method": "oidc"})
	http.Redirect(w, r, target+"#sso_token="+url.QueryEscape(token), http.StatusFound)
	return nil
}

// ssoFail redirects to the login page with the message in #sso_error.
func (a *api) ssoFail(w http.ResponseWriter, r *http.Request, msg string) {
	http.Redirect(w, r, ssoLoginPage+"#sso_error="+url.QueryEscape(msg), http.StatusFound)
}

// ssoMessage returns the client-safe message for err (server errors are
// logged and reported generically).
func (a *api) ssoMessage(r *http.Request, err error) string {
	status, _, msg := classify(err)
	if status >= 500 {
		a.log.ErrorContext(r.Context(), "sso request failed",
			slog.String("request_id", requestIDFrom(r.Context())), slog.Any("error", err))
		return "internal server error"
	}
	return msg
}

func ssoNoStore(w http.ResponseWriter) {
	w.Header().Set("Cache-Control", "no-store")
	w.Header().Set("Pragma", "no-cache")
	w.Header().Set("Referrer-Policy", "no-referrer")
}

// isSecureRequest reports whether the client reached us over TLS (directly
// or through a proxy that says so).
func ssoSecureRequest(r *http.Request) bool {
	return r.TLS != nil || strings.EqualFold(r.Header.Get("X-Forwarded-Proto"), "https")
}

func ssoUserAgent(ua string) string {
	const max = 512
	if len(ua) > max {
		return ua[:max]
	}
	return ua
}

// oauthErrorCode keeps an OAuth error code only if it is a plain token.
func ssoOAuthCode(s string) string {
	if len(s) > 64 {
		return ""
	}
	for _, c := range s {
		if !(c >= 'a' && c <= 'z' || c >= 'A' && c <= 'Z' || c >= '0' && c <= '9' || c == '_' || c == '-' || c == '.') {
			return ""
		}
	}
	return s
}

// requireServerAdmin authorizes the caller as a server administrator.
func (a *api) ssoRequireAdmin(r *http.Request) (*domain.Principal, error) {
	p, err := requireLogin(r)
	if err != nil {
		return nil, err
	}
	if err := a.d.Authz.Authorize(r.Context(), p, domain.ActServerAdmin, "", nil); err != nil {
		return nil, err
	}
	return p, nil
}

var errSSOUnavailable = notFoundf("single sign-on is not available on this server")

// handleGetSSOConfig answers GET /api/v1/admin/sso (server admin).
func (a *api) handleGetSSOConfig(w http.ResponseWriter, r *http.Request) error {
	if _, err := a.ssoRequireAdmin(r); err != nil {
		return err
	}
	if a.d.SSO == nil {
		return errSSOUnavailable
	}
	cfg, err := a.d.SSO.Config(r.Context())
	if err != nil {
		return err
	}
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, nonNilSSO(cfg))
	return nil
}

// putSSORequest is the body of PUT /api/v1/admin/sso. has_client_secret
// and updated_at are accepted (and ignored) so that a GET response can be
// sent back as is.
type putSSORequest struct {
	Enabled        bool       `json:"enabled"`
	Name           string     `json:"name"`
	Issuer         string     `json:"issuer"`
	ClientID       string     `json:"client_id"`
	ClientSecret   *string    `json:"client_secret,omitempty"`
	RedirectURL    string     `json:"redirect_url"`
	Scopes         []string   `json:"scopes"`
	AllowedDomains []string   `json:"allowed_domains"`
	AutoCreate     bool       `json:"auto_create"`
	UsernameClaim  string     `json:"username_claim"`
	HasSecret      *bool      `json:"has_client_secret,omitempty"`
	UpdatedAt      *time.Time `json:"updated_at,omitempty"`
}

// handlePutSSOConfig answers PUT /api/v1/admin/sso (server admin).
func (a *api) handlePutSSOConfig(w http.ResponseWriter, r *http.Request) error {
	if _, err := a.ssoRequireAdmin(r); err != nil {
		return err
	}
	if a.d.SSO == nil {
		return errSSOUnavailable
	}
	var req putSSORequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	cfg, err := a.d.SSO.SaveConfig(r.Context(), &domain.SSOConfig{
		Enabled:        req.Enabled,
		Name:           req.Name,
		Issuer:         req.Issuer,
		ClientID:       req.ClientID,
		RedirectURL:    req.RedirectURL,
		Scopes:         req.Scopes,
		AllowedDomains: req.AllowedDomains,
		AutoCreate:     req.AutoCreate,
		UsernameClaim:  req.UsernameClaim,
	}, req.ClientSecret)
	if err != nil {
		return err
	}
	a.audit(r, "sso.config.update", "", "sso", "config", map[string]string{
		"enabled":        strconv.FormatBool(cfg.Enabled),
		"issuer":         cfg.Issuer,
		"client_id":      cfg.ClientID,
		"auto_create":    strconv.FormatBool(cfg.AutoCreate),
		"secret_changed": strconv.FormatBool(req.ClientSecret != nil),
	})
	writeJSON(w, http.StatusOK, nonNilSSO(cfg))
	return nil
}

// nonNilSSO makes list fields encode as [] and guarantees the secret is
// never serialized (it is json:"-" already; this is defence in depth).
func nonNilSSO(c *domain.SSOConfig) *domain.SSOConfig {
	out := *c
	out.ClientSecret = ""
	out.Scopes = nonNil(out.Scopes)
	out.AllowedDomains = nonNil(out.AllowedDomains)
	return &out
}
