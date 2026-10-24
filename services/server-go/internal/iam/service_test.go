package iam

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestRegister(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a, err := e.svc.Register(ctx, "  Alice ", " alice@example.com ", testPassword)
	if err != nil {
		t.Fatal(err)
	}
	if a.Username != "alice" || a.Email != "alice@example.com" || a.ID == "" || a.IsAdmin || a.Disabled {
		t.Fatalf("unexpected account %+v", a)
	}
	if !strings.HasPrefix(a.PasswordHash, "$argon2id$") {
		t.Fatalf("password not hashed with argon2id: %q", a.PasswordHash)
	}
	ten, err := e.store.GetTenant(ctx, a.TenantID)
	if err != nil {
		t.Fatal(err)
	}
	if ten.Slug != "alice" || ten.Kind != domain.TenantUser || ten.Plan != "free" {
		t.Fatalf("unexpected tenant %+v", ten)
	}
	m, err := e.store.GetMembership(ctx, ten.ID, a.ID)
	if err != nil || m.Role != domain.RoleOwner {
		t.Fatalf("membership = %+v, %v", m, err)
	}
	if role, _ := e.svc.RoleOf(ctx, a.ID, ten.ID); role != domain.RoleOwner {
		t.Fatalf("RoleOf = %q", role)
	}
}

func TestRegisterErrors(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	e.register(t, "alice")
	// An org tenant occupying a slug blocks registering that username.
	if err := e.store.CreateTenant(ctx, &domain.Tenant{ID: "org1", Slug: "acme", Kind: domain.TenantOrg}); err != nil {
		t.Fatal(err)
	}
	tests := []struct {
		name, user, email, pw string
		want                  error
	}{
		{"duplicate username", "alice", "other@example.com", testPassword, domain.ErrAlreadyExists},
		{"duplicate username case", "ALICE", "other@example.com", testPassword, domain.ErrAlreadyExists},
		{"duplicate email", "bob", "Alice@Example.com", testPassword, domain.ErrAlreadyExists},
		{"slug taken by org", "acme", "acme@example.com", testPassword, domain.ErrAlreadyExists},
		{"invalid username", "b", "b@example.com", testPassword, domain.ErrInvalid},
		{"reserved admin", "admin", "admin@example.com", testPassword, domain.ErrInvalid},
		{"reserved api", "api", "api@example.com", testPassword, domain.ErrInvalid},
		{"invalid email", "carol", "carol", testPassword, domain.ErrInvalid},
		{"short password", "carol", "carol@example.com", "short", domain.ErrInvalid},
		{"long password", "carol", "carol@example.com", strings.Repeat("p", MaxPasswordLen+1), domain.ErrInvalid},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := e.svc.Register(ctx, tt.user, tt.email, tt.pw)
			if !errors.Is(err, tt.want) {
				t.Fatalf("Register = %v, want %v", err, tt.want)
			}
		})
	}
}

func TestRegisterDisabled(t *testing.T) {
	e := newTestEnv(t, func(o *Options) { o.AllowRegistration = false })
	_, err := e.svc.Register(context.Background(), "alice", "a@example.com", testPassword)
	if !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("Register = %v, want ErrForbidden", err)
	}
}

func TestRegisterConcurrentSameName(t *testing.T) {
	e := newTestEnv(t, nil)
	var wg sync.WaitGroup
	var mu sync.Mutex
	okCount := 0
	for i := 0; i < 8; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			_, err := e.svc.Register(context.Background(), "racer", "racer@example.com", testPassword)
			if err == nil {
				mu.Lock()
				okCount++
				mu.Unlock()
			} else if !errors.Is(err, domain.ErrAlreadyExists) {
				t.Errorf("unexpected error: %v", err)
			}
		}(i)
	}
	wg.Wait()
	if okCount != 1 {
		t.Fatalf("%d registrations succeeded, want 1", okCount)
	}
	if n := len(e.store.tenants); n != 1 {
		t.Fatalf("%d tenants created, want 1", n)
	}
}

