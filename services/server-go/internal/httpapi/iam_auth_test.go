package httpapi

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http/httptest"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestIAMLogin(t *testing.T) {
	const login = "/api/v1/auth/login"
	runIAMCases(t, []endpointCase{
		{name: "password only", method: "POST", path: login, body: map[string]string{"username": "bob", "password": "bob-password"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if keys := strings.Join(jsonKeys(t, rec), ","); keys != "account,token" {
					t.Fatalf("keys = %s", keys)
				}
				if m := f.iamSec().meta(); m.RemoteIP != "192.0.2.1" {
					t.Fatalf("session meta = %+v", m)
				}
				hasAudit(t, f, "auth.login")
			}},
		{name: "mfa required", method: "POST", path: login, body: map[string]string{"username": "bob", "password": "bob-password"}, status: 200,
			setup: func(t *testing.T, f *fixture) { f.iamSec().mfaUsers["bob"] = true },
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				var r struct {
					MFARequired bool              `json:"mfa_required"`
					MFAToken    string            `json:"mfa_token"`
					Account     map[string]string `json:"account"`
					Token       *string           `json:"token"`
				}
				must(t, json.Unmarshal(rec.Body.Bytes(), &r))
				if !r.MFARequired || r.MFAToken != "mfa-bob" || r.Token != nil || len(r.Account) != 1 || r.Account["username"] != "bob" {
					t.Fatalf("login = %s", rec.Body.String())
				}
				hasAudit(t, f, "auth.mfa_challenge")
				if slices.Contains(f.audit.actions(), "auth.login") {
					t.Fatalf("challenge audited as login")
				}
			}},
		{name: "wrong password", method: "POST", path: login, body: map[string]string{"username": "bob", "password": "nope"}, status: 401, code: "unauthenticated",
			check: audited("auth.login_failed")},
		{name: "mfa step", method: "POST", path: login + "/mfa", body: map[string]string{"mfa_token": "mfa-bob", "code": "123 456"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[sessionResponse](t, rec)
				if r.Token == "" || r.Account.Username != "bob" {
					t.Fatalf("session = %s", rec.Body.String())
				}
				if e := auditDetail(t, f, "auth.login"); e.Details["mfa"] != "true" || e.Actor != "bob" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "mfa step bad code", method: "POST", path: login + "/mfa", body: map[string]string{"mfa_token": "mfa-bob", "code": "000000"}, status: 401,
			check: audited("auth.login_failed")},
		{name: "mfa step missing code", method: "POST", path: login + "/mfa", body: map[string]string{"mfa_token": "mfa-bob"}, status: 400},
	})
}

func TestIAMLoginUserAgent(t *testing.T) {
	f := newIAMFixture(t)
	req := httptest.NewRequest("POST", "/api/v1/auth/login", strings.NewReader(`{"username":"bob","password":"bob-password"}`))
	req.Header.Set("User-Agent", "wt/0.4")
	req.RemoteAddr = "203.0.113.9:5555"
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	if rec.Code != 200 {
		t.Fatalf("status %d: %s", rec.Code, rec.Body.String())
	}
	if m := f.iamSec().meta(); m.UserAgent != "wt/0.4" || m.RemoteIP != "203.0.113.9" {
		t.Fatalf("meta = %+v", m)
	}
}

func TestIAMLoginMFAWithoutSecurity(t *testing.T) {
	runIAMCases(t, []endpointCase{
		{name: "login falls back to Authenticator", method: "POST", path: "/api/v1/auth/login", body: map[string]string{"username": "bob", "password": "bob-password"}, status: 200},
		{name: "mfa step 501", method: "POST", path: "/api/v1/auth/login/mfa", body: map[string]string{"mfa_token": "x", "code": "1"}, status: 501, code: "not_implemented"},
	}, noSecurity)
}

