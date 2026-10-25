package sso

import (
	"context"
	"errors"
	"net/url"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func wantErr(t *testing.T, err error, kind error, substr string) {
	t.Helper()
	if err == nil {
		t.Fatalf("expected error (%v, %q), got nil", kind, substr)
	}
	if kind != nil && !errors.Is(err, kind) {
		t.Fatalf("error %q does not wrap %v", err, kind)
	}
	if substr != "" && !strings.Contains(err.Error(), substr) {
		t.Fatalf("error %q does not contain %q", err, substr)
	}
}

func TestHappyPathAutoCreateThenLinkedLogin(t *testing.T) {
	h := newHarness(t)
	claims := h.idp.defaultClaims("u-123")
	claims["preferred_username"] = "Jane.Doe"
	claims["email"] = "jane@example.com"

	token, acct, redirect, err := h.login(t, claims)
	if err != nil {
		t.Fatalf("login: %v", err)
	}
	if acct.Username != "jane-doe" || acct.Email != "jane@example.com" || acct.IsAdmin {
		t.Fatalf("account = %+v", acct)
	}
	if acct.DisplayName != "User u-123" {
		t.Fatalf("display name = %q", acct.DisplayName)
	}
	if token != "sess-"+acct.ID || redirect != "/admin/trees?x=1" {
		t.Fatalf("token %q redirect %q", token, redirect)
	}
	if h.idp.lastAuth != "basic" {
		t.Fatalf("client auth = %q, want basic (discovery default)", h.idp.lastAuth)
	}
	if pw := h.creator.passwords["jane-doe"]; len(pw) < 40 {
		t.Fatalf("auto-created password too weak: %d chars", len(pw))
	}
	link, err := h.store.GetSSOIdentity(context.Background(), h.idp.URL(), "u-123")
	if err != nil || link.AccountID != acct.ID || link.LastLoginAt.IsZero() || link.Email != "jane@example.com" {
		t.Fatalf("identity link = %+v, %v", link, err)
	}
	if h.sec.metas[0].RemoteIP != "192.0.2.1" || h.sec.metas[0].UserAgent != "test" {
		t.Fatalf("session meta = %+v", h.sec.metas[0])
	}
	first := link.LastLoginAt

	// Second login: the existing link wins even though the email changed.
	h.clock.advance(time.Second)
	claims2 := h.idp.defaultClaims("u-123")
	claims2["email"] = "jane.new@example.com"
	_, acct2, _, err := h.login(t, claims2)
	if err != nil {
		t.Fatalf("second login: %v", err)
	}
	if acct2.ID != acct.ID || h.store.accountCount() != 1 {
		t.Fatalf("second login mapped to %+v (accounts: %d)", acct2, h.store.accountCount())
	}
	link, _ = h.store.GetSSOIdentity(context.Background(), h.idp.URL(), "u-123")
	if !link.LastLoginAt.After(first) || link.Email != "jane.new@example.com" {
		t.Fatalf("last login not updated: %+v", link)
	}
	if h.idp.discHits != 1 || h.idp.jwksHits != 1 {
		t.Fatalf("discovery/jwks not cached: %d/%d", h.idp.discHits, h.idp.jwksHits)
	}
}

func TestClientAuthMethods(t *testing.T) {
	t.Run("client_secret_post", func(t *testing.T) {
		h := newHarness(t)
		h.idp.authMethods = []string{"client_secret_post"}
		if _, _, _, err := h.login(t, h.idp.defaultClaims("a1")); err != nil {
			t.Fatal(err)
		}
		if h.idp.lastAuth != "post" {
			t.Fatalf("auth = %q", h.idp.lastAuth)
		}
	})
	t.Run("public client", func(t *testing.T) {
		h := newHarness(t)
		h.idp.secret = ""
		empty := ""
		cfg, _ := h.svc.Config(context.Background())
		if _, err := h.svc.SaveConfig(context.Background(), cfg, &empty); err != nil {
			t.Fatal(err)
		}
		if _, _, _, err := h.login(t, h.idp.defaultClaims("a2")); err != nil {
			t.Fatal(err)
		}
		if h.idp.lastAuth != "none" {
			t.Fatalf("auth = %q", h.idp.lastAuth)
		}
	})
	t.Run("wrong secret", func(t *testing.T) {
		h := newHarness(t)
		h.idp.secret = "different"
		_, _, _, err := h.login(t, h.idp.defaultClaims("a3"))
		wantErr(t, err, domain.ErrUnauthenticated, "invalid_client")
	})
}

func TestES256(t *testing.T) {
	h := newHarness(t)
	h.idp.signAlg, h.idp.signKid, h.idp.signKey = "ES256", "key-ec", ecKey
	if _, _, _, err := h.login(t, h.idp.defaultClaims("ec-user")); err != nil {
		t.Fatalf("ES256 login: %v", err)
	}
}

func TestStateFailures(t *testing.T) {
	ctx := context.Background()
	meta := domain.SessionMeta{}

	t.Run("unknown state", func(t *testing.T) {
		h := newHarness(t)
		_, _, _, err := h.svc.Callback(ctx, "code", "nope", meta)
		wantErr(t, err, domain.ErrUnauthenticated, "invalid or has expired")
	})
	t.Run("empty state", func(t *testing.T) {
		h := newHarness(t)
		_, _, _, err := h.svc.Callback(ctx, "code", "", meta)
		wantErr(t, err, domain.ErrUnauthenticated, "")
	})
	t.Run("state is single use", func(t *testing.T) {
		h := newHarness(t)
		authURL, _ := h.svc.AuthURL(ctx, "")
		code, state := h.idp.authorize(t, authURL, h.idp.defaultClaims("s1"))
		if _, _, redirect, err := h.svc.Callback(ctx, code, state, meta); err != nil || redirect != DefaultRedirect {
			t.Fatalf("first callback: %v (redirect %q)", err, redirect)
		}
		code2, _ := h.idp.authorize(t, authURL, h.idp.defaultClaims("s1"))
		_, _, _, err := h.svc.Callback(ctx, code2, state, meta)
		wantErr(t, err, domain.ErrUnauthenticated, "invalid or has expired")
	})
	t.Run("state expires", func(t *testing.T) {
		h := newHarness(t)
		authURL, _ := h.svc.AuthURL(ctx, "")
		code, state := h.idp.authorize(t, authURL, h.idp.defaultClaims("s2"))
		h.clock.advance(11 * time.Minute)
		_, _, _, err := h.svc.Callback(ctx, code, state, meta)
		wantErr(t, err, domain.ErrUnauthenticated, "invalid or has expired")
		if h.idp.tokenHits != 0 {
			t.Fatal("token endpoint called for an expired state")
		}
	})
	t.Run("missing code consumes state", func(t *testing.T) {
		h := newHarness(t)
		authURL, _ := h.svc.AuthURL(ctx, "")
		_, state := h.idp.authorize(t, authURL, nil)
		_, _, _, err := h.svc.Callback(ctx, "", state, meta)
		wantErr(t, err, domain.ErrInvalid, "authorization code")
		_, _, _, err = h.svc.Callback(ctx, "x", state, meta)
		wantErr(t, err, domain.ErrUnauthenticated, "invalid or has expired")
	})
	t.Run("config changed mid-flight", func(t *testing.T) {
		h := newHarness(t)
		authURL, _ := h.svc.AuthURL(ctx, "")
		code, state := h.idp.authorize(t, authURL, h.idp.defaultClaims("s3"))
		cfg, _ := h.svc.Config(ctx)
		cfg.ClientID = "other-client"
		if _, err := h.svc.SaveConfig(ctx, cfg, nil); err != nil {
			t.Fatal(err)
		}
		_, _, _, err := h.svc.Callback(ctx, code, state, meta)
		wantErr(t, err, domain.ErrUnauthenticated, "configuration changed")
	})
	t.Run("pending table is bounded", func(t *testing.T) {
		h := newHarness(t, func(_ *domain.SSOConfig, o *Options) { o.MaxPending = 2 })
		u1, _ := h.svc.AuthURL(ctx, "")
		h.clock.advance(time.Millisecond)
		u2, _ := h.svc.AuthURL(ctx, "")
		h.clock.advance(time.Millisecond)
		u3, _ := h.svc.AuthURL(ctx, "")
		state := func(u string) string { p, _ := url.Parse(u); return p.Query().Get("state") }
		if h.svc.takePending(state(u1)) != nil {
			t.Fatal("oldest pending login was not evicted")
		}
		if h.svc.takePending(state(u2)) == nil || h.svc.takePending(state(u3)) == nil {
			t.Fatal("newer pending logins were evicted")
		}
	})
}

func TestAuthURLParameters(t *testing.T) {
	h := newHarness(t)
	ctx := context.Background()
	a, err := h.svc.AuthURL(ctx, "/admin/")
	if err != nil {
		t.Fatal(err)
	}
	b, _ := h.svc.AuthURL(ctx, "/admin/")
	ua, _ := url.Parse(a)
	ub, _ := url.Parse(b)
	qa, qb := ua.Query(), ub.Query()
	if !strings.HasPrefix(a, h.idp.URL()+"/authorize?") {
		t.Fatalf("auth URL %q", a)
	}
	if qa.Get("scope") != "openid email profile" || qa.Get("redirect_uri") != "http://localhost:7420"+CallbackPath {
		t.Fatalf("query = %v", qa)
	}
	for _, k := range []string{"state", "nonce", "code_challenge"} {
		if len(qa.Get(k)) != 43 || qa.Get(k) == qb.Get(k) {
			t.Fatalf("%s not random 256-bit: %q / %q", k, qa.Get(k), qb.Get(k))
		}
	}
	if _, err := h.svc.AuthURL(ctx, "https://evil.example/admin/"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("absolute redirect accepted: %v", err)
	}
}

func TestPKCEAndNonce(t *testing.T) {
	ctx := context.Background()
	t.Run("PKCE verifier mismatch", func(t *testing.T) {
		h := newHarness(t)
		authURL, _ := h.svc.AuthURL(ctx, "")
		code, state := h.idp.authorize(t, authURL, h.idp.defaultClaims("p1"))
		h.idp.codes[code].challenge = "tampered-challenge"
		_, _, _, err := h.svc.Callback(ctx, code, state, domain.SessionMeta{})
		wantErr(t, err, domain.ErrUnauthenticated, "invalid_grant")
	})
	t.Run("nonce mismatch", func(t *testing.T) {
		h := newHarness(t)
		c := h.idp.defaultClaims("p2")
		c["nonce"] = "attacker-nonce"
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrUnauthenticated, "nonce mismatch")
	})
	t.Run("nonce missing", func(t *testing.T) {
		h := newHarness(t)
		c := h.idp.defaultClaims("p3")
		c["nonce"] = ""
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrUnauthenticated, "nonce mismatch")
	})
}

