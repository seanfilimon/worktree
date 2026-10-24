package iam

import (
	"context"
	"errors"
	"fmt"
	"net"
	"testing"
	"time"

	"google.golang.org/grpc/peer"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestLoginRateLimit(t *testing.T) {
	e := newTestEnv(t, func(o *Options) { o.DisableRateLimit = false })
	e.register(t, "alice")
	fromIP := func(ip string) context.Context {
		return WithRemoteAddr(context.Background(), ip+":5555")
	}
	ctxA := fromIP("10.0.0.1")

	for i := 0; i < DefaultMaxLoginFailures; i++ {
		if _, _, err := e.svc.Login(ctxA, "alice", "wrong password"); !errors.Is(err, domain.ErrUnauthenticated) {
			t.Fatalf("attempt %d: %v", i, err)
		}
	}
	// Locked, even with the right password.
	if _, _, err := e.svc.Login(ctxA, "alice", testPassword); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("expected lockout, got %v", err)
	}
	// Username matching is case-insensitive for the limiter too.
	if _, _, err := e.svc.Login(ctxA, "ALICE", testPassword); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("expected lockout for upper-case variant, got %v", err)
	}
	// Other IPs are unaffected.
	if _, _, err := e.svc.Login(fromIP("10.0.0.2"), "alice", testPassword); err != nil {
		t.Fatalf("other IP locked out: %v", err)
	}
	// Window passes.
	e.clock.Advance(DefaultLoginFailureWindow)
	if _, _, err := e.svc.Login(ctxA, "alice", testPassword); err != nil {
		t.Fatalf("still locked after window: %v", err)
	}
}

func TestLoginRateLimitUnknownUserAndReset(t *testing.T) {
	e := newTestEnv(t, func(o *Options) {
		o.DisableRateLimit = false
		o.MaxLoginFailures = 3
		o.LoginFailureWindow = time.Minute
	})
	e.register(t, "alice")
	ctx := WithRemoteAddr(context.Background(), "192.0.2.7")

	// Unknown usernames are limited as well.
	for i := 0; i < 3; i++ {
		_, _, _ = e.svc.Login(ctx, "ghost", "whatever pw")
	}
	if _, _, err := e.svc.Login(ctx, "ghost", "whatever pw"); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("unknown user not limited: %v", err)
	}

	// A success resets the counter.
	for i := 0; i < 2; i++ {
		_, _, _ = e.svc.Login(ctx, "alice", "wrong password")
	}
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); err != nil {
		t.Fatal(err)
	}
	for i := 0; i < 2; i++ {
		_, _, _ = e.svc.Login(ctx, "alice", "wrong password")
	}
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); err != nil {
		t.Fatalf("counter not reset by success: %v", err)
	}
}

func TestLoginRateLimitGRPCPeer(t *testing.T) {
	e := newTestEnv(t, func(o *Options) {
		o.DisableRateLimit = false
		o.MaxLoginFailures = 1
	})
	e.register(t, "alice")
	peerCtx := func(ip string) context.Context {
		return peer.NewContext(context.Background(), &peer.Peer{Addr: &net.TCPAddr{IP: net.ParseIP(ip), Port: 1234}})
	}
	_, _, _ = e.svc.Login(peerCtx("10.1.1.1"), "alice", "wrong password")
	if _, _, err := e.svc.Login(peerCtx("10.1.1.1"), "alice", testPassword); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("peer address not used for limiting: %v", err)
	}
	if _, _, err := e.svc.Login(peerCtx("10.1.1.2"), "alice", testPassword); err != nil {
		t.Fatalf("different peer locked: %v", err)
	}
}

func TestLoginLimiterSweep(t *testing.T) {
	l := newLoginLimiter(2, time.Minute)
	now := time.Unix(1000, 0)
	for i := 0; i < limiterSweepThreshold; i++ {
		l.fail(fmt.Sprintf("k%d", i), now)
	}
	later := now.Add(2 * time.Minute)
	l.fail("fresh", later)
	if n := len(l.entries); n != 1 {
		t.Fatalf("sweep left %d entries, want 1", n)
	}
	if l.blocked("fresh", later) {
		t.Fatal("blocked after one failure")
	}
	l.fail("fresh", later)
	if !l.blocked("fresh", later) {
		t.Fatal("not blocked after max failures")
	}
	var nilLimiter *loginLimiter
	nilLimiter.fail("x", now)
	nilLimiter.reset("x")
	if nilLimiter.blocked("x", now) {
		t.Fatal("nil limiter blocks")
	}
}

func TestClientIP(t *testing.T) {
	tests := []struct {
		ctx  context.Context
		want string
	}{
		{context.Background(), ""},
		{WithRemoteAddr(context.Background(), "1.2.3.4:80"), "1.2.3.4"},
		{WithRemoteAddr(context.Background(), "[::1]:80"), "::1"},
		{WithRemoteAddr(context.Background(), "5.6.7.8"), "5.6.7.8"},
	}
	for _, tt := range tests {
		if got := clientIP(tt.ctx); got != tt.want {
			t.Errorf("clientIP = %q, want %q", got, tt.want)
		}
	}
}