func TestIAMScopedTokens(t *testing.T) {
	const tokens = "/api/v1/auth/tokens"
	runIAMCases(t, []endpointCase{
		{name: "scoped", method: "POST", path: tokens, as: "bob", body: map[string]any{"name": "ci", "ttl_hours": 1, "scopes": []string{"tree:read", "mr:*", "tree:read"}}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[createTokenResponse](t, rec)
				if r.Token == "" || !slices.Equal(r.APIToken.Scopes, []string{"tree:read", "mr:*"}) {
					t.Fatalf("token = %s", rec.Body.String())
				}
				if s := f.iamSec().lastScopes; !slices.Equal(s, []string{"tree:read", "mr:*"}) {
					t.Fatalf("CreateScopedToken scopes = %v", s)
				}
				if e := auditDetail(t, f, "token.create"); e.Details["scopes"] != "tree:read,mr:*" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "unscoped uses Authenticator", method: "POST", path: tokens, as: "bob", body: map[string]any{"name": "full"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if f.iamSec().lastScopes != nil {
					t.Fatalf("CreateScopedToken called")
				}
			}},
		{name: "star scope", method: "POST", path: tokens, as: "bob", body: map[string]any{"name": "x", "scopes": []string{"*"}}, status: 200},
		{name: "unknown scope", method: "POST", path: tokens, as: "bob", body: map[string]any{"name": "x", "scopes": []string{"tree:fly"}}, status: 400},
		{name: "scoped caller within scopes", method: "POST", path: tokens, as: "scoped", body: map[string]any{"name": "x", "scopes": []string{"tree:read"}}, status: 200,
			setup: func(t *testing.T, f *fixture) { addScopedToken(f, "bob", "scoped", "tree:*") }},
		{name: "scoped caller broader scope", method: "POST", path: tokens, as: "scoped", body: map[string]any{"name": "x", "scopes": []string{"mr:create"}}, status: 403,
			setup: func(t *testing.T, f *fixture) { addScopedToken(f, "bob", "scoped", "tree:*") }},
		{name: "scoped caller unscoped token", method: "POST", path: tokens, as: "scoped", body: map[string]any{"name": "x"}, status: 403,
			setup: func(t *testing.T, f *fixture) { addScopedToken(f, "bob", "scoped", "tree:*") }},
		{name: "service account", method: "POST", path: tokens, as: "sa-token", body: map[string]any{"name": "x"}, status: 403,
			setup: func(t *testing.T, f *fixture) { addServiceAccountToken(f, "sa-token") }},
	})
}

func TestIAMSessions(t *testing.T) {
	seed := func(t *testing.T, f *fixture) {
		ctx := context.Background()
		must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "s-cur", AccountID: "acct-bob", Name: "session", Kind: domain.TokenSession,
			TokenHash: hashToken("bob"), CreatedAt: t0, LastIP: "10.0.0.1", UserAgent: "wt"}))
		must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "s-old", AccountID: "acct-bob", Name: "session", Kind: domain.TokenSession,
			TokenHash: hashToken("other"), CreatedAt: t0.Add(-time.Hour)}))
		must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "s-carol", AccountID: "acct-carol", Name: "session", Kind: domain.TokenSession, CreatedAt: t0}))
	}
	runIAMCases(t, []endpointCase{
		{name: "list", method: "GET", path: "/api/v1/auth/sessions", as: "bob", setup: seed, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				var ss []map[string]any
				must(t, json.Unmarshal(rec.Body.Bytes(), &ss))
				if len(ss) != 2 || ss[0]["id"] != "s-cur" || ss[0]["current"] != true || ss[1]["current"] != false || ss[0]["last_ip"] != "10.0.0.1" {
					t.Fatalf("sessions = %s", rec.Body.String())
				}
				if strings.Contains(rec.Body.String(), "token_hash") {
					t.Fatalf("leaks hash")
				}
			}},
		{name: "list anonymous", method: "GET", path: "/api/v1/auth/sessions", status: 401},
		{name: "revoke", method: "DELETE", path: "/api/v1/auth/sessions/s-old", as: "bob", setup: seed, status: 204,
			check: func(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) {
				if ids := tokenIDs(f, "acct-bob"); !slices.Equal(ids, []string{"s-cur"}) {
					t.Fatalf("tokens = %v", ids)
				}
				hasAudit(t, f, "session.revoke")
			}},
		{name: "revoke other account's session", method: "DELETE", path: "/api/v1/auth/sessions/s-carol", as: "bob", setup: seed, status: 404},
		{name: "revoke with scoped token", method: "DELETE", path: "/api/v1/auth/sessions/s-old", as: "scoped", status: 403,
			setup: func(t *testing.T, f *fixture) { seed(t, f); addScopedToken(f, "bob", "scoped", "tree:read") }},
		{name: "revoke others", method: "POST", path: "/api/v1/auth/sessions/revoke-others", as: "bob", setup: seed, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[revokedResponse](t, rec); r.Revoked != 1 {
					t.Fatalf("revoked = %+v", r)
				}
				if keep := f.iamSec().revoked["acct-bob"]; keep != hashToken("bob") {
					t.Fatalf("kept %q", keep)
				}
				if e := auditDetail(t, f, "session.revoke_others"); e.Details["revoked"] != "1" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "service account", method: "GET", path: "/api/v1/auth/sessions", as: "sa-token", status: 403,
			setup: func(t *testing.T, f *fixture) { addServiceAccountToken(f, "sa-token") }},
	})
}