func TestIDTokenValidation(t *testing.T) {
	now := time.Now()
	cases := []struct {
		name   string
		mutate func(h *harness, c map[string]any)
		want   string
	}{
		{"expired", func(_ *harness, c map[string]any) { c["exp"] = now.Add(-10 * time.Minute).Unix() }, "expired"},
		{"within leeway", nil, ""},
		{"missing exp", func(_ *harness, c map[string]any) { delete(c, "exp") }, "missing exp"},
		{"missing iat", func(_ *harness, c map[string]any) { delete(c, "iat") }, "missing iat"},
		{"iat in future", func(_ *harness, c map[string]any) { c["iat"] = now.Add(time.Hour).Unix() }, "issued in the future"},
		{"nbf in future", func(_ *harness, c map[string]any) { c["nbf"] = now.Add(time.Hour).Unix() }, "not yet valid"},
		{"wrong issuer", func(_ *harness, c map[string]any) { c["iss"] = "https://evil.example" }, "wrong issuer"},
		{"wrong audience", func(_ *harness, c map[string]any) { c["aud"] = "other-client" }, "wrong audience"},
		{"audience list ok with azp", func(h *harness, c map[string]any) {
			c["aud"] = []any{h.idp.clientID, "other"}
			c["azp"] = h.idp.clientID
		}, ""},
		{"audience list without azp", func(h *harness, c map[string]any) { c["aud"] = []any{h.idp.clientID, "other"} }, "wrong authorized party"},
		{"wrong azp", func(_ *harness, c map[string]any) { c["azp"] = "other" }, "wrong authorized party"},
		{"missing sub", func(_ *harness, c map[string]any) { delete(c, "sub") }, "missing subject"},
		{"bad signature", func(h *harness, _ map[string]any) { h.idp.signKey = rsaKeyB }, "bad signature"},
		{"unknown kid", func(h *harness, _ map[string]any) { h.idp.signKid = "nope" }, "unknown signing key"},
		{"alg none", func(h *harness, _ map[string]any) { h.idp.signAlg = "none" }, "unsupported signing algorithm"},
		{"alg HS256", func(h *harness, _ map[string]any) { h.idp.signAlg = "HS256" }, "unsupported signing algorithm"},
		{"alg/key confusion", func(h *harness, _ map[string]any) {
			h.idp.signAlg, h.idp.signKid, h.idp.signKey = "ES256", "key-a", ecKey
		}, "unknown signing key"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			h := newHarness(t)
			c := h.idp.defaultClaims("tok-user")
			if tc.name == "within leeway" {
				c["exp"] = time.Now().Add(-time.Minute).Unix() // skew is 2m
			}
			if tc.mutate != nil {
				tc.mutate(h, c)
			}
			_, acct, _, err := h.login(t, c)
			if tc.want == "" {
				if err != nil || acct == nil {
					t.Fatalf("login failed: %v", err)
				}
				return
			}
			wantErr(t, err, domain.ErrUnauthenticated, tc.want)
			if h.store.accountCount() != 0 || len(h.sec.sessions) != 0 {
				t.Fatal("rejected token still created an account or session")
			}
		})
	}
}