func TestLogin(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	disabled := e.register(t, "dora")
	disabled.Disabled = true
	if err := e.store.UpdateAccount(ctx, disabled); err != nil {
		t.Fatal(err)
	}
	tests := []struct {
		name, user, pw string
		want           error
	}{
		{"ok", "alice", testPassword, nil},
		{"ok case-insensitive username", "ALICE", testPassword, nil},
		{"wrong password", "alice", "wrong password", domain.ErrUnauthenticated},
		{"unknown user", "nobody", testPassword, domain.ErrUnauthenticated},
		{"invalid username", "x", testPassword, domain.ErrUnauthenticated},
		{"oversized password", "alice", strings.Repeat("p", MaxPasswordLen+1), domain.ErrUnauthenticated},
		{"disabled account", "dora", testPassword, domain.ErrForbidden},
		{"disabled account wrong password", "dora", "nope nope", domain.ErrUnauthenticated},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			tok, acct, err := e.svc.Login(ctx, tt.user, tt.pw)
			if !errors.Is(err, tt.want) {
				t.Fatalf("Login = %v, want %v", err, tt.want)
			}
			if tt.want != nil {
				if tok != "" || acct != nil {
					t.Fatal("failed login returned credentials")
				}
				return
			}
			if !wellFormedToken(tok) || acct.ID != alice.ID {
				t.Fatalf("Login returned %q, %+v", tok, acct)
			}
			p, err := e.svc.Authenticate(ctx, tok)
			if err != nil || p.AccountID != alice.ID || p.Username != "alice" || p.TenantID != alice.TenantID {
				t.Fatalf("Authenticate = %+v, %v", p, err)
			}
			stored, err := e.store.GetTokenByHash(ctx, HashToken(tok))
			if err != nil {
				t.Fatal(err)
			}
			if stored.Name != "session" || stored.ExpiresAt == nil ||
				!stored.ExpiresAt.Equal(e.clock.Now().Add(DefaultSessionTTL)) {
				t.Fatalf("unexpected session token %+v", stored)
			}
			if stored.TokenHash == tok || strings.Contains(stored.TokenHash, tok[3:]) {
				t.Fatal("raw token stored")
			}
		})
	}
}

func TestLoginGenericError(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	e.register(t, "alice")
	_, _, errBadPw := e.svc.Login(ctx, "alice", "wrong password")
	_, _, errNoUser := e.svc.Login(ctx, "nobody", "wrong password")
	if errBadPw == nil || errNoUser == nil || errBadPw.Error() != errNoUser.Error() {
		t.Fatalf("errors differ: %q vs %q", errBadPw, errNoUser)
	}
}

func TestLoginPasswordlessAccount(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	a.PasswordHash = ""
	if err := e.store.UpdateAccount(ctx, a); err != nil {
		t.Fatal(err)
	}
	for _, pw := range []string{"", testPassword, "\x00iam-dummy-password\x00"} {
		if _, _, err := e.svc.Login(ctx, "alice", pw); !errors.Is(err, domain.ErrUnauthenticated) {
			t.Fatalf("Login(%q) on passwordless account = %v", pw, err)
		}
	}
}

func TestLoginSessionTTLOption(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, func(o *Options) { o.SessionTTL = time.Hour })
	e.register(t, "alice")
	tok, _, err := e.svc.Login(ctx, "alice", testPassword)
	if err != nil {
		t.Fatal(err)
	}
	e.clock.Advance(59 * time.Minute)
	if _, err := e.svc.Authenticate(ctx, tok); err != nil {
		t.Fatalf("token expired early: %v", err)
	}
	e.clock.Advance(time.Minute)
	if _, err := e.svc.Authenticate(ctx, tok); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("Authenticate after TTL = %v", err)
	}
}

func TestLoginUpgradesHashAndPrunesExpired(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	// Store a hash with weaker parameters than the service's.
	weak, err := HashPassword(testPassword, PasswordParams{Memory: 32, Time: 1, Threads: 1})
	if err != nil {
		t.Fatal(err)
	}
	a.PasswordHash = weak
	if err := e.store.UpdateAccount(ctx, a); err != nil {
		t.Fatal(err)
	}
	_, short, err := e.svc.CreateToken(ctx, a.ID, "short", time.Minute)
	if err != nil {
		t.Fatal(err)
	}
	e.clock.Advance(2 * time.Minute)
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); err != nil {
		t.Fatal(err)
	}
	got, _ := e.store.GetAccount(ctx, a.ID)
	if got.PasswordHash == weak || needsRehash(got.PasswordHash, e.svc.params) {
		t.Fatalf("hash not upgraded: %s", got.PasswordHash)
	}
	if ok, _ := VerifyPassword(testPassword, got.PasswordHash); !ok {
		t.Fatal("upgraded hash does not verify")
	}
	toks, _ := e.store.ListTokens(ctx, a.ID)
	for _, tk := range toks {
		if tk.ID == short.ID {
			t.Fatal("expired token not pruned")
		}
	}
}