func TestIAMMFA(t *testing.T) {
	enroll := func(t *testing.T, f *fixture) {
		_, _, err := f.iamSec().BeginTOTPEnroll(context.Background(), "acct-bob")
		must(t, err)
	}
	enabled := func(t *testing.T, f *fixture) {
		enroll(t, f)
		_, err := f.iamSec().ConfirmTOTP(context.Background(), "acct-bob", iamTestTOTP)
		must(t, err)
	}
	runIAMCases(t, []endpointCase{
		{name: "status", method: "GET", path: "/api/v1/auth/mfa", as: "bob", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if st := decode[domain.MFAState](t, rec); st.TOTPEnabled || st.AccountID != "acct-bob" {
					t.Fatalf("state = %s", rec.Body.String())
				}
			}},
		{name: "status enabled hides secrets", method: "GET", path: "/api/v1/auth/mfa", as: "bob", setup: enabled, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if !strings.Contains(rec.Body.String(), `"totp_enabled":true`) || strings.Contains(rec.Body.String(), "JBSW") {
					t.Fatalf("state = %s", rec.Body.String())
				}
			}},
		{name: "status anonymous", method: "GET", path: "/api/v1/auth/mfa", status: 401},
		{name: "enroll", method: "POST", path: "/api/v1/auth/mfa/totp/enroll", as: "bob", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[mfaEnrollResponse](t, rec)
				if r.Secret == "" || !strings.HasPrefix(r.OTPAuthURL, "otpauth://totp/") || rec.Header().Get("Cache-Control") != "no-store" {
					t.Fatalf("enroll = %s", rec.Body.String())
				}
				hasAudit(t, f, "mfa.enroll")
			}},
		{name: "enroll scoped token", method: "POST", path: "/api/v1/auth/mfa/totp/enroll", as: "scoped", status: 403,
			setup: func(t *testing.T, f *fixture) { addScopedToken(f, "bob", "scoped", "*") }},
		{name: "confirm", method: "POST", path: "/api/v1/auth/mfa/totp/confirm", as: "bob", setup: enroll, body: map[string]string{"code": "123456"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[recoveryCodesResponse](t, rec); len(r.RecoveryCodes) != 2 {
					t.Fatalf("codes = %s", rec.Body.String())
				}
				hasAudit(t, f, "mfa.enable")
			}},
		{name: "confirm wrong code", method: "POST", path: "/api/v1/auth/mfa/totp/confirm", as: "bob", setup: enroll, body: map[string]string{"code": "000000"}, status: 403},
		{name: "confirm without enrollment", method: "POST", path: "/api/v1/auth/mfa/totp/confirm", as: "bob", body: map[string]string{"code": "123456"}, status: 409},
		{name: "confirm missing code", method: "POST", path: "/api/v1/auth/mfa/totp/confirm", as: "bob", body: map[string]string{}, status: 400},
		{name: "disable", method: "DELETE", path: "/api/v1/auth/mfa/totp", as: "bob", setup: enabled, body: map[string]string{"password": "bob-password"}, status: 204,
			check: audited("mfa.disable")},
		{name: "disable wrong password", method: "DELETE", path: "/api/v1/auth/mfa/totp", as: "bob", setup: enabled, body: map[string]string{"password": "x"}, status: 403},
		{name: "disable missing password", method: "DELETE", path: "/api/v1/auth/mfa/totp", as: "bob", body: map[string]string{}, status: 400},
		{name: "recovery codes", method: "POST", path: "/api/v1/auth/mfa/recovery-codes", as: "bob", setup: enabled, body: map[string]string{"code": "123456"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[recoveryCodesResponse](t, rec); !slices.Equal(r.RecoveryCodes, []string{"eeee-ffff"}) {
					t.Fatalf("codes = %s", rec.Body.String())
				}
				hasAudit(t, f, "mfa.recovery_codes")
			}},
		{name: "recovery codes wrong code", method: "POST", path: "/api/v1/auth/mfa/recovery-codes", as: "bob", body: map[string]string{"code": "1"}, status: 403},
		{name: "service account", method: "POST", path: "/api/v1/auth/mfa/totp/enroll", as: "sa-token", status: 403,
			setup: func(t *testing.T, f *fixture) { addServiceAccountToken(f, "sa-token") }},
	})
}