func TestMalformedTokenResponses(t *testing.T) {
	cases := []struct {
		name, body, want string
		status           int
	}{
		{"no id_token", `{"access_token":"x","token_type":"Bearer"}`, "did not return an ID token", 200},
		{"garbage id_token", `{"id_token":"a.b"}`, "malformed", 200},
		{"not json", `<html>`, "invalid token response", 200},
		{"server error", `oops`, "HTTP 500", 500},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			h := newHarness(t)
			h.idp.tokenBody, h.idp.tokenStatus = tc.body, tc.status
			_, _, _, err := h.login(t, nil)
			wantErr(t, err, domain.ErrUnauthenticated, tc.want)
		})
	}
}

func TestJWKSRotation(t *testing.T) {
	h := newHarness(t, func(_ *domain.SSOConfig, o *Options) { o.JWKSMinFetch = time.Nanosecond })
	if _, _, _, err := h.login(t, h.idp.defaultClaims("r1")); err != nil {
		t.Fatal(err)
	}
	// The provider rotates to a new key; the unknown kid forces a refetch.
	h.idp.mu.Lock()
	h.idp.jwks = []map[string]any{rsaJWK("key-b", &rsaKeyB.PublicKey)}
	h.idp.signKid, h.idp.signKey = "key-b", rsaKeyB
	h.idp.mu.Unlock()
	if _, _, _, err := h.login(t, h.idp.defaultClaims("r1")); err != nil {
		t.Fatalf("login after rotation: %v", err)
	}
	if h.idp.jwksHits != 2 {
		t.Fatalf("jwks hits = %d, want 2", h.idp.jwksHits)
	}
}

