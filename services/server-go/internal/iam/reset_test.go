package iam

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestCreatePasswordReset(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	code, exp, err := e.svc.CreatePasswordReset(ctx, a.ID, "root", 0)
	if err != nil {
		t.Fatal(err)
	}
	if !wellFormedSecret(ResetCodePrefix, code) || !exp.Equal(e.clock.Now().Add(DefaultResetTTL)) {
		t.Fatalf("code %q expires %v", code, exp)
	}
	r, err := e.store.GetPasswordReset(ctx, HashToken(code))
	if err != nil || r.AccountID != a.ID || r.CreatedBy != "root" || r.UsedAt != nil {
		t.Fatalf("stored %+v, %v", r, err)
	}
	if _, exp, err := e.svc.CreatePasswordReset(ctx, a.ID, "root", 3*time.Hour); err != nil || !exp.Equal(e.clock.Now().Add(3*time.Hour)) {
		t.Fatalf("custom ttl = %v, %v", exp, err)
	}
	for _, ttl := range []time.Duration{-time.Second, MaxResetTTL + time.Second} {
		if _, _, err := e.svc.CreatePasswordReset(ctx, a.ID, "root", ttl); !errors.Is(err, domain.ErrInvalid) {
			t.Fatalf("ttl %v = %v", ttl, err)
		}
	}
	if _, _, err := e.svc.CreatePasswordReset(ctx, "nope", "root", 0); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown account = %v", err)
	}
}

func TestResetPassword(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	b := e.register(t, "bob")
	session, _, _ := e.svc.Login(ctx, "alice", testPassword)
	api, _, _ := e.svc.CreateToken(ctx, a.ID, "ci", 0)
	bobTok, _, _ := e.svc.Login(ctx, "bob", testPassword)
	code, _, _ := e.svc.CreatePasswordReset(ctx, a.ID, "root", 0)

	// Policy violations do not consume the code.
	if err := e.svc.ResetPassword(ctx, code, "short"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("weak password = %v", err)
	}
	const newPW = "a brand new passphrase"
	if err := e.svc.ResetPassword(ctx, code, newPW); err != nil {
		t.Fatal(err)
	}
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("old password = %v", err)
	}
	if _, _, err := e.svc.Login(ctx, "alice", newPW); err != nil {
		t.Fatalf("new password = %v", err)
	}
	for _, tok := range []string{session, api} {
		if _, err := e.svc.Authenticate(ctx, tok); !errors.Is(err, domain.ErrUnauthenticated) {
			t.Fatalf("token survived reset: %v", err)
		}
	}
	if _, err := e.svc.Authenticate(ctx, bobTok); err != nil {
		t.Fatalf("other account's token revoked: %v", err)
	}
	if r, _ := e.store.GetPasswordReset(ctx, HashToken(code)); r.UsedAt == nil {
		t.Fatal("code not marked used")
	}
	// Single use.
	if err := e.svc.ResetPassword(ctx, code, "yet another passphrase"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("reused code = %v", err)
	}

	// Expired, unknown and malformed codes all fail the same way.
	expired, _, _ := e.svc.CreatePasswordReset(ctx, b.ID, "root", time.Minute)
	e.clock.Advance(time.Minute)
	unknown, _, _ := newSecret(ResetCodePrefix)
	for _, c := range []string{expired, unknown, "", "wtr_x", InvitationCodePrefix + unknown[len(ResetCodePrefix):]} {
		err := e.svc.ResetPassword(ctx, c, "another fine passphrase")
		if !errors.Is(err, domain.ErrInvalid) || !errors.Is(err, errInvalidReset) {
			t.Fatalf("ResetPassword(%q) = %v", c, err)
		}
	}
	if _, _, err := e.svc.Login(ctx, "bob", testPassword); err != nil {
		t.Fatalf("expired code changed bob's password: %v", err)
	}
}

func TestResetPasswordKeepsMFAAndCancelsPending(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	secret, _ := e.enableTOTP(t, a)
	res, _ := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
	code, _, _ := e.svc.CreatePasswordReset(ctx, a.ID, "root", 0)
	const newPW = "a brand new passphrase"
	if err := e.svc.ResetPassword(ctx, code, newPW); err != nil {
		t.Fatal(err)
	}
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("pending login survived reset: %v", err)
	}
	r2, err := e.svc.LoginWithMeta(ctx, "alice", newPW, domain.SessionMeta{})
	if err != nil || !r2.MFARequired {
		t.Fatalf("mfa not required after reset: %+v, %v", r2, err)
	}
}