func TestIAMInvitations(t *testing.T) {
	const invs = "/api/v1/tenants/acme/invitations"
	// invite creates an invitation through the fake and returns its code.
	invite := func(t *testing.T, f *fixture, inv domain.Invitation) string {
		t.Helper()
		inv.TenantID = f.acme.ID
		code, err := f.iamSec().CreateInvitation(context.Background(), &inv, time.Hour)
		must(t, err)
		f.iamStore().imu.Lock()
		defer f.iamStore().imu.Unlock()
		for id, x := range f.iamStore().invitations {
			if x.CodeHash == inv.CodeHash {
				x.ID = "inv-1"
				delete(f.iamStore().invitations, id)
				f.iamStore().invitations["inv-1"] = x
			}
		}
		return code
	}
	runIAMCases(t, []endpointCase{
		{name: "create", method: "POST", path: invs, as: "alice", body: map[string]string{"email": "new@example.com", "role": "auditor", "team": "backend"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[createInvitationResponse](t, rec)
				if !strings.HasPrefix(r.Code, "wti_") || r.Invitation.TeamID != "team-backend" || r.Invitation.Role != "auditor" ||
					r.Invitation.InvitedBy != "alice" || r.Invitation.Email != "new@example.com" {
					t.Fatalf("invitation = %s", rec.Body.String())
				}
				if d := r.Invitation.ExpiresAt.Sub(r.Invitation.CreatedAt); d != 7*24*time.Hour {
					t.Fatalf("ttl = %v", d)
				}
				if strings.Contains(rec.Body.String(), "code_hash") {
					t.Fatalf("leaks hash")
				}
				if e := auditDetail(t, f, "invitation.create"); e.Details["email"] != "new@example.com" || e.Details["team"] != "backend" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "create open default role", method: "POST", path: invs, as: "alice", body: map[string]any{"ttl_hours": 2}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[createInvitationResponse](t, rec)
				if r.Invitation.Role != domain.RoleDeveloper || r.Invitation.ExpiresAt.Sub(r.Invitation.CreatedAt) != 2*time.Hour {
					t.Fatalf("invitation = %s", rec.Body.String())
				}
			}},
		{name: "create for username", method: "POST", path: invs, as: "alice", body: map[string]string{"username": "dave"}, status: 201},
		{name: "create both", method: "POST", path: invs, as: "alice", body: map[string]string{"email": "a@b.c", "username": "dave"}, status: 400},
		{name: "create bad email", method: "POST", path: invs, as: "alice", body: map[string]string{"email": "nope"}, status: 400},
		{name: "create unknown team", method: "POST", path: invs, as: "alice", body: map[string]string{"team": "zed"}, status: 404},
		{name: "create unknown role", method: "POST", path: invs, as: "alice", body: map[string]string{"role": "wizard"}, status: 400},
		{name: "create existing member", method: "POST", path: invs, as: "alice", body: map[string]string{"username": "bob"}, status: 409},
		{name: "create unknown user", method: "POST", path: invs, as: "alice", body: map[string]string{"username": "zed"}, status: 404},
		{name: "create owner by admin", method: "POST", path: invs, as: "mallory", body: map[string]string{"role": "owner"}, status: 403},
		{name: "create maintainer forbidden", method: "POST", path: invs, as: "maria", body: map[string]string{}, status: 403},
		{name: "list", method: "GET", path: invs, as: "maria", status: 200,
			setup: func(t *testing.T, f *fixture) { invite(t, f, domain.Invitation{Role: "viewer"}) },
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if l := decode[[]domain.Invitation](t, rec); len(l) != 1 {
					t.Fatalf("invitations = %s", rec.Body.String())
				}
			}},
		{name: "revoke", method: "DELETE", path: invs + "/inv-1", as: "alice", status: 204,
			setup: func(t *testing.T, f *fixture) { invite(t, f, domain.Invitation{Role: "viewer"}) },
			check: func(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) {
				if !f.iamStore().invitations["inv-1"].Revoked {
					t.Fatalf("not revoked")
				}
				hasAudit(t, f, "invitation.revoke")
			}},
		{name: "revoke missing", method: "DELETE", path: invs + "/nope", as: "alice", status: 404},
		{name: "preview public", method: "GET", path: "/api/v1/invitations/CODE", status: 200,
			setup: func(t *testing.T, f *fixture) {
				code := invite(t, f, domain.Invitation{Role: "viewer"})
				f.iamStore().imu.Lock()
				x := f.iamStore().invitations["inv-1"]
				x.CodeHash = hashToken("CODE")
				f.iamStore().invitations["inv-1"] = x
				f.iamStore().imu.Unlock()
				_ = code
			},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				p := decode[invitationPreview](t, rec)
				if p.Invitation.ID != "inv-1" || p.Tenant.Slug != "acme" {
					t.Fatalf("preview = %s", rec.Body.String())
				}
			}},
		{name: "preview unknown", method: "GET", path: "/api/v1/invitations/nope", status: 404},
		{name: "accept", method: "POST", path: "/api/v1/invitations/CODE/accept", as: "dave", status: 200,
			setup: func(t *testing.T, f *fixture) {
				invite(t, f, domain.Invitation{Role: "viewer"})
				f.iamStore().imu.Lock()
				x := f.iamStore().invitations["inv-1"]
				x.CodeHash = hashToken("CODE")
				f.iamStore().invitations["inv-1"] = x
				f.iamStore().imu.Unlock()
			},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				inv := decode[domain.Invitation](t, rec)
				if inv.AcceptedBy != "dave" {
					t.Fatalf("invitation = %s", rec.Body.String())
				}
				if m, err := f.store.GetMembership(context.Background(), f.acme.ID, "acct-dave"); err != nil || m.Role != "viewer" {
					t.Fatalf("membership = %+v, %v", m, err)
				}
				if e := auditDetail(t, f, "invitation.accept"); e.TenantID != f.acme.ID || e.Actor != "dave" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "accept anonymous", method: "POST", path: "/api/v1/invitations/CODE/accept", status: 401},
		{name: "accept for another user", method: "POST", path: "/api/v1/invitations/CODE/accept", as: "carol", status: 403,
			setup: func(t *testing.T, f *fixture) {
				invite(t, f, domain.Invitation{Role: "viewer", Username: "dave"})
				f.iamStore().imu.Lock()
				x := f.iamStore().invitations["inv-1"]
				x.CodeHash = hashToken("CODE")
				f.iamStore().invitations["inv-1"] = x
				f.iamStore().imu.Unlock()
			}},
	})
}