func TestJWKSRefreshIsRateLimited(t *testing.T) {
	h := newHarness(t) // JWKSMinFetch default 1m
	if _, _, _, err := h.login(t, h.idp.defaultClaims("r2")); err != nil {
		t.Fatal(err)
	}
	h.idp.signKid = "unknown"
	for i := 0; i < 3; i++ {
		_, _, _, err := h.login(t, h.idp.defaultClaims("r2"))
		wantErr(t, err, domain.ErrUnauthenticated, "unknown signing key")
	}
	if h.idp.jwksHits != 1 {
		t.Fatalf("jwks hits = %d, want 1 (refresh must be rate-limited)", h.idp.jwksHits)
	}
}

func TestDiscoveryIssuerMismatch(t *testing.T) {
	h := newHarness(t)
	h.idp.issuer = "https://impostor.example"
	_, err := h.svc.AuthURL(context.Background(), "")
	wantErr(t, err, domain.ErrUnauthenticated, "could not be reached")
}

func TestDiscoveryTTL(t *testing.T) {
	h := newHarness(t)
	for i := 0; i < 2; i++ {
		if _, err := h.svc.AuthURL(context.Background(), ""); err != nil {
			t.Fatal(err)
		}
	}
	h.clock.advance(2 * time.Hour)
	if _, err := h.svc.AuthURL(context.Background(), ""); err != nil {
		t.Fatal(err)
	}
	if h.idp.discHits != 2 {
		t.Fatalf("discovery hits = %d, want 2", h.idp.discHits)
	}
}

func TestDomainRestrictions(t *testing.T) {
	restrict := func(c *domain.SSOConfig, _ *Options) { c.AllowedDomains = []string{"Example.com", "@corp.example"} }
	t.Run("allowed", func(t *testing.T) {
		h := newHarness(t, restrict)
		c := h.idp.defaultClaims("d1")
		c["email"] = "d1@corp.example"
		if _, _, _, err := h.login(t, c); err != nil {
			t.Fatal(err)
		}
	})
	t.Run("other domain", func(t *testing.T) {
		h := newHarness(t, restrict)
		c := h.idp.defaultClaims("d2")
		c["email"] = "d2@evil.example"
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrForbidden, "evil.example")
	})
	t.Run("subdomain is not the domain", func(t *testing.T) {
		h := newHarness(t, restrict)
		c := h.idp.defaultClaims("d3")
		c["email"] = "d3@sub.example.com"
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrForbidden, "")
	})
	t.Run("unverified email", func(t *testing.T) {
		h := newHarness(t, restrict)
		c := h.idp.defaultClaims("d4")
		c["email_verified"] = false
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrForbidden, "verified email")
	})
	t.Run("applies to linked identities", func(t *testing.T) {
		h := newHarness(t)
		if _, _, _, err := h.login(t, h.idp.defaultClaims("d5")); err != nil {
			t.Fatal(err)
		}
		cfg, _ := h.svc.Config(context.Background())
		cfg.AllowedDomains = []string{"corp.example"}
		if _, err := h.svc.SaveConfig(context.Background(), cfg, nil); err != nil {
			t.Fatal(err)
		}
		_, _, _, err := h.login(t, h.idp.defaultClaims("d5"))
		wantErr(t, err, domain.ErrForbidden, "")
	})
	t.Run("string email_verified", func(t *testing.T) {
		h := newHarness(t, restrict)
		c := h.idp.defaultClaims("d6")
		c["email_verified"] = "true"
		if _, _, _, err := h.login(t, c); err != nil {
			t.Fatal(err)
		}
	})
}

