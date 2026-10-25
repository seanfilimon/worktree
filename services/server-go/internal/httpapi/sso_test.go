package httpapi

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/http/httptest"
	"net/url"
	"strings"
	"sync"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/sso"
)

// fakeSSO is a scriptable domain.SSOProvider.
type fakeSSO struct {
	mu sync.Mutex

	cfg       domain.SSOConfig
	secret    string
	configErr error
	saveErr   error
	saved     *domain.SSOConfig
	savedSec  **string

	authErr      error
	authRedirect string // redirectAfter passed to AuthURL

	cbErr      error
	cbToken    string
	cbAcct     *domain.Account
	cbRedirect string
	cbCode     string
	cbState    string
	cbMeta     domain.SessionMeta
	cbCalls    int
}

func (f *fakeSSO) Config(context.Context) (*domain.SSOConfig, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	if f.configErr != nil {
		return nil, f.configErr
	}
	c := f.cfg
	c.HasSecret = f.secret != ""
	return &c, nil
}

func (f *fakeSSO) SaveConfig(_ context.Context, c *domain.SSOConfig, secret *string) (*domain.SSOConfig, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	cp := *c
	f.saved = &cp
	f.savedSec = &secret
	if f.saveErr != nil {
		return nil, f.saveErr
	}
	if secret != nil {
		f.secret = *secret
	}
	f.cfg = cp
	out := cp
	out.HasSecret = f.secret != ""
	return &out, nil
}

func (f *fakeSSO) AuthURL(_ context.Context, redirectAfter string) (string, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.authRedirect = redirectAfter
	if f.authErr != nil {
		return "", f.authErr
	}
	return "https://idp.example/authorize?client_id=c&state=state-123&nonce=n", nil
}

func (f *fakeSSO) Callback(_ context.Context, code, state string, meta domain.SessionMeta) (string, *domain.Account, string, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.cbCalls++
	f.cbCode, f.cbState, f.cbMeta = code, state, meta
	if f.cbErr != nil {
		return "", nil, "", f.cbErr
	}
	return f.cbToken, f.cbAcct, f.cbRedirect, nil
}

func newSSOFixture(t *testing.T, s *fakeSSO) *fixture {
	t.Helper()
	return newFixture(t, func(d *Deps) {
		if s != nil {
			d.SSO = s
		}
		d.AllowRegistration = true
	})
}

func newEnabledSSO() *fakeSSO {
	return &fakeSSO{
		cfg: domain.SSOConfig{Enabled: true, Name: "Okta", Issuer: "https://idp.example", ClientID: "c",
			RedirectURL: "https://wt.example/api/v1/auth/sso/callback", Scopes: []string{"openid"}},
		secret:     "shh",
		cbToken:    "tok/en+1",
		cbAcct:     &domain.Account{ID: "acct-alice", Username: "alice", TenantID: "tenant-alice"},
		cbRedirect: "/admin/trees",
	}
}

// fragment returns the decoded value of key in the Location fragment.
func fragment(t *testing.T, rec *httptest.ResponseRecorder, key string) (path string, value string) {
	t.Helper()
	loc := rec.Header().Get("Location")
	u, err := url.Parse(loc)
	if err != nil {
		t.Fatalf("bad Location %q", loc)
	}
	vals, err := url.ParseQuery(u.EscapedFragment())
	if err != nil {
		t.Fatalf("bad fragment in %q", loc)
	}
	return u.Path, vals.Get(key)
}

