package iam

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestLoginWithMetaNoMFA(t *testing.T) {
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	// Meta falls back to the request context.
	ctx := WithUserAgent(WithRemoteAddr(context.Background(), "[2001:db8::1]:5000"), "Mozilla/5.0\x00\n")
	res, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
	if err != nil || res.MFARequired || res.MFAToken != "" || res.Account.ID != a.ID || !wellFormedToken(res.Token) {
		t.Fatalf("LoginWithMeta = %+v, %v", res, err)
	}
	p, err := e.svc.Authenticate(context.Background(), res.Token)
	if err != nil || p.MFA || p.Kind != domain.PrincipalUser || p.Scopes != nil {
		t.Fatalf("Authenticate = %+v, %v", p, err)
	}
	tok, _ := e.store.token(p.TokenID)
	if tok.Kind != domain.TokenSession || tok.Name != "session" || tok.LastIP != "2001:db8::1" || tok.UserAgent != "Mozilla/5.0" {
		t.Fatalf("session %+v", tok)
	}
	// Errors match Login.
	if _, err := e.svc.LoginWithMeta(ctx, "alice", "wrong password", domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("wrong password = %v", err)
	}
}

func TestLoginWithMetaRateLimitUsesMetaIP(t *testing.T) {
	e := newTestEnv(t, func(o *Options) {
		o.DisableRateLimit = false
		o.MaxLoginFailures = 2
	})
	e.register(t, "alice")
	ctx := context.Background()
	m1 := domain.SessionMeta{RemoteIP: "198.51.100.1:1"}
	for i := 0; i < 2; i++ {
		_, _ = e.svc.LoginWithMeta(ctx, "alice", "wrong password", m1)
	}
	if _, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, m1); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("blocked ip = %v", err)
	}
	// Another address (different port of a different host) is independent.
	if _, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{RemoteIP: "198.51.100.2:1"}); err != nil {
		t.Fatalf("other ip = %v", err)
	}
}

func TestLoginRecordsSessionKind(t *testing.T) {
	e := newTestEnv(t, nil)
	e.register(t, "alice")
	ctx := WithUserAgent(WithRemoteAddr(context.Background(), "192.0.2.5:999"), "curl/8")
	raw, _, err := e.svc.Login(ctx, "alice", testPassword)
	if err != nil {
		t.Fatal(err)
	}
	p, _ := e.svc.Authenticate(ctx, raw)
	tok, _ := e.store.token(p.TokenID)
	if tok.Kind != domain.TokenSession || tok.MFA || tok.LastIP != "192.0.2.5" || tok.UserAgent != "curl/8" {
		t.Fatalf("session %+v", tok)
	}
}

func TestIssueSession(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	raw, err := e.svc.IssueSession(ctx, a.ID, true, domain.SessionMeta{RemoteIP: "192.0.2.1", UserAgent: "sso"})
	if err != nil {
		t.Fatal(err)
	}
	p, err := e.svc.Authenticate(ctx, raw)
	if err != nil || !p.MFA || p.AccountID != a.ID {
		t.Fatalf("Authenticate = %+v, %v", p, err)
	}
	if tok, _ := e.store.token(p.TokenID); tok.Kind != domain.TokenSession || tok.LastIP != "192.0.2.1" || tok.UserAgent != "sso" {
		t.Fatalf("session %+v", tok)
	}
	if _, err := e.svc.IssueSession(ctx, "nope", false, domain.SessionMeta{}); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown account = %v", err)
	}
	a.Disabled = true
	_ = e.store.UpdateAccount(ctx, a)
	if _, err := e.svc.IssueSession(ctx, a.ID, false, domain.SessionMeta{}); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("disabled account = %v", err)
	}
}