func TestLinkByVerifiedEmail(t *testing.T) {
	t.Run("links existing account", func(t *testing.T) {
		h := newHarness(t)
		alice := h.store.addAccount("alice", "alice@example.com", false)
		c := h.idp.defaultClaims("idp-alice")
		c["email"] = "Alice@Example.com"
		_, acct, _, err := h.login(t, c)
		if err != nil {
			t.Fatal(err)
		}
		if acct.ID != alice.ID || h.store.accountCount() != 1 {
			t.Fatalf("mapped to %+v", acct)
		}
		if link, err := h.store.GetSSOIdentity(context.Background(), h.idp.URL(), "idp-alice"); err != nil || link.AccountID != alice.ID {
			t.Fatalf("link = %+v, %v", link, err)
		}
	})
	t.Run("unverified email never links", func(t *testing.T) {
		h := newHarness(t)
		h.store.addAccount("alice", "alice@example.com", false)
		c := h.idp.defaultClaims("idp-alice")
		c["email"] = "alice@example.com"
		c["email_verified"] = false
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrConflict, "not verified")
		if h.store.accountCount() != 1 {
			t.Fatal("duplicate account created")
		}
	})
	t.Run("disabled account", func(t *testing.T) {
		h := newHarness(t)
		h.store.addAccount("alice", "alice@example.com", true)
		c := h.idp.defaultClaims("idp-alice")
		c["email"] = "alice@example.com"
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrForbidden, "disabled")
	})
	t.Run("account already linked to another subject", func(t *testing.T) {
		h := newHarness(t)
		alice := h.store.addAccount("alice", "alice@example.com", false)
		_ = h.store.PutSSOIdentity(context.Background(), &domain.SSOIdentity{Issuer: h.idp.URL(), Subject: "first", AccountID: alice.ID})
		c := h.idp.defaultClaims("second")
		c["email"] = "alice@example.com"
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrConflict, "different identity")
	})
	t.Run("disabled linked account", func(t *testing.T) {
		h := newHarness(t)
		_, acct, _, err := h.login(t, h.idp.defaultClaims("dl"))
		if err != nil {
			t.Fatal(err)
		}
		acct.Disabled = true
		_ = h.store.UpdateAccount(context.Background(), acct)
		_, _, _, err = h.login(t, h.idp.defaultClaims("dl"))
		wantErr(t, err, domain.ErrForbidden, "disabled")
		if len(h.sec.sessions) != 1 {
			t.Fatal("session issued for a disabled account")
		}
	})
}