func TestIAMPasswordReset(t *testing.T) {
	runIAMCases(t, []endpointCase{
		{name: "issue code without body", method: "POST", path: "/api/v1/admin/accounts/bob/reset-code", as: "admin", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[resetCodeResponse](t, rec)
				if !strings.HasPrefix(r.Code, "wtr_") || time.Until(r.ExpiresAt) > time.Hour || time.Until(r.ExpiresAt) < 59*time.Minute {
					t.Fatalf("reset = %s", rec.Body.String())
				}
				if e := auditDetail(t, f, "password.reset_code"); e.ResourceID != "acct-bob" || strings.Contains(strings.Join(mapValues(e.Details), ""), r.Code) {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "issue code with ttl", method: "POST", path: "/api/v1/admin/accounts/bob/reset-code", as: "admin", body: map[string]int{"ttl_hours": 3}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[resetCodeResponse](t, rec); time.Until(r.ExpiresAt) < 2*time.Hour {
					t.Fatalf("reset = %s", rec.Body.String())
				}
			}},
		{name: "issue code bad ttl", method: "POST", path: "/api/v1/admin/accounts/bob/reset-code", as: "admin", body: map[string]int{"ttl_hours": 9999}, status: 400},
		{name: "issue code non admin", method: "POST", path: "/api/v1/admin/accounts/bob/reset-code", as: "alice", status: 403},
		{name: "issue code anonymous", method: "POST", path: "/api/v1/admin/accounts/bob/reset-code", status: 401},
		{name: "issue code unknown user", method: "POST", path: "/api/v1/admin/accounts/zed/reset-code", as: "admin", status: 404},
		{name: "redeem", method: "POST", path: "/api/v1/auth/password-reset", body: map[string]string{"code": "wtr_fixed", "new_password": "brand-new-pass"}, status: 204,
			setup: func(t *testing.T, f *fixture) {
				must(t, f.iamStore().PutPasswordReset(context.Background(), &domain.PasswordReset{CodeHash: hashToken("wtr_fixed"), AccountID: "acct-bob", ExpiresAt: time.Now().Add(time.Hour)}))
			},
			check: func(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) {
				if f.auth.passwords["bob"] != "brand-new-pass" {
					t.Fatalf("password not changed")
				}
				if e := auditDetail(t, f, "password.reset"); e.Actor != "bob" || e.ResourceID != "acct-bob" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "redeem bad code", method: "POST", path: "/api/v1/auth/password-reset", body: map[string]string{"code": "wtr_nope", "new_password": "brand-new-pass"}, status: 400},
		{name: "redeem short password", method: "POST", path: "/api/v1/auth/password-reset", body: map[string]string{"code": "wtr_x", "new_password": "short"}, status: 400},
	})
}

func TestIAMSimulate(t *testing.T) {
	const sim = "/api/v1/iam/simulate"
	runIAMCases(t, []endpointCase{
		{name: "user", method: "POST", path: sim, as: "alice", status: 200,
			body: map[string]any{"principal": map[string]string{"type": "user", "name": "bob"}, "action": "tree:read", "tenant": "acme",
				"tree": "web", "branch": "main", "path": "/src/x.go", "context": map[string]any{"ip": "10.1.2.3", "mfa": true}},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				d := decode[domain.Decision](t, rec)
				if !d.Allowed || len(d.Reasons) != 1 {
					t.Fatalf("decision = %s", rec.Body.String())
				}
				e := f.iamAcc()
				if e.lastP.AccountID != "acct-bob" || !e.lastP.MFA || e.lastP.Kind != domain.PrincipalUser || e.lastAct != domain.ActTreeRead ||
					e.lastRes.Tree == nil || e.lastRes.Tree.ID != f.web.ID || e.lastRes.Branch != "main" || e.lastRes.Path != "src/x.go" ||
					e.lastRes.TenantID != f.acme.ID || e.lastEnv.RemoteIP != "10.1.2.3" || e.lastEnv.Now.IsZero() {
					t.Fatalf("evaluate args = %+v %+v %+v", e.lastP, e.lastRes, e.lastEnv)
				}
			}},
		{name: "deny has empty reasons array", method: "POST", path: sim, as: "alice", status: 200,
			body: map[string]any{"principal": map[string]string{"type": "user", "name": "bob"}, "action": "tree:admin", "tenant": "acme"},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if !strings.Contains(rec.Body.String(), `"reasons":[]`) || !strings.Contains(rec.Body.String(), `"implicit_deny"`) {
					t.Fatalf("decision = %s", rec.Body.String())
				}
			}},
		{name: "service account", method: "POST", path: sim, as: "maria", status: 200,
			body: map[string]any{"principal": map[string]string{"type": "service_account", "name": "ci"}, "action": "tree:write", "tenant": "acme", "tree": "acme/web"},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				p := f.iamAcc().lastP
				if p.Kind != domain.PrincipalServiceAccount || p.Username != "sa:acme/ci" || p.ServiceAccountID != "sa-ci" || p.AccountID != "" || p.TenantID != f.acme.ID {
					t.Fatalf("principal = %+v", p)
				}
			}},
		{name: "caller by default", method: "POST", path: sim, as: "carol", body: map[string]any{"action": "tree:read", "tenant": "acme"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if p := f.iamAcc().lastP; p.AccountID != "acct-carol" {
					t.Fatalf("principal = %+v", p)
				}
			}},
		{name: "self by name without iam:read", method: "POST", path: sim, as: "carol",
			body: map[string]any{"principal": map[string]string{"type": "user", "name": "carol"}, "action": "tree:read", "tenant": "acme"}, status: 200},
		{name: "other without iam:read", method: "POST", path: sim, as: "carol",
			body: map[string]any{"principal": map[string]string{"type": "user", "name": "bob"}, "action": "tree:read", "tenant": "acme"}, status: 403},
		{name: "unknown user hidden without iam:read", method: "POST", path: sim, as: "dave",
			body: map[string]any{"principal": map[string]string{"type": "user", "name": "zed"}, "action": "tree:read", "tenant": "acme"}, status: 403},
		{name: "unknown user", method: "POST", path: sim, as: "alice",
			body: map[string]any{"principal": map[string]string{"type": "user", "name": "zed"}, "action": "tree:read", "tenant": "acme"}, status: 404},
		{name: "unknown action", method: "POST", path: sim, as: "alice", body: map[string]any{"action": "tree:fly", "tenant": "acme"}, status: 400},
		{name: "missing tenant", method: "POST", path: sim, as: "alice", body: map[string]any{"action": "tree:read"}, status: 400},
		{name: "bad ip", method: "POST", path: sim, as: "alice", body: map[string]any{"action": "tree:read", "tenant": "acme", "context": map[string]any{"ip": "x"}}, status: 400},
		{name: "bad principal type", method: "POST", path: sim, as: "alice",
			body: map[string]any{"principal": map[string]string{"type": "robot", "name": "x"}, "action": "tree:read", "tenant": "acme"}, status: 400},
		{name: "branch without tree", method: "POST", path: sim, as: "alice", body: map[string]any{"action": "branch:push", "tenant": "acme", "branch": "main"}, status: 400},
		{name: "unknown tree", method: "POST", path: sim, as: "alice", body: map[string]any{"action": "tree:read", "tenant": "acme", "tree": "nope"}, status: 404},
		{name: "anonymous", method: "POST", path: sim, body: map[string]any{"action": "tree:read", "tenant": "acme"}, status: 401},
	})
}