func TestMFAFlagOnlyForSessions(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	// A forged/legacy API token carrying MFA=true must not grant MFA.
	_, rec, err := e.svc.CreateToken(ctx, a.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	raw, _, _ := newToken()
	e.store.mu.Lock()
	tk := e.store.tokens[rec.ID]
	tk.MFA = true
	tk.TokenHash = HashToken(raw)
	e.store.tokens[rec.ID] = tk
	e.store.mu.Unlock()
	p, err := e.svc.Authenticate(ctx, raw)
	if err != nil || p.MFA {
		t.Fatalf("Authenticate = %+v, %v", p, err)
	}
}

func TestListSessions(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	b := e.register(t, "bob")
	s1, _, _ := e.svc.Login(ctx, "alice", testPassword)
	e.clock.Advance(time.Second)
	_, api, _ := e.svc.CreateToken(ctx, a.ID, "ci", 0)
	e.clock.Advance(time.Second)
	s2, _, _ := e.svc.Login(ctx, "alice", testPassword)
	_, _, _ = e.svc.Login(ctx, "bob", testPassword)
	// A pre-v0.4 session (no Kind) and an expired session.
	past := e.clock.Now().Add(-time.Hour)
	_ = e.store.CreateToken(ctx, &domain.APIToken{ID: "legacy", AccountID: a.ID, Name: "session", TokenHash: "h1", CreatedAt: past})
	_ = e.store.CreateToken(ctx, &domain.APIToken{ID: "expired", AccountID: a.ID, Name: "session", Kind: domain.TokenSession,
		TokenHash: "h2", CreatedAt: past, ExpiresAt: &past})

	got, err := e.svc.ListSessions(ctx, a.ID)
	if err != nil {
		t.Fatal(err)
	}
	ids := []string{}
	for _, s := range got {
		ids = append(ids, s.ID)
		if s.ID == api.ID || s.AccountID != a.ID {
			t.Fatalf("unexpected token in sessions: %+v", s)
		}
	}
	if len(got) != 3 || got[0].TokenHash != HashToken(s2) || got[1].TokenHash != HashToken(s1) || got[2].ID != "legacy" {
		t.Fatalf("sessions = %v", ids)
	}
	if none, err := e.svc.ListSessions(ctx, "nobody"); err != nil || none == nil || len(none) != 0 {
		t.Fatalf("ListSessions(nobody) = %v, %v", none, err)
	}
	if bs, _ := e.svc.ListSessions(ctx, b.ID); len(bs) != 1 {
		t.Fatalf("bob sessions = %d", len(bs))
	}
}

func TestRevokeOtherSessions(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	e.register(t, "bob")
	cur, _, _ := e.svc.Login(ctx, "alice", testPassword)
	other1, _, _ := e.svc.Login(ctx, "alice", testPassword)
	other2, _, _ := e.svc.Login(ctx, "alice", testPassword)
	api, _, _ := e.svc.CreateToken(ctx, a.ID, "ci", 0)
	bob, _, _ := e.svc.Login(ctx, "bob", testPassword)

	n, err := e.svc.RevokeOtherSessions(ctx, a.ID, HashToken(cur))
	if err != nil || n != 2 {
		t.Fatalf("RevokeOtherSessions = %d, %v", n, err)
	}
	for _, tc := range []struct {
		tok string
		ok  bool
	}{{cur, true}, {other1, false}, {other2, false}, {api, true}, {bob, true}} {
		_, err := e.svc.Authenticate(ctx, tc.tok)
		if (err == nil) != tc.ok {
			t.Fatalf("Authenticate after revoke-others: ok=%v err=%v", tc.ok, err)
		}
	}
	// Empty keep hash revokes every session.
	if n, err := e.svc.RevokeOtherSessions(ctx, a.ID, ""); err != nil || n != 1 {
		t.Fatalf("revoke all = %d, %v", n, err)
	}
	if _, err := e.svc.RevokeOtherSessions(ctx, "", ""); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty account = %v", err)
	}
}

func TestAuthenticateRecordsClientMeta(t *testing.T) {
	st := newFakeStore()
	clk := newFakeClock()
	svc := New(metaStore{st}, Options{AllowRegistration: true, DisableRateLimit: true, Password: fastParams, Clock: clk.Now})
	ctx := context.Background()
	a, err := svc.Register(ctx, "alice", "alice@example.com", testPassword)
	if err != nil {
		t.Fatal(err)
	}
	raw, rec, err := svc.CreateToken(ctx, a.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	rctx := WithUserAgent(WithRemoteAddr(ctx, "203.0.113.9:1234"), "wt/0.4")
	if _, err := svc.Authenticate(rctx, raw); err != nil {
		t.Fatal(err)
	}
	tok, _ := st.token(rec.ID)
	if tok.LastIP != "203.0.113.9" || tok.UserAgent != "wt/0.4" || tok.LastUsedAt == nil {
		t.Fatalf("token after use %+v", tok)
	}
	// Throttled like TouchToken; a request without client info keeps it.
	clk.Advance(2 * time.Minute)
	if _, err := svc.Authenticate(ctx, raw); err != nil {
		t.Fatal(err)
	}
	tok, _ = st.token(rec.ID)
	if tok.LastIP != "203.0.113.9" || tok.UserAgent != "wt/0.4" || st.iam().metaTouches != 2 {
		t.Fatalf("token after anonymous-meta use %+v (touches %d)", tok, st.iam().metaTouches)
	}
}

func TestTokenKind(t *testing.T) {
	tests := []struct {
		tok  domain.APIToken
		want string
	}{
		{domain.APIToken{Name: "session"}, domain.TokenSession},
		{domain.APIToken{Name: "ci"}, domain.TokenAPI},
		{domain.APIToken{Name: "session", Kind: domain.TokenAPI}, domain.TokenAPI},
		{domain.APIToken{ServiceAccountID: "sa"}, domain.TokenService},
		{domain.APIToken{Kind: domain.TokenService}, domain.TokenService},
	}
	for _, tt := range tests {
		if got := TokenKind(&tt.tok); got != tt.want {
			t.Errorf("TokenKind(%+v) = %q, want %q", tt.tok, got, tt.want)
		}
	}
	if TokenKind(nil) != "" {
		t.Error("TokenKind(nil) not empty")
	}
}

func TestClientMetaNormalization(t *testing.T) {
	ips := map[string]string{
		"192.0.2.1":           "192.0.2.1",
		"192.0.2.1:80":        "192.0.2.1",
		"[2001:db8::1]:443":   "2001:db8::1",
		"2001:DB8::1":         "2001:db8::1",
		"[fe80::1%eth0]:1":    "fe80::1",
		"localhost:80":        "",
		"not an ip":           "",
		"":                    "",
		" 198.51.100.4 ":      "198.51.100.4",
		"::ffff:192.0.2.1:80": "",
	}
	for in, want := range ips {
		if got := normalizeIP(in); got != want {
			t.Errorf("normalizeIP(%q) = %q, want %q", in, got, want)
		}
	}
	long := strings.Repeat("é", 200) // 400 bytes
	if got := sanitizeUserAgent(long); len(got) > maxUserAgentLen || !strings.HasPrefix(long, got) || len(got) < maxUserAgentLen-1 {
		t.Errorf("sanitizeUserAgent truncation: %d bytes", len(got))
	}
	if got := sanitizeUserAgent(" a\tb\r\nc\x7f\xff "); got != "abc" {
		t.Errorf("sanitizeUserAgent = %q", got)
	}
}
