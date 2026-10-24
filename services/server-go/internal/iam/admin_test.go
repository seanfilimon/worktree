package iam

import (
	"context"
	"errors"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestEnsureAdminCreateAndIdempotent(t *testing.T) {
	ctx := context.Background()
	// Registration disabled: EnsureAdmin must still work.
	e := newTestEnv(t, func(o *Options) { o.AllowRegistration = false })

	for i := 0; i < 3; i++ {
		if err := EnsureAdmin(ctx, e.store, e.svc, "admin", testPassword); err != nil {
			t.Fatalf("run %d: %v", i, err)
		}
	}
	a, err := e.store.GetAccountByUsername(ctx, "admin")
	if err != nil {
		t.Fatal(err)
	}
	if !a.IsAdmin || a.Email != "admin@localhost" {
		t.Fatalf("admin account = %+v", a)
	}
	if n := len(e.store.accounts); n != 1 {
		t.Fatalf("%d accounts, want 1", n)
	}
	if n := len(e.store.tenants); n != 1 {
		t.Fatalf("%d tenants, want 1", n)
	}
	if role, _ := e.svc.RoleOf(ctx, a.ID, a.TenantID); role != domain.RoleOwner {
		t.Fatalf("admin role in personal tenant = %q", role)
	}
	// The hash is not rewritten when the password is unchanged.
	b, _ := e.store.GetAccountByUsername(ctx, "admin")
	if err := EnsureAdmin(ctx, e.store, e.svc, "admin", testPassword); err != nil {
		t.Fatal(err)
	}
	c, _ := e.store.GetAccountByUsername(ctx, "admin")
	if b.PasswordHash != c.PasswordHash {
		t.Fatal("password rehashed although unchanged")
	}

	tok, acct, err := e.svc.Login(ctx, "admin", testPassword)
	if err != nil || !acct.IsAdmin {
		t.Fatalf("admin login: %v", err)
	}
	p, err := e.svc.Authenticate(ctx, tok)
	if err != nil || !p.IsAdmin {
		t.Fatalf("admin principal = %+v, %v", p, err)
	}
	if err := e.svc.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err != nil {
		t.Fatalf("admin not authorized: %v", err)
	}
}

func TestEnsureAdminPromotesAndResets(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	e.register(t, "root")

	// Existing account, no password: only promotion.
	if err := EnsureAdmin(ctx, e.store, e.svc, "Root", ""); err != nil {
		t.Fatal(err)
	}
	a, _ := e.store.GetAccountByUsername(ctx, "root")
	if !a.IsAdmin {
		t.Fatal("not promoted")
	}
	if _, _, err := e.svc.Login(ctx, "root", testPassword); err != nil {
		t.Fatalf("password changed without being given: %v", err)
	}

	// New password resets.
	if err := EnsureAdmin(ctx, e.store, e.svc, "root", "brand new secret"); err != nil {
		t.Fatal(err)
	}
	if _, _, err := e.svc.Login(ctx, "root", testPassword); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("old password still valid: %v", err)
	}
	if _, _, err := e.svc.Login(ctx, "root", "brand new secret"); err != nil {
		t.Fatalf("new password rejected: %v", err)
	}
	a, _ = e.store.GetAccountByUsername(ctx, "root")
	if !a.IsAdmin {
		t.Fatal("admin flag lost on password reset")
	}
}

func TestEnsureAdminErrors(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	tests := []struct {
		name, user, pw string
		want           error
	}{
		{"missing password for new account", "admin", "", domain.ErrInvalid},
		{"short password", "admin", "short", domain.ErrInvalid},
		{"invalid username", "-bad", testPassword, domain.ErrInvalid},
		{"other reserved name", "api", testPassword, domain.ErrInvalid},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if err := EnsureAdmin(ctx, e.store, e.svc, tt.user, tt.pw); !errors.Is(err, tt.want) {
				t.Fatalf("EnsureAdmin = %v, want %v", err, tt.want)
			}
		})
	}
	if len(e.store.accounts) != 0 {
		t.Fatal("accounts created on error")
	}
}

// otherAuth is a non-*Service Authenticator delegating to a Service, used
// to exercise EnsureAdmin's generic path.
type otherAuth struct{ *Service }

func TestEnsureAdminGenericAuthenticator(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	var auth domain.Authenticator = otherAuth{e.svc}
	if err := EnsureAdmin(ctx, e.store, auth, "boss", testPassword); err != nil {
		t.Fatal(err)
	}
	a, err := e.store.GetAccountByUsername(ctx, "boss")
	if err != nil || !a.IsAdmin {
		t.Fatalf("account = %+v, %v", a, err)
	}
	if err := EnsureAdmin(ctx, e.store, auth, "boss", testPassword); err != nil {
		t.Fatal(err)
	}
}