func TestIAMEffective(t *testing.T) {
	const eff = "/api/v1/iam/effective"
	runIAMCases(t, []endpointCase{
		{name: "user in tenant", method: "GET", path: eff + "?tenant=acme&principal=user:bob", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[effectiveResponse](t, rec)
				if r.Principal != "user:bob" || r.Role != domain.RoleDeveloper || len(r.TeamRoles) != 1 ||
					r.TeamRoles[0] != (teamRoleView{Team: "backend", Role: domain.TeamRoleMaintainer}) ||
					len(r.Grants) != 1 || r.Grants[0].Subject != "backend" ||
					!slices.Equal(r.Permissions, []domain.Action{domain.ActTreeRead, domain.ActTreeWrite}) {
					t.Fatalf("effective = %s", rec.Body.String())
				}
				if f.iamAcc().lastTree != nil || f.iamAcc().lastP.AccountID != "acct-bob" {
					t.Fatalf("evaluator args = %+v %+v", f.iamAcc().lastP, f.iamAcc().lastTree)
				}
			}},
		{name: "tree derives tenant", method: "GET", path: eff + "?tree=acme/web&principal=user:carol", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[effectiveResponse](t, rec)
				if r.Role != domain.RoleViewer || len(r.Grants) != 1 || f.iamAcc().lastTree == nil || f.iamAcc().lastTree.ID != f.web.ID {
					t.Fatalf("effective = %s", rec.Body.String())
				}
			}},
		{name: "tree without matching grants", method: "GET", path: eff + "?tenant=acme&tree=docs&principal=user:bob", as: "alice", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[effectiveResponse](t, rec); r.Grants == nil || len(r.Grants) != 0 {
					t.Fatalf("effective = %s", rec.Body.String())
				}
			}},
		{name: "caller default", method: "GET", path: eff + "?tenant=acme", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[effectiveResponse](t, rec); r.Principal != "user:carol" || r.Role != domain.RoleViewer {
					t.Fatalf("effective = %s", rec.Body.String())
				}
			}},
		{name: "service account", method: "GET", path: eff + "?principal=sa:acme/ci", as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[effectiveResponse](t, rec)
				if r.Principal != "sa:acme/ci" || r.Role != domain.RoleDeveloper || len(r.TeamRoles) != 0 {
					t.Fatalf("effective = %s", rec.Body.String())
				}
			}},
		{name: "service account caller", method: "GET", path: eff, as: "sa-token", status: 200,
			setup: func(t *testing.T, f *fixture) { addServiceAccountToken(f, "sa-token") },
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[effectiveResponse](t, rec); r.Principal != "sa:acme/ci" {
					t.Fatalf("effective = %s", rec.Body.String())
				}
			}},
		{name: "other without iam:read", method: "GET", path: eff + "?tenant=acme&principal=user:bob", as: "carol", status: 403},
		{name: "bad principal", method: "GET", path: eff + "?tenant=acme&principal=bob", as: "alice", status: 400},
		{name: "missing tenant", method: "GET", path: eff + "?principal=user:bob", as: "alice", status: 400},
		{name: "unknown tenant", method: "GET", path: eff + "?tenant=nope", as: "alice", status: 404},
		{name: "anonymous", method: "GET", path: eff + "?tenant=acme", status: 401},
	})
}