func TestAutoCreate(t *testing.T) {
	t.Run("username collisions get suffixes", func(t *testing.T) {
		h := newHarness(t)
		h.store.addAccount("jane", "other-jane@example.com", false)
		h.store.addAccount("jane-2", "jane2@example.com", false)
		c := h.idp.defaultClaims("j1")
		c["preferred_username"] = "jane"
		c["email"] = "jane@corp.example"
		_, acct, _, err := h.login(t, c)
		if err != nil {
			t.Fatal(err)
		}
		if acct.Username != "jane-3" {
			t.Fatalf("username = %q, want jane-3", acct.Username)
		}
	})
	t.Run("reserved name", func(t *testing.T) {
		h := newHarness(t)
		c := h.idp.defaultClaims("j2")
		c["preferred_username"] = "Admin"
		_, acct, _, err := h.login(t, c)
		if err != nil {
			t.Fatal(err)
		}
		if acct.Username != "admin-2" || acct.IsAdmin {
			t.Fatalf("account = %+v", acct)
		}
	})
	t.Run("custom username claim then email local part", func(t *testing.T) {
		h := newHarness(t, func(c *domain.SSOConfig, _ *Options) { c.UsernameClaim = "nickname" })
		c := h.idp.defaultClaims("j3")
		c["nickname"] = "Nick_Name"
		_, acct, _, err := h.login(t, c)
		if err != nil || acct.Username != "nick-name" {
			t.Fatalf("account = %+v, %v", acct, err)
		}
		c = h.idp.defaultClaims("j4")
		delete(c, "preferred_username")
		c["email"] = "first.last+tag@example.com"
		_, acct, _, err = h.login(t, c)
		if err != nil || acct.Username != "first-last-tag" {
			t.Fatalf("account = %+v, %v", acct, err)
		}
	})
	t.Run("no usable name falls back to user", func(t *testing.T) {
		h := newHarness(t)
		c := h.idp.defaultClaims("j5")
		c["preferred_username"] = "株式"
		c["email"] = "x@example.com"
		_, acct, _, err := h.login(t, c)
		if err != nil || acct.Username != "user" {
			t.Fatalf("account = %+v, %v", acct, err)
		}
	})
	t.Run("disabled", func(t *testing.T) {
		h := newHarness(t, func(c *domain.SSOConfig, _ *Options) { c.AutoCreate = false })
		_, _, _, err := h.login(t, h.idp.defaultClaims("j6"))
		wantErr(t, err, domain.ErrForbidden, "no account is linked")
		if h.store.accountCount() != 0 {
			t.Fatal("account created with auto_create off")
		}
	})
	t.Run("requires email", func(t *testing.T) {
		h := newHarness(t)
		c := h.idp.defaultClaims("j7")
		delete(c, "email")
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrForbidden, "email")
	})
}

func TestUserinfoFallback(t *testing.T) {
	t.Run("fills missing email", func(t *testing.T) {
		h := newHarness(t)
		h.idp.userinfo = map[string]any{"sub": "ui1", "email": "ui1@corp.example", "email_verified": true}
		c := h.idp.defaultClaims("ui1")
		delete(c, "email")
		delete(c, "email_verified")
		_, acct, _, err := h.login(t, c)
		if err != nil || acct.Email != "ui1@corp.example" || h.idp.uiHits != 1 {
			t.Fatalf("account = %+v, %v (userinfo hits %d)", acct, err, h.idp.uiHits)
		}
	})
	t.Run("subject mismatch ignored", func(t *testing.T) {
		h := newHarness(t)
		h.idp.userinfo = map[string]any{"sub": "someone-else", "email": "victim@example.com", "email_verified": true}
		h.store.addAccount("victim", "victim@example.com", false)
		c := h.idp.defaultClaims("ui2")
		delete(c, "email")
		_, _, _, err := h.login(t, c)
		wantErr(t, err, domain.ErrForbidden, "email")
	})
	t.Run("not called when the ID token has an email", func(t *testing.T) {
		h := newHarness(t)
		h.idp.userinfo = map[string]any{"sub": "ui3"}
		if _, _, _, err := h.login(t, h.idp.defaultClaims("ui3")); err != nil {
			t.Fatal(err)
		}
		if h.idp.uiHits != 0 {
			t.Fatal("userinfo called unnecessarily")
		}
	})
}

func TestIssueSessionFailure(t *testing.T) {
	h := newHarness(t)
	h.sec.fail = errors.New("boom")
	_, _, _, err := h.login(t, h.idp.defaultClaims("is1"))
	if err == nil || errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("err = %v", err)
	}
}

func TestDisabledProvider(t *testing.T) {
	ctx := context.Background()
	svc := New(newMemStore(), &fakeSecurity{}, &fakeCreator{}, Options{Logger: quietLogger()})
	if _, err := svc.AuthURL(ctx, ""); !errors.Is(err, ErrNotEnabled) || !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("AuthURL on unset config: %v", err)
	}
	cfg, err := svc.Config(ctx)
	if err != nil || cfg.Enabled || cfg.UsernameClaim != DefaultUsernameClaim || len(cfg.Scopes) != 3 || cfg.AllowedDomains == nil {
		t.Fatalf("zero config = %+v, %v", cfg, err)
	}
}

func TestConfigSecretIsWriteOnly(t *testing.T) {
	h := newHarness(t)
	ctx := context.Background()
	cfg, err := h.svc.Config(ctx)
	if err != nil || !cfg.HasSecret || cfg.ClientSecret != "" {
		t.Fatalf("config = %+v, %v", cfg, err)
	}
	// nil keeps the secret.
	cfg.Name = "Renamed"
	out, err := h.svc.SaveConfig(ctx, cfg, nil)
	if err != nil || !out.HasSecret || out.ClientSecret != "" || h.store.cfg.ClientSecret != h.idp.secret {
		t.Fatalf("save keep: %+v, %v", out, err)
	}
	// "" clears it.
	empty := ""
	out, err = h.svc.SaveConfig(ctx, cfg, &empty)
	if err != nil || out.HasSecret || h.store.cfg.ClientSecret != "" {
		t.Fatalf("save clear: %+v, %v", out, err)
	}
}