func TestAuthProviders(t *testing.T) {
	t.Run("without sso", func(t *testing.T) {
		f := newSSOFixture(t, nil)
		rec := f.do(t, "GET", "/api/v1/auth/providers", "", nil)
		if rec.Code != 200 {
			t.Fatalf("status %d", rec.Code)
		}
		got := decode[authProvidersResponse](t, rec)
		if !got.Password || !got.Registration || got.SSO.Enabled || got.SSO.Name != "" {
			t.Fatalf("%+v", got)
		}
	})
	t.Run("enabled", func(t *testing.T) {
		f := newSSOFixture(t, newEnabledSSO())
		got := decode[authProvidersResponse](t, f.do(t, "GET", "/api/v1/auth/providers", "", nil))
		if !got.SSO.Enabled || got.SSO.Name != "Okta" {
			t.Fatalf("%+v", got)
		}
		if !strings.Contains(f.do(t, "GET", "/api/v1/auth/providers", "", nil).Body.String(), `"sso":{"enabled":true,"name":"Okta"}`) {
			t.Fatal("unexpected JSON shape")
		}
	})
	t.Run("config disabled or broken", func(t *testing.T) {
		s := newEnabledSSO()
		s.cfg.Enabled = false
		f := newSSOFixture(t, s)
		if decode[authProvidersResponse](t, f.do(t, "GET", "/api/v1/auth/providers", "", nil)).SSO.Enabled {
			t.Fatal("disabled config reported enabled")
		}
		s.configErr = errors.New("store down")
		rec := f.do(t, "GET", "/api/v1/auth/providers", "", nil)
		if rec.Code != 200 || decode[authProvidersResponse](t, rec).SSO.Enabled {
			t.Fatalf("broken config: %d %s", rec.Code, rec.Body)
		}
	})
	t.Run("registration off", func(t *testing.T) {
		f := newFixture(t, func(d *Deps) { d.AllowRegistration = false })
		if decode[authProvidersResponse](t, f.do(t, "GET", "/api/v1/auth/providers", "", nil)).Registration {
			t.Fatal("registration reported on")
		}
	})
}

func TestSSOStart(t *testing.T) {
	t.Run("redirects to the IdP and binds the state", func(t *testing.T) {
		s := newEnabledSSO()
		f := newSSOFixture(t, s)
		rec := f.do(t, "GET", "/api/v1/auth/sso/start?redirect=%2Fadmin%2Ftrees%3Fa%3D1", "", nil)
		if rec.Code != http.StatusFound || !strings.HasPrefix(rec.Header().Get("Location"), "https://idp.example/authorize?") {
			t.Fatalf("status %d, Location %q", rec.Code, rec.Header().Get("Location"))
		}
		if s.authRedirect != "/admin/trees?a=1" {
			t.Fatalf("redirect passed = %q", s.authRedirect)
		}
		c := findCookie(rec, ssoStateCookie)
		if c == nil || c.Value != "state-123" || !c.HttpOnly || c.SameSite != http.SameSiteLaxMode || c.Path != ssoStateCookiePath || c.MaxAge <= 0 {
			t.Fatalf("state cookie = %+v", c)
		}
		if rec.Header().Get("Cache-Control") != "no-store" {
			t.Fatal("start response is cacheable")
		}
	})
	t.Run("default redirect", func(t *testing.T) {
		s := newEnabledSSO()
		f := newSSOFixture(t, s)
		f.do(t, "GET", "/api/v1/auth/sso/start", "", nil)
		if s.authRedirect != "/admin/" {
			t.Fatalf("redirect = %q", s.authRedirect)
		}
	})
	t.Run("secure cookie behind TLS proxy", func(t *testing.T) {
		f := newSSOFixture(t, newEnabledSSO())
		req := httptest.NewRequest("GET", "/api/v1/auth/sso/start", nil)
		req.Header.Set("X-Forwarded-Proto", "https")
		rec := httptest.NewRecorder()
		f.h.ServeHTTP(rec, req)
		if c := findCookie(rec, ssoStateCookie); c == nil || !c.Secure {
			t.Fatalf("cookie = %+v", c)
		}
	})
	for _, bad := range []string{"https://evil.example/", "//evil.example/admin/", "/api/v1/auth/me", "/admin/../api"} {
		t.Run("rejects redirect "+bad, func(t *testing.T) {
			s := newEnabledSSO()
			f := newSSOFixture(t, s)
			rec := f.do(t, "GET", "/api/v1/auth/sso/start?redirect="+url.QueryEscape(bad), "", nil)
			path, msg := fragment(t, rec, "sso_error")
			if rec.Code != http.StatusFound || path != ssoLoginPage || !strings.Contains(msg, "redirect") {
				t.Fatalf("status %d Location %q", rec.Code, rec.Header().Get("Location"))
			}
			if s.authRedirect != "" {
				t.Fatal("AuthURL called with an unsafe redirect")
			}
		})
	}
	t.Run("not configured", func(t *testing.T) {
		f := newSSOFixture(t, nil)
		rec := f.do(t, "GET", "/api/v1/auth/sso/start", "", nil)
		if _, msg := fragment(t, rec, "sso_error"); rec.Code != http.StatusFound || !strings.Contains(msg, "not enabled") {
			t.Fatalf("%d %q", rec.Code, rec.Header().Get("Location"))
		}
	})
	t.Run("provider error", func(t *testing.T) {
		s := newEnabledSSO()
		s.authErr = sso.ErrNotEnabled
		f := newSSOFixture(t, s)
		rec := f.do(t, "GET", "/api/v1/auth/sso/start", "", nil)
		if _, msg := fragment(t, rec, "sso_error"); msg != "single sign-on is not enabled" {
			t.Fatalf("message %q", msg)
		}
		if findCookie(rec, ssoStateCookie) != nil {
			t.Fatal("state cookie set on failure")
		}
	})
	t.Run("internal errors are not leaked", func(t *testing.T) {
		s := newEnabledSSO()
		s.authErr = errors.New("dial tcp 10.0.0.1:443: secret internal detail")
		f := newSSOFixture(t, s)
		rec := f.do(t, "GET", "/api/v1/auth/sso/start", "", nil)
		if _, msg := fragment(t, rec, "sso_error"); msg != "internal server error" {
			t.Fatalf("message %q", msg)
		}
	})
}