func TestIAMEvaluatorMissing(t *testing.T) {
	runIAMCases(t, []endpointCase{
		{name: "simulate", method: "POST", path: "/api/v1/iam/simulate", as: "alice", body: map[string]any{"action": "tree:read", "tenant": "acme"}, status: 501, code: "not_implemented"},
		{name: "effective", method: "GET", path: "/api/v1/iam/effective?tenant=acme", as: "alice", status: 501, code: "not_implemented"},
		{name: "validate falls back to structural checks", method: "POST", path: "/api/v1/tenants/acme/policies/validate", as: "alice",
			body: map[string]any{"statements": []any{map[string]any{"effect": "allow"}}}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if v := decode[validatePolicyResponse](t, rec); v.Valid || len(v.Errors) != 3 {
					t.Fatalf("validate = %s", rec.Body.String())
				}
			}},
	}, noAccess)
}

func TestValidateScopes(t *testing.T) {
	scoped := &domain.Principal{Scopes: []string{"tree:*", "mr:create"}}
	for _, tc := range []struct {
		in     []string
		caller *domain.Principal
		ok     bool
	}{
		{[]string{"tree:read"}, nil, true},
		{[]string{"*"}, nil, true},
		{[]string{"iam:*"}, nil, true},
		{[]string{"tree:fly"}, nil, false},
		{[]string{"nope:*"}, nil, false},
		{[]string{"tree:read", "tree:*"}, scoped, true},
		{[]string{"mr:create"}, scoped, true},
		{[]string{"mr:merge"}, scoped, false},
		{[]string{"mr:*"}, scoped, false},
		{[]string{"*"}, scoped, false},
		{nil, scoped, false},
	} {
		_, err := validateScopes(tc.in, tc.caller)
		if (err == nil) != tc.ok {
			t.Errorf("validateScopes(%v, %v) err = %v, want ok=%v", tc.in, tc.caller, err, tc.ok)
		}
	}
}

func TestDecodeOptionalJSON(t *testing.T) {
	for _, body := range []string{"", "  \n"} {
		req := httptest.NewRequest("POST", "/", bytes.NewBufferString(body))
		v := resetCodeRequest{TTLHours: 7}
		if err := decodeOptionalJSON(httptest.NewRecorder(), req, &v); err != nil || v.TTLHours != 7 {
			t.Fatalf("body %q: %v %+v", body, err, v)
		}
	}
	req := httptest.NewRequest("POST", "/", bytes.NewBufferString(`{"bogus":1}`))
	if err := decodeOptionalJSON(httptest.NewRecorder(), req, &resetCodeRequest{}); err == nil {
		t.Fatal("unknown field accepted")
	}
}
