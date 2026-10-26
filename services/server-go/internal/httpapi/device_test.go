package httpapi

import (
	"bytes"
	"context"
	"encoding/base32"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// deviceStack is the real IAM service (memory store, manual clock) behind
// the HTTP API, for the device flow and refresh routes (§16.6).
type deviceStack struct {
	t     *testing.T
	store *meta.Memory
	sec   *iam.Service
	audit *fakeAudit
	h     http.Handler

	mu  sync.Mutex
	now time.Time
}

const devicePassword = "correct horse battery"

func newDeviceStack(t *testing.T, mutate ...func(*iam.Options)) *deviceStack {
	t.Helper()
	s := &deviceStack{t: t, store: meta.NewMemory(), audit: &fakeAudit{}, now: time.Date(2026, 3, 4, 5, 6, 7, 0, time.UTC)}
	opts := iam.Options{
		AllowRegistration: true,
		DisableRateLimit:  true,
		Password:          iam.PasswordParams{Memory: 64, Time: 1, Threads: 1},
		Clock:             s.clock,
	}
	for _, m := range mutate {
		m(&opts)
	}
	s.sec = iam.New(s.store, opts)
	az := authz.New(s.store, authz.Options{})
	s.h = New(Deps{Store: s.store, Auth: s.sec, Authz: az, Access: az, Security: s.sec, Audit: s.audit})
	for _, u := range []string{"alice", "bob"} {
		if _, err := s.sec.Register(context.Background(), u, u+"@example.com", devicePassword); err != nil {
			t.Fatal(err)
		}
	}
	return s
}

func (s *deviceStack) clock() time.Time {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.now
}

func (s *deviceStack) advance(d time.Duration) {
	s.mu.Lock()
	s.now = s.now.Add(d)
	s.mu.Unlock()
}

func (s *deviceStack) do(method, path, token string, body any) *httptest.ResponseRecorder {
	s.t.Helper()
	var rd *bytes.Reader
	if body == nil {
		rd = bytes.NewReader(nil)
	} else {
		raw, err := json.Marshal(body)
		must(s.t, err)
		rd = bytes.NewReader(raw)
	}
	req := httptest.NewRequest(method, path, rd)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	req.Header.Set("User-Agent", "device-test/1")
	if token != "" {
		req.Header.Set("Authorization", "Bearer "+token)
	}
	rec := httptest.NewRecorder()
	s.h.ServeHTTP(rec, req)
	return rec
}

func (s *deviceStack) expect(rec *httptest.ResponseRecorder, status int, code string) {
	s.t.Helper()
	if rec.Code != status || (code != "" && errCode(rec) != code) {
		s.t.Fatalf("status %d code %q, want %d %q: %s", rec.Code, errCode(rec), status, code, rec.Body.String())
	}
}

func (s *deviceStack) login(user string) sessionResponse {
	s.t.Helper()
	rec := s.do("POST", "/api/v1/auth/login", "", map[string]string{"username": user, "password": devicePassword})
	s.expect(rec, 200, "")
	return decode[sessionResponse](s.t, rec)
}

func TestLoginReturnsRefreshToken(t *testing.T) {
	s := newDeviceStack(t)
	r := s.login("alice")
	if r.Token == "" || !strings.HasPrefix(r.RefreshToken, iam.RefreshTokenPrefix) || r.TokenType != "Bearer" ||
		r.ExpiresIn != 3600 || r.RefreshExpiresIn != 30*24*3600 || r.Account == nil || r.Account.Username != "alice" {
		t.Fatalf("login = %+v", r)
	}
	s.expect(s.do("GET", "/api/v1/auth/me", r.Token, nil), 200, "")

	// Registration returns a pair too.
	rec := s.do("POST", "/api/v1/auth/register", "", map[string]string{"username": "carol", "email": "carol@example.com", "password": devicePassword})
	s.expect(rec, 201, "")
	if reg := decode[sessionResponse](t, rec); reg.RefreshToken == "" || reg.Token == "" {
		t.Fatalf("register = %s", rec.Body.String())
	}
}

func TestRefreshRoute(t *testing.T) {
	s := newDeviceStack(t)
	first := s.login("alice")
	const path = "/api/v1/auth/refresh"
	s.expect(s.do("POST", path, "", map[string]string{}), 400, "bad_request")
	s.expect(s.do("POST", path, "", map[string]string{"refresh_token": "nope"}), 401, "invalid_grant")

	s.advance(2 * time.Hour) // the access token expired
	s.expect(s.do("GET", "/api/v1/auth/me", first.Token, nil), 401, "unauthenticated")
	rec := s.do("POST", path, "", map[string]string{"refresh_token": first.RefreshToken})
	s.expect(rec, 200, "")
	if rec.Header().Get("Cache-Control") != "no-store" {
		t.Fatalf("Cache-Control = %q", rec.Header().Get("Cache-Control"))
	}
	second := decode[tokenPairResponse](t, rec)
	if second.Token == "" || second.RefreshToken == first.RefreshToken || second.TokenType != "Bearer" || second.ExpiresIn != 3600 ||
		second.Account == nil || second.Account.Username != "alice" {
		t.Fatalf("refresh = %s", rec.Body.String())
	}
	s.expect(s.do("GET", "/api/v1/auth/me", second.Token, nil), 200, "")

	rec = s.do("POST", path, "", map[string]string{"refresh_token": second.RefreshToken})
	s.expect(rec, 200, "")
	third := decode[tokenPairResponse](t, rec)

	// Reuse of the first token revokes the login.
	s.expect(s.do("POST", path, "", map[string]string{"refresh_token": first.RefreshToken}), 401, "invalid_grant")
	if !slices.Contains(s.audit.actions(), "auth.refresh_reuse") {
		t.Fatalf("audit = %v", s.audit.actions())
	}
	s.expect(s.do("GET", "/api/v1/auth/me", third.Token, nil), 401, "unauthenticated")
	s.expect(s.do("POST", path, "", map[string]string{"refresh_token": third.RefreshToken}), 401, "invalid_grant")

	// Logging out ends the refresh token.
	r := s.login("bob")
	s.expect(s.do("POST", "/api/v1/auth/logout", r.Token, nil), 204, "")
	s.expect(s.do("POST", path, "", map[string]string{"refresh_token": r.RefreshToken}), 401, "invalid_grant")
}

func TestDeviceRoutesApprove(t *testing.T) {
	s := newDeviceStack(t)
	alice := s.login("alice")

	rec := s.do("POST", "/api/v1/auth/device", "", map[string]any{"client_name": "wt on laptop", "client_id": "wt-cli", "scope": "tree:read mr:*"})
	s.expect(rec, 200, "")
	da := decode[deviceAuthorizationResponse](t, rec)
	if !strings.HasPrefix(da.DeviceCode, iam.DeviceCodePrefix) || len(da.UserCode) != 9 || da.ExpiresIn != 600 || da.Interval != 5 ||
		da.VerificationURI != "http://example.com/admin/device" ||
		da.VerificationURIComplete != "http://example.com/admin/device?code="+da.UserCode {
		t.Fatalf("device authorization = %s", rec.Body.String())
	}

	const token = "/api/v1/auth/device/token"
	poll := func() *httptest.ResponseRecorder {
		return s.do("POST", token, "", map[string]string{"device_code": da.DeviceCode, "grant_type": deviceGrantType})
	}
	s.expect(poll(), 400, "authorization_pending")
	s.expect(poll(), 400, "slow_down")
	s.expect(s.do("POST", token, "", map[string]string{"device_code": da.DeviceCode, "grant_type": "password"}), 400, "unsupported_grant_type")
	s.expect(s.do("POST", token, "", map[string]string{"device_code": "wtd_nope"}), 400, "invalid_grant")
	s.expect(s.do("POST", token, "", map[string]string{}), 400, "bad_request")

	code := da.UserCode
	lower := strings.ToLower(code)
	s.expect(s.do("GET", "/api/v1/auth/device/"+code, "", nil), 401, "unauthenticated")
	s.expect(s.do("GET", "/api/v1/auth/device/BBBB-BBBB", alice.Token, nil), 404, "not_found")
	rec = s.do("GET", "/api/v1/auth/device/"+lower, alice.Token, nil)
	s.expect(rec, 200, "")
	view := decode[iam.DeviceRequest](t, rec)
	if view.UserCode != code || view.ClientName != "wt on laptop" || view.ClientID != "wt-cli" || view.Status != "pending" ||
		strings.Join(view.Scopes, ",") != "tree:read,mr:*" || view.MFARequired || view.UserAgent != "device-test/1" {
		t.Fatalf("device request = %s", rec.Body.String())
	}

	// Scoped tokens cannot approve devices.
	scoped, _, err := s.sec.CreateScopedToken(context.Background(), alice.Account.ID, "ro", time.Hour, []string{"tree:read"})
	must(t, err)
	s.expect(s.do("POST", "/api/v1/auth/device/"+code+"/approve", scoped, nil), 403, "forbidden")

	rec = s.do("POST", "/api/v1/auth/device/"+code+"/approve", alice.Token, nil)
	s.expect(rec, 200, "")
	if decode[iam.DeviceRequest](t, rec).Status != "approved" || !slices.Contains(s.audit.actions(), "auth.device_approve") {
		t.Fatalf("approve = %s / %v", rec.Body.String(), s.audit.actions())
	}
	s.expect(s.do("POST", "/api/v1/auth/device/"+code+"/deny", alice.Token, nil), 409, "conflict")

	s.advance(20 * time.Second)
	rec = poll()
	s.expect(rec, 200, "")
	pair := decode[tokenPairResponse](t, rec)
	if pair.Token == "" || pair.RefreshToken == "" || pair.Account == nil || pair.Account.Username != "alice" {
		t.Fatalf("device tokens = %s", rec.Body.String())
	}
	s.expect(s.do("GET", "/api/v1/auth/me", pair.Token, nil), 200, "")
	// The device session is listed, with its client.
	rec = s.do("GET", "/api/v1/auth/sessions", alice.Token, nil)
	s.expect(rec, 200, "")
	if !strings.Contains(rec.Body.String(), `"name":"device: wt on laptop"`) {
		t.Fatalf("sessions = %s", rec.Body.String())
	}
	// Scoped like requested: no write access.
	if p, err := s.sec.Authenticate(context.Background(), pair.Token); err != nil || strings.Join(p.Scopes, ",") != "tree:read,mr:*" {
		t.Fatalf("device principal = %+v, %v", p, err)
	}
	s.advance(20 * time.Second)
	s.expect(poll(), 400, "invalid_grant")
}

func TestDeviceRoutesDenyAndMFA(t *testing.T) {
	s := newDeviceStack(t)
	bob := s.login("bob")
	rec := s.do("POST", "/api/v1/auth/device", "", nil) // empty body
	s.expect(rec, 200, "")
	da := decode[deviceAuthorizationResponse](t, rec)
	s.expect(s.do("POST", "/api/v1/auth/device/"+da.UserCode+"/deny", bob.Token, nil), 200, "")
	s.expect(s.do("POST", "/api/v1/auth/device/token", "", map[string]string{"device_code": da.DeviceCode}), 400, "access_denied")
	if !slices.Contains(s.audit.actions(), "auth.device_deny") {
		t.Fatalf("audit = %v", s.audit.actions())
	}

	// With MFA enabled a session without a second factor needs a code.
	ctx := context.Background()
	acct, err := s.store.GetAccountByUsername(ctx, "bob")
	must(t, err)
	secret, _, err := s.sec.BeginTOTPEnroll(ctx, acct.ID)
	must(t, err)
	raw := decodeBase32(t, secret)
	_, err = s.sec.ConfirmTOTP(ctx, acct.ID, iam.TOTPCode(raw, s.clock()))
	must(t, err)
	s.advance(iam.TOTPPeriod)

	rec = s.do("POST", "/api/v1/auth/device", "", map[string]string{})
	s.expect(rec, 200, "")
	da = decode[deviceAuthorizationResponse](t, rec)
	rec = s.do("GET", "/api/v1/auth/device/"+da.UserCode, bob.Token, nil)
	s.expect(rec, 200, "")
	if !decode[iam.DeviceRequest](t, rec).MFARequired {
		t.Fatalf("mfa_required: %s", rec.Body.String())
	}
	s.expect(s.do("POST", "/api/v1/auth/device/"+da.UserCode+"/approve", bob.Token, map[string]string{}), 403, "forbidden")
	s.expect(s.do("POST", "/api/v1/auth/device/"+da.UserCode+"/approve", bob.Token,
		map[string]string{"code": iam.TOTPCode(raw, s.clock())}), 200, "")
	rec = s.do("POST", "/api/v1/auth/device/token", "", map[string]string{"device_code": da.DeviceCode})
	s.expect(rec, 200, "")
	p, err := s.sec.Authenticate(ctx, decode[tokenPairResponse](t, rec).Token)
	must(t, err)
	if !p.MFA {
		t.Fatal("the device session records the second factor")
	}
}

func TestDeviceRoutesRateLimit(t *testing.T) {
	s := newDeviceStack(t)
	var rec *httptest.ResponseRecorder
	for i := 0; i < 31; i++ {
		rec = s.do("POST", "/api/v1/auth/device", "", nil)
	}
	s.expect(rec, 429, "rate_limited")
}

func TestDeviceRoutesNotImplemented(t *testing.T) {
	f := newFixture(t)
	for _, c := range []struct{ method, path string }{
		{"POST", "/api/v1/auth/device"},
		{"POST", "/api/v1/auth/refresh"},
	} {
		rec := f.do(t, c.method, c.path, "", map[string]string{"refresh_token": "x"})
		if rec.Code != http.StatusNotImplemented {
			t.Errorf("%s %s = %d: %s", c.method, c.path, rec.Code, rec.Body.String())
		}
	}
}

// decodeBase32 decodes a TOTP secret as shown to users.
func decodeBase32(t *testing.T, secret string) []byte {
	t.Helper()
	s := strings.TrimRight(strings.ToUpper(strings.ReplaceAll(secret, " ", "")), "=")
	raw, err := base32.StdEncoding.WithPadding(base32.NoPadding).DecodeString(s)
	must(t, err)
	return raw
}