func TestAuthenticate(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	valid, _, err := e.svc.CreateToken(ctx, alice.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	expiring, _, err := e.svc.CreateToken(ctx, alice.ID, "short", time.Second)
	if err != nil {
		t.Fatal(err)
	}
	unknown, _, _ := newToken()

	bob := e.register(t, "bob")
	bobTok, _, err := e.svc.CreateToken(ctx, bob.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	bob.Disabled = true
	if err := e.store.UpdateAccount(ctx, bob); err != nil {
		t.Fatal(err)
	}

	carol := e.register(t, "carol")
	carolTok, _, err := e.svc.CreateToken(ctx, carol.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	e.store.mu.Lock()
	delete(e.store.accounts, carol.ID)
	e.store.mu.Unlock()

	e.clock.Advance(time.Second)

	tests := []struct {
		name  string
		token string
		ok    bool
	}{
		{"valid", valid, true},
		{"expired", expiring, false},
		{"unknown", unknown, false},
		{"empty", "", false},
		{"garbage", "not-a-token", false},
		{"truncated", valid[:len(valid)-1], false},
		{"disabled account", bobTok, false},
		{"deleted account", carolTok, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			p, err := e.svc.Authenticate(ctx, tt.token)
			if tt.ok {
				if err != nil || p.AccountID != alice.ID {
					t.Fatalf("Authenticate = %+v, %v", p, err)
				}
				return
			}
			if p != nil || !errors.Is(err, domain.ErrUnauthenticated) {
				t.Fatalf("Authenticate = %+v, %v; want ErrUnauthenticated", p, err)
			}
		})
	}
}

func TestAuthenticateReflectsAdmin(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	tok, _, _ := e.svc.CreateToken(ctx, a.ID, "t", 0)
	a.IsAdmin = true
	_ = e.store.UpdateAccount(ctx, a)
	p, err := e.svc.Authenticate(ctx, tok)
	if err != nil || !p.IsAdmin {
		t.Fatalf("Authenticate = %+v, %v", p, err)
	}
}

func TestAuthenticateTouchThrottle(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	tok, rec, err := e.svc.CreateToken(ctx, a.ID, "ci", 0)
	if err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 5; i++ {
		if _, err := e.svc.Authenticate(ctx, tok); err != nil {
			t.Fatal(err)
		}
		e.clock.Advance(10 * time.Second)
	}
	if n := e.store.touchCount(); n != 1 {
		t.Fatalf("TouchToken called %d times within a minute, want 1", n)
	}
	e.clock.Advance(15 * time.Second) // 65s since first use
	if _, err := e.svc.Authenticate(ctx, tok); err != nil {
		t.Fatal(err)
	}
	if n := e.store.touchCount(); n != 2 {
		t.Fatalf("TouchToken called %d times after a minute, want 2", n)
	}
	stored, _ := e.store.GetTokenByHash(ctx, rec.TokenHash)
	if stored.LastUsedAt == nil || !stored.LastUsedAt.Equal(e.clock.Now()) {
		t.Fatalf("LastUsedAt = %v, want %v", stored.LastUsedAt, e.clock.Now())
	}

	// Concurrent first use of a fresh token touches once.
	tok2, _, _ := e.svc.CreateToken(ctx, a.ID, "ci2", 0)
	var wg sync.WaitGroup
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			_, _ = e.svc.Authenticate(ctx, tok2)
		}()
	}
	wg.Wait()
	if n := e.store.touchCount(); n != 3 {
		t.Fatalf("TouchToken called %d times total, want 3", n)
	}
}