func findCookie(rec *httptest.ResponseRecorder, name string) *http.Cookie {
	for _, c := range rec.Result().Cookies() {
		if c.Name == name {
			return c
		}
	}
	return nil
}

// callback performs GET /auth/sso/callback with the binding cookie.
func callback(t *testing.T, f *fixture, query, cookie string) *httptest.ResponseRecorder {
	t.Helper()
	req := httptest.NewRequest("GET", "/api/v1/auth/sso/callback?"+query, nil)
	req.Header.Set("User-Agent", "browser/1.0")
	if cookie != "" {
		req.AddCookie(&http.Cookie{Name: ssoStateCookie, Value: cookie})
	}
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	return rec
}

func TestSSOCallback(t *testing.T) {
	t.Run("success", func(t *testing.T) {
		s := newEnabledSSO()
		f := newSSOFixture(t, s)
		rec := callback(t, f, "code=abc&state=state-123", "state-123")
		if rec.Code != http.StatusFound {
			t.Fatalf("status %d", rec.Code)
		}
		path, tok := fragment(t, rec, "sso_token")
		if path != "/admin/trees" || tok != "tok/en+1" {
			t.Fatalf("Location %q", rec.Header().Get("Location"))
		}
		if s.cbCode != "abc" || s.cbState != "state-123" || s.cbMeta.UserAgent != "browser/1.0" || s.cbMeta.RemoteIP == "" {
			t.Fatalf("callback args: %q %q %+v", s.cbCode, s.cbState, s.cbMeta)
		}
		if c := findCookie(rec, ssoStateCookie); c == nil || c.MaxAge >= 0 {
			t.Fatalf("binding cookie not cleared: %+v", c)
		}
		if rec.Header().Get("Referrer-Policy") != "no-referrer" || rec.Header().Get("Cache-Control") != "no-store" {
			t.Fatal("missing no-store / no-referrer headers")
		}
		hasAudit(t, f, "sso.login")
	})
	t.Run("unsafe redirect from provider falls back", func(t *testing.T) {
		s := newEnabledSSO()
		s.cbRedirect = "//evil.example/"
		f := newSSOFixture(t, s)
		rec := callback(t, f, "code=abc&state=state-123", "state-123")
		if path, tok := fragment(t, rec, "sso_token"); path != "/admin/" || tok == "" || strings.Contains(rec.Header().Get("Location"), "evil") {
			t.Fatalf("Location %q", rec.Header().Get("Location"))
		}
	})
	for _, tc := range []struct{ name, query, cookie string }{
		{"missing cookie", "code=abc&state=state-123", ""},
		{"cookie mismatch", "code=abc&state=state-123", "other"},
		{"missing state", "code=abc", "state-123"},
	} {
		t.Run(tc.name, func(t *testing.T) {
			s := newEnabledSSO()
			f := newSSOFixture(t, s)
			rec := callback(t, f, tc.query, tc.cookie)
			path, msg := fragment(t, rec, "sso_error")
			if rec.Code != http.StatusFound || path != ssoLoginPage || !strings.Contains(msg, "not started from this browser") {
				t.Fatalf("%d %q", rec.Code, rec.Header().Get("Location"))
			}
			if s.cbCalls != 0 {
				t.Fatal("provider callback ran without a matching binding cookie")
			}
		})
	}
	t.Run("provider error parameter", func(t *testing.T) {
		s := newEnabledSSO()
		f := newSSOFixture(t, s)
		rec := callback(t, f, "error=access_denied&state=state-123", "state-123")
		if _, msg := fragment(t, rec, "sso_error"); !strings.Contains(msg, "access_denied") {
			t.Fatalf("message %q", msg)
		}
		rec = callback(t, f, "error="+url.QueryEscape("<script>")+"&state=state-123", "state-123")
		if _, msg := fragment(t, rec, "sso_error"); strings.Contains(msg, "<") {
			t.Fatalf("unsanitized message %q", msg)
		}
		if s.cbCalls != 0 {
			t.Fatal("callback ran for an error response")
		}
		hasAudit(t, f, "sso.login_failed")
	})
	t.Run("login refused", func(t *testing.T) {
		s := newEnabledSSO()
		s.cbErr = &sso.Error{Msg: "this account is disabled", Kind: domain.ErrForbidden}
		f := newSSOFixture(t, s)
		rec := callback(t, f, "code=abc&state=state-123", "state-123")
		path, msg := fragment(t, rec, "sso_error")
		if path != ssoLoginPage || msg != "this account is disabled" {
			t.Fatalf("Location %q", rec.Header().Get("Location"))
		}
		if strings.Contains(rec.Header().Get("Location"), "sso_token") {
			t.Fatal("token issued on failure")
		}
		hasAudit(t, f, "sso.login_failed")
	})
	t.Run("internal failure", func(t *testing.T) {
		s := newEnabledSSO()
		s.cbErr = fmt.Errorf("sso: issuing session: %w", errors.New("bolt: database locked"))
		f := newSSOFixture(t, s)
		rec := callback(t, f, "code=abc&state=state-123", "state-123")
		if _, msg := fragment(t, rec, "sso_error"); msg != "internal server error" {
			t.Fatalf("message %q", msg)
		}
	})
	t.Run("not configured", func(t *testing.T) {
		f := newSSOFixture(t, nil)
		rec := callback(t, f, "code=abc&state=s", "s")
		if _, msg := fragment(t, rec, "sso_error"); !strings.Contains(msg, "not enabled") {
			t.Fatalf("message %q", msg)
		}
	})
}