func TestSaveConfigValidation(t *testing.T) {
	base := func() *domain.SSOConfig {
		return &domain.SSOConfig{Enabled: true, Issuer: "https://idp.example", ClientID: "c", RedirectURL: "https://wt.example" + CallbackPath}
	}
	cases := []struct {
		name   string
		mutate func(c *domain.SSOConfig)
		opts   Options
		want   string // "" = valid
		check  func(t *testing.T, c *domain.SSOConfig)
	}{
		{"valid defaults", nil, Options{}, "", func(t *testing.T, c *domain.SSOConfig) {
			if c.Name != DefaultName || strings.Join(c.Scopes, " ") != "openid email profile" || c.UsernameClaim != DefaultUsernameClaim || c.UpdatedAt.IsZero() {
				t.Fatalf("%+v", c)
			}
		}},
		{"http issuer", func(c *domain.SSOConfig) { c.Issuer = "http://idp.example" }, Options{}, "https", nil},
		{"http localhost issuer", func(c *domain.SSOConfig) { c.Issuer = "http://localhost:8080/realms/x" }, Options{}, "", nil},
		{"http loopback issuer", func(c *domain.SSOConfig) { c.Issuer = "http://127.0.0.1:8080" }, Options{}, "", nil},
		{"issuer with query", func(c *domain.SSOConfig) { c.Issuer = "https://idp.example/?a=1" }, Options{}, "query", nil},
		{"issuer with credentials", func(c *domain.SSOConfig) { c.Issuer = "https://u:p@idp.example" }, Options{}, "credentials", nil},
		{"issuer not a URL", func(c *domain.SSOConfig) { c.Issuer = "idp.example" }, Options{}, "absolute URL", nil},
		{"ftp issuer", func(c *domain.SSOConfig) { c.Issuer = "ftp://idp.example" }, Options{}, "http(s)", nil},
		{"enabled without issuer", func(c *domain.SSOConfig) { c.Issuer = "" }, Options{}, "issuer is required", nil},
		{"disabled without issuer", func(c *domain.SSOConfig) { c.Enabled = false; c.Issuer = ""; c.ClientID = "" }, Options{}, "", nil},
		{"enabled without client id", func(c *domain.SSOConfig) { c.ClientID = "" }, Options{}, "client_id is required", nil},
		{"client id with space", func(c *domain.SSOConfig) { c.ClientID = "a b" }, Options{}, "client_id", nil},
		{"redirect http", func(c *domain.SSOConfig) { c.RedirectURL = "http://wt.example/cb" }, Options{}, "https", nil},
		{"redirect with fragment", func(c *domain.SSOConfig) { c.RedirectURL = "https://wt.example/cb#x" }, Options{}, "fragment", nil},
		{"redirect missing", func(c *domain.SSOConfig) { c.RedirectURL = "" }, Options{}, "redirect_url is required", nil},
		{"redirect from public URL", func(c *domain.SSOConfig) { c.RedirectURL = "" }, Options{PublicURL: "https://wt.example/"}, "", func(t *testing.T, c *domain.SSOConfig) {
			if c.RedirectURL != "https://wt.example"+CallbackPath {
				t.Fatalf("redirect = %q", c.RedirectURL)
			}
		}},
		{"scopes without openid", func(c *domain.SSOConfig) { c.Scopes = []string{"email"} }, Options{}, "openid", nil},
		{"scope with quote", func(c *domain.SSOConfig) { c.Scopes = []string{"openid", `a"b`} }, Options{}, "scope", nil},
		{"scopes deduplicated", func(c *domain.SSOConfig) { c.Scopes = []string{"openid", " openid", "groups"} }, Options{}, "", func(t *testing.T, c *domain.SSOConfig) {
			if strings.Join(c.Scopes, ",") != "openid,groups" {
				t.Fatalf("scopes = %v", c.Scopes)
			}
		}},
		{"bad domain", func(c *domain.SSOConfig) { c.AllowedDomains = []string{"not a domain"} }, Options{}, "allowed domain", nil},
		{"domains normalized", func(c *domain.SSOConfig) { c.AllowedDomains = []string{"@Corp.Example", "corp.example"} }, Options{}, "", func(t *testing.T, c *domain.SSOConfig) {
			if strings.Join(c.AllowedDomains, ",") != "corp.example" {
				t.Fatalf("domains = %v", c.AllowedDomains)
			}
		}},
		{"bad username claim", func(c *domain.SSOConfig) { c.UsernameClaim = "a b" }, Options{}, "username_claim", nil},
		{"long name", func(c *domain.SSOConfig) { c.Name = strings.Repeat("x", 101) }, Options{}, "name", nil},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			tc.opts.Logger = quietLogger()
			svc := New(newMemStore(), &fakeSecurity{}, &fakeCreator{}, tc.opts)
			c := base()
			if tc.mutate != nil {
				tc.mutate(c)
			}
			out, err := svc.SaveConfig(context.Background(), c, nil)
			if tc.want != "" {
				wantErr(t, err, domain.ErrInvalid, tc.want)
				return
			}
			if err != nil {
				t.Fatalf("SaveConfig: %v", err)
			}
			if tc.check != nil {
				tc.check(t, out)
			}
		})
	}
	t.Run("long secret", func(t *testing.T) {
		svc := New(newMemStore(), &fakeSecurity{}, &fakeCreator{}, Options{Logger: quietLogger()})
		long := strings.Repeat("s", 2000)
		_, err := svc.SaveConfig(context.Background(), base(), &long)
		wantErr(t, err, domain.ErrInvalid, "client_secret")
	})
	t.Run("store failure is not a validation error", func(t *testing.T) {
		st := newMemStore()
		st.failGet = errors.New("disk on fire")
		svc := New(st, &fakeSecurity{}, &fakeCreator{}, Options{Logger: quietLogger()})
		_, err := svc.SaveConfig(context.Background(), base(), nil)
		if err == nil || errors.Is(err, domain.ErrInvalid) {
			t.Fatalf("err = %v", err)
		}
	})
}

