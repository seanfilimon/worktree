package iam

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// loginPair logs username in with a password and upgrades the session to
// a token pair, as POST /auth/login does.
func (e *testEnv) loginPair(t *testing.T, username string) *TokenPair {
	t.Helper()
	ctx := context.Background()
	res, err := e.svc.LoginWithMeta(ctx, username, testPassword, domain.SessionMeta{RemoteIP: "198.51.100.7", UserAgent: "wt/1"})
	if err != nil {
		t.Fatalf("LoginWithMeta: %v", err)
	}
	pair, err := e.svc.UpgradeSession(ctx, res.Token, domain.SessionMeta{})
	if err != nil {
		t.Fatalf("UpgradeSession: %v", err)
	}
	return pair
}

func (e *testEnv) authOK(t *testing.T, token string) *domain.Principal {
	t.Helper()
	p, err := e.svc.Authenticate(context.Background(), token)
	if err != nil {
		t.Fatalf("Authenticate: %v", err)
	}
	return p
}

func (e *testEnv) authFails(t *testing.T, token string) {
	t.Helper()
	if _, err := e.svc.Authenticate(context.Background(), token); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("Authenticate = %v, want ErrUnauthenticated", err)
	}
}

func TestUpgradeSessionIssuesPair(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	res, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{RemoteIP: "198.51.100.7", UserAgent: "wt/1"})
	if err != nil {
		t.Fatal(err)
	}
	pair, err := e.svc.UpgradeSession(ctx, res.Token, domain.SessionMeta{})
	if err != nil {
		t.Fatal(err)
	}
	e.authFails(t, res.Token) // the long-lived session was exchanged
	p := e.authOK(t, pair.AccessToken)
	if p.AccountID != alice.ID || p.TokenID != pair.Session.ID {
		t.Fatalf("principal = %+v", p)
	}
	now := e.clock.Now()
	if !pair.AccessExpiresAt.Equal(now.Add(DefaultAccessTTL)) || !pair.RefreshExpiresAt.Equal(now.Add(DefaultRefreshTTL)) {
		t.Fatalf("expiry access %v refresh %v (now %v)", pair.AccessExpiresAt, pair.RefreshExpiresAt, now)
	}
	if !strings.HasPrefix(pair.RefreshToken, RefreshTokenPrefix) {
		t.Fatalf("refresh token %q", pair.RefreshToken)
	}
	if _, ok := refreshFamilyID(pair.RefreshToken); !ok {
		t.Fatalf("refresh token is not well formed: %q", pair.RefreshToken)
	}
	if pair.Session.LastIP != "198.51.100.7" || pair.Session.UserAgent != "wt/1" || pair.Session.Name != sessionTokenName {
		t.Fatalf("session = %+v", pair.Session)
	}
	sessions, err := e.svc.ListSessions(ctx, alice.ID)
	if err != nil || len(sessions) != 1 || sessions[0].ID != pair.Session.ID {
		t.Fatalf("sessions = %+v, %v", sessions, err)
	}
	// Access tokens expire after an hour.
	e.clock.Advance(DefaultAccessTTL)
	e.authFails(t, pair.AccessToken)

	// Only sessions qualify.
	api, _, err := e.svc.CreateToken(ctx, alice.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := e.svc.UpgradeSession(ctx, api, domain.SessionMeta{}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("UpgradeSession(api token) = %v", err)
	}
	if _, err := e.svc.UpgradeSession(ctx, "wt_garbage", domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("UpgradeSession(garbage) = %v", err)
	}
}

func TestRefreshRotation(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	p1 := e.loginPair(t, "alice")

	e.clock.Advance(50 * time.Minute)
	p2, err := e.svc.Refresh(ctx, p1.RefreshToken, domain.SessionMeta{RemoteIP: "203.0.113.9", UserAgent: "wt/2"})
	if err != nil {
		t.Fatalf("Refresh: %v", err)
	}
	if p2.RefreshToken == p1.RefreshToken || p2.AccessToken == p1.AccessToken {
		t.Fatal("rotation must issue new tokens")
	}
	e.authFails(t, p1.AccessToken) // the previous access session is gone
	e.authOK(t, p2.AccessToken)
	if p2.Account == nil || p2.Account.ID != alice.ID || p2.Session.LastIP != "203.0.113.9" {
		t.Fatalf("pair = %+v / %+v", p2.Account, p2.Session)
	}
	if !p2.RefreshExpiresAt.Equal(e.clock.Now().Add(DefaultRefreshTTL)) {
		t.Fatalf("rotation restarts the refresh period: %v", p2.RefreshExpiresAt)
	}
	sessions, _ := e.svc.ListSessions(ctx, alice.ID)
	if len(sessions) != 1 || sessions[0].ID != p2.Session.ID {
		t.Fatalf("one session per login: %+v", sessions)
	}

	// A race (the previous token again, within the grace period) is
	// rejected without revoking anything.
	if _, err := e.svc.Refresh(ctx, p1.RefreshToken, domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) || errors.Is(err, ErrRefreshReused) {
		t.Fatalf("race = %v", err)
	}
	e.authOK(t, p2.AccessToken)

	p3, err := e.svc.Refresh(ctx, p2.RefreshToken, domain.SessionMeta{})
	if err != nil {
		t.Fatalf("Refresh 2: %v", err)
	}
	// Reusing an older token (its successor was used) revokes the family.
	if _, err := e.svc.Refresh(ctx, p1.RefreshToken, domain.SessionMeta{}); !errors.Is(err, ErrRefreshReused) {
		t.Fatalf("reuse = %v, want ErrRefreshReused", err)
	}
	e.authFails(t, p3.AccessToken)
	if _, err := e.svc.Refresh(ctx, p3.RefreshToken, domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("refresh after revocation = %v", err)
	}
}