func TestCreateToken(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	d := e.register(t, "dora")
	d.Disabled = true
	_ = e.store.UpdateAccount(ctx, d)

	tests := []struct {
		name    string
		account string
		tname   string
		ttl     time.Duration
		want    error
		expires bool
	}{
		{"no expiry", a.ID, "ci", 0, nil, false},
		{"with ttl", a.ID, "laptop", 24 * time.Hour, nil, true},
		{"trimmed name", a.ID, "  spaced  ", 0, nil, false},
		{"negative ttl", a.ID, "bad", -time.Second, domain.ErrInvalid, false},
		{"empty name", a.ID, "  ", 0, domain.ErrInvalid, false},
		{"unknown account", "nope", "ci", 0, domain.ErrNotFound, false},
		{"disabled account", d.ID, "ci", 0, domain.ErrForbidden, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			raw, tok, err := e.svc.CreateToken(ctx, tt.account, tt.tname, tt.ttl)
			if !errors.Is(err, tt.want) {
				t.Fatalf("CreateToken = %v, want %v", err, tt.want)
			}
			if tt.want != nil {
				return
			}
			if !wellFormedToken(raw) || tok.TokenHash != HashToken(raw) || tok.AccountID != tt.account ||
				tok.Name != strings.TrimSpace(tt.tname) || tok.ID == "" {
				t.Fatalf("bad token %q %+v", raw, tok)
			}
			if (tok.ExpiresAt != nil) != tt.expires {
				t.Fatalf("ExpiresAt = %v, want set=%v", tok.ExpiresAt, tt.expires)
			}
			if tt.expires && !tok.ExpiresAt.Equal(e.clock.Now().Add(tt.ttl)) {
				t.Fatalf("ExpiresAt = %v", tok.ExpiresAt)
			}
		})
	}
	// Tokens are unique.
	r1, _, _ := e.svc.CreateToken(ctx, a.ID, "x", 0)
	r2, _, _ := e.svc.CreateToken(ctx, a.ID, "x", 0)
	if r1 == r2 {
		t.Fatal("duplicate tokens issued")
	}
}

func TestRevokeToken(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	bob := e.register(t, "bob")
	root := e.register(t, "root")
	root.IsAdmin = true
	_ = e.store.UpdateAccount(ctx, root)

	mk := func(acct *domain.Account) (string, string) {
		raw, tok, err := e.svc.CreateToken(ctx, acct.ID, "t", 0)
		if err != nil {
			t.Fatal(err)
		}
		return raw, tok.ID
	}
	aliceRaw, aliceID := mk(alice)
	_, bobID := mk(bob)
	_, bob2ID := mk(bob)

	tests := []struct {
		name    string
		actor   string
		tokenID string
		want    error
	}{
		{"other's token", alice.ID, bobID, domain.ErrNotFound},
		{"unknown token", alice.ID, "missing", domain.ErrNotFound},
		{"unknown actor", "ghost", aliceID, domain.ErrUnauthenticated},
		{"own token", alice.ID, aliceID, nil},
		{"already revoked", alice.ID, aliceID, domain.ErrNotFound},
		{"admin revokes other's", root.ID, bobID, nil},
		{"admin unknown token", root.ID, "missing", domain.ErrNotFound},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if err := e.svc.RevokeToken(ctx, tt.actor, tt.tokenID); !errors.Is(err, tt.want) {
				t.Fatalf("RevokeToken = %v, want %v", err, tt.want)
			}
		})
	}
	if _, err := e.svc.Authenticate(ctx, aliceRaw); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("revoked token still authenticates: %v", err)
	}
	if toks, _ := e.store.ListTokens(ctx, bob.ID); len(toks) != 1 || toks[0].ID != bob2ID {
		t.Fatalf("bob's tokens = %+v", toks)
	}
}

func TestSetPassword(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	if err := e.svc.SetPassword(ctx, a.ID, "short"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("SetPassword(short) = %v", err)
	}
	if err := e.svc.SetPassword(ctx, "ghost", "a new password"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("SetPassword(unknown) = %v", err)
	}
	if err := e.svc.SetPassword(ctx, a.ID, "a new password"); err != nil {
		t.Fatal(err)
	}
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("old password still works: %v", err)
	}
	if _, _, err := e.svc.Login(ctx, "alice", "a new password"); err != nil {
		t.Fatalf("new password rejected: %v", err)
	}
}

func TestHashConcurrencyHonorsContext(t *testing.T) {
	e := newTestEnv(t, nil)
	// Fill the semaphore, then a hash must give up when ctx is cancelled.
	for i := 0; i < cap(e.svc.hashSem); i++ {
		e.svc.hashSem <- struct{}{}
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if _, err := e.svc.Register(ctx, "alice", "a@example.com", testPassword); !errors.Is(err, context.Canceled) {
		t.Fatalf("Register = %v, want context.Canceled", err)
	}
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); !errors.Is(err, context.Canceled) {
		t.Fatalf("Login = %v, want context.Canceled", err)
	}
}