func TestSafeRedirect(t *testing.T) {
	cases := []struct {
		in, want string
		ok       bool
	}{
		{"", DefaultRedirect, true},
		{"/admin/", "/admin/", true},
		{"/admin", "/admin", true},
		{"/admin/trees/acme/web?tab=log", "/admin/trees/acme/web?tab=log", true},
		{"/admin/x#frag", "/admin/x", true},
		{"/administrator", "", false},
		{"/api/v1/trees/a/b/blob/s/x.html", "", false},
		{"//evil.example/admin/", "", false},
		{"/\\evil.example", "", false},
		{"https://evil.example/admin/", "", false},
		{"javascript:alert(1)", "", false},
		{"admin/", "", false},
		{"/admin/../api/v1/x", "", false},
		{"/admin/./x", "", false},
		{"/admin/%2e%2e/api", "", false},
		{"/admin/%2E%2E/api", "", false},
		{"/admin/%2f%2fevil", "", false},
		{"/admin/\tx", "", false},
		{"/admin/ x", "", false},
		{"/admin/" + strings.Repeat("a", 1100), "", false},
	}
	for _, tc := range cases {
		got, ok := SafeRedirect(tc.in)
		if ok != tc.ok || got != tc.want {
			t.Errorf("SafeRedirect(%q) = %q, %v; want %q, %v", tc.in, got, ok, tc.want, tc.ok)
		}
	}
}

func TestSanitizeUsername(t *testing.T) {
	cases := map[string]string{
		"Jane.Doe":                "jane-doe",
		"  bob  ":                 "bob",
		"a":                       "",
		"__x__":                   "", // a single character remains
		"first.last+tag":          "first-last-tag",
		"UPPER_case":              "upper-case",
		"é-accent":                "accent",
		"a..b":                    "a-b",
		strings.Repeat("ab", 30):  strings.Repeat("ab", 19) + "a",
		"user@example.com":        "user-example-com",
		"-leading-and-trailing--": "leading-and-trailing",
	}
	for in, want := range cases {
		if got := SanitizeUsername(in); got != want {
			t.Errorf("SanitizeUsername(%q) = %q, want %q", in, got, want)
		}
	}
	for attempt, want := range map[int]string{0: "abc", 1: "abc-2", 9: "abc-10"} {
		if got, _ := usernameVariant("abc", attempt); got != want {
			t.Errorf("usernameVariant(abc, %d) = %q, want %q", attempt, got, want)
		}
	}
	long := strings.Repeat("x", 39)
	for _, attempt := range []int{0, 5, 15} {
		got, _ := usernameVariant(long, attempt)
		if len(got) > 39 || len(got) < 2 {
			t.Errorf("usernameVariant(long, %d) = %q (len %d)", attempt, got, len(got))
		}
	}
}