func TestAdminSSOConfig(t *testing.T) {
	runCases(t, []endpointCase{
		{name: "get anonymous", method: "GET", path: "/api/v1/admin/sso", status: 401, code: codeUnauthenticated},
		{name: "get non-admin", method: "GET", path: "/api/v1/admin/sso", as: "alice", status: 403, code: codeForbidden},
		{name: "put non-admin", method: "PUT", path: "/api/v1/admin/sso", as: "alice", body: map[string]any{"enabled": false}, status: 403, code: codeForbidden},
		{name: "get without provider", method: "GET", path: "/api/v1/admin/sso", as: "admin", status: 404, code: codeNotFound},
	})

	t.Run("get", func(t *testing.T) {
		f := newSSOFixture(t, newEnabledSSO())
		rec := f.do(t, "GET", "/api/v1/admin/sso", "admin", nil)
		if rec.Code != 200 {
			t.Fatalf("status %d: %s", rec.Code, rec.Body)
		}
		body := rec.Body.String()
		if strings.Contains(body, "shh") || strings.Contains(body, `"client_secret"`) {
			t.Fatalf("secret leaked: %s", body)
		}
		got := decode[map[string]any](t, rec)
		if got["has_client_secret"] != true || got["issuer"] != "https://idp.example" || got["allowed_domains"] == nil {
			t.Fatalf("%v", got)
		}
	})

	t.Run("put keeps secret when omitted", func(t *testing.T) {
		s := newEnabledSSO()
		f := newSSOFixture(t, s)
		// Round-trip the GET body (includes has_client_secret / updated_at).
		cur := f.do(t, "GET", "/api/v1/admin/sso", "admin", nil).Body.String()
		rec := f.do(t, "PUT", "/api/v1/admin/sso", "admin", cur)
		if rec.Code != 200 {
			t.Fatalf("status %d: %s", rec.Code, rec.Body)
		}
		if *s.savedSec != nil || s.secret != "shh" {
			t.Fatal("omitted client_secret changed the secret")
		}
		hasAudit(t, f, "sso.config.update")
	})

	t.Run("put sets secret", func(t *testing.T) {
		s := newEnabledSSO()
		f := newSSOFixture(t, s)
		rec := f.do(t, "PUT", "/api/v1/admin/sso", "admin", map[string]any{
			"enabled": true, "name": "Corp", "issuer": "https://idp2.example", "client_id": "c2",
			"client_secret": "new-secret", "redirect_url": "https://wt.example/cb", "scopes": []string{"openid"},
			"allowed_domains": []string{"corp.example"}, "auto_create": true, "username_claim": "email",
		})
		if rec.Code != 200 {
			t.Fatalf("status %d: %s", rec.Code, rec.Body)
		}
		if s.secret != "new-secret" || s.saved.Issuer != "https://idp2.example" || !s.saved.AutoCreate ||
			s.saved.UsernameClaim != "email" || s.saved.AllowedDomains[0] != "corp.example" {
			t.Fatalf("saved = %+v secret %q", s.saved, s.secret)
		}
		if strings.Contains(rec.Body.String(), "new-secret") {
			t.Fatal("secret echoed in response")
		}
		var ev *domain.AuditEvent
		for i := range f.audit.events {
			if f.audit.events[i].Action == "sso.config.update" {
				ev = &f.audit.events[i]
			}
		}
		if ev == nil || ev.Details["secret_changed"] != "true" || ev.Actor != "admin" {
			t.Fatalf("audit = %+v", ev)
		}
	})

	t.Run("put validation error", func(t *testing.T) {
		s := newEnabledSSO()
		s.saveErr = &sso.Error{Msg: "scopes must include \"openid\"", Kind: domain.ErrInvalid}
		f := newSSOFixture(t, s)
		rec := f.do(t, "PUT", "/api/v1/admin/sso", "admin", map[string]any{"enabled": true, "scopes": []string{"email"}})
		if rec.Code != 400 || errCode(rec) != codeBadRequest || !strings.Contains(rec.Body.String(), "openid") {
			t.Fatalf("%d %s", rec.Code, rec.Body)
		}
	})

	t.Run("put unknown field", func(t *testing.T) {
		f := newSSOFixture(t, newEnabledSSO())
		rec := f.do(t, "PUT", "/api/v1/admin/sso", "admin", map[string]any{"bogus": 1})
		if rec.Code != 400 {
			t.Fatalf("%d %s", rec.Code, rec.Body)
		}
	})
}