func TestRefreshReuseAfterGrace(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	e.register(t, "alice")
	p1 := e.loginPair(t, "alice")
	p2, err := e.svc.Refresh(ctx, p1.RefreshToken, domain.SessionMeta{})
	if err != nil {
		t.Fatal(err)
	}
	e.clock.Advance(DefaultRefreshReuseGrace)
	if _, err := e.svc.Refresh(ctx, p1.RefreshToken, domain.SessionMeta{}); !errors.Is(err, ErrRefreshReused) {
		t.Fatalf("reuse after the grace period = %v", err)
	}
	e.authFails(t, p2.AccessToken)
	if _, err := e.svc.Refresh(ctx, p2.RefreshToken, domain.SessionMeta{}); err == nil {
		t.Fatal("the family must be revoked")
	}

	// Without a grace period every reuse revokes.
	e2 := newTestEnv(t, func(o *Options) { o.RefreshReuseGrace = -1 })
	e2.register(t, "bob")
	q1 := e2.loginPair(t, "bob")
	q2, err := e2.svc.Refresh(ctx, q1.RefreshToken, domain.SessionMeta{})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := e2.svc.Refresh(ctx, q1.RefreshToken, domain.SessionMeta{}); !errors.Is(err, ErrRefreshReused) {
		t.Fatalf("reuse without grace = %v", err)
	}
	e2.authFails(t, q2.AccessToken)
}

func TestRefreshExpiryAndInvalidTokens(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, func(o *Options) { o.AccessTTL = time.Minute; o.RefreshTTL = time.Hour })
	e.register(t, "alice")
	p1 := e.loginPair(t, "alice")
	if !p1.AccessExpiresAt.Equal(e.clock.Now().Add(time.Minute)) {
		t.Fatalf("AccessTTL option ignored: %v", p1.AccessExpiresAt)
	}
	e.clock.Advance(2 * time.Minute)
	e.authFails(t, p1.AccessToken)
	p2, err := e.svc.Refresh(ctx, p1.RefreshToken, domain.SessionMeta{})
	if err != nil {
		t.Fatalf("an expired access token is renewed: %v", err)
	}
	e.clock.Advance(time.Hour)
	if _, err := e.svc.Refresh(ctx, p2.RefreshToken, domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("expired refresh token = %v", err)
	}
	fam, _ := refreshFamilyID(p2.RefreshToken)
	for _, raw := range []string{
		"", "wtrt_", "wt_" + strings.Repeat("a", 43), p2.RefreshToken + "x",
		RefreshTokenPrefix + fam + "." + strings.Repeat("A", secretBody), // wrong secret
		RefreshTokenPrefix + "not-a-uuid-not-a-uuid-not-a-uuid-xxxx." + strings.Repeat("A", secretBody),
		RefreshTokenPrefix + "00000000-0000-4000-8000-000000000000." + strings.Repeat("A", secretBody), // unknown family
	} {
		if _, err := e.svc.Refresh(ctx, raw, domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) || errors.Is(err, ErrRefreshReused) {
			t.Errorf("Refresh(%q) = %v", raw, err)
		}
	}
}

func TestRefreshRevokedWithSessions(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")

	// Signing out a session ends its refresh token.
	p := e.loginPair(t, "alice")
	if err := e.svc.RevokeToken(ctx, alice.ID, p.Session.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := e.svc.Refresh(ctx, p.RefreshToken, domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("refresh after sign-out = %v", err)
	}

	// Revoking other sessions keeps the current login's refresh token.
	keep := e.loginPair(t, "alice")
	other := e.loginPair(t, "alice")
	if n, err := e.svc.RevokeOtherSessions(ctx, alice.ID, HashToken(keep.AccessToken)); err != nil || n != 1 {
		t.Fatalf("RevokeOtherSessions = %d, %v", n, err)
	}
	if _, err := e.svc.Refresh(ctx, other.RefreshToken, domain.SessionMeta{}); err == nil {
		t.Fatal("the other login's refresh token must be revoked")
	}
	kept, err := e.svc.Refresh(ctx, keep.RefreshToken, domain.SessionMeta{})
	if err != nil {
		t.Fatalf("current login: %v", err)
	}

	// Disabled accounts cannot refresh.
	a, _ := e.store.GetAccount(ctx, alice.ID)
	a.Disabled = true
	if err := e.store.UpdateAccount(ctx, a); err != nil {
		t.Fatal(err)
	}
	if _, err := e.svc.Refresh(ctx, kept.RefreshToken, domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("disabled account = %v", err)
	}
}

func TestRefreshKeepsMFAAndScopes(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	pair, err := e.svc.issuePair(ctx, alice, true, DeviceSessionPrefix+"laptop", []string{"tree:read"}, domain.SessionMeta{})
	if err != nil {
		t.Fatal(err)
	}
	next, err := e.svc.Refresh(ctx, pair.RefreshToken, domain.SessionMeta{})
	if err != nil {
		t.Fatal(err)
	}
	p := e.authOK(t, next.AccessToken)
	if !p.MFA || len(p.Scopes) != 1 || p.Scopes[0] != "tree:read" || next.Session.Name != "device: laptop" {
		t.Fatalf("principal %+v session %+v", p, next.Session)
	}
}
