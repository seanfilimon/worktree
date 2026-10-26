package iam

import (
	"context"
	"errors"
	"regexp"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

var userCodeRe = regexp.MustCompile(`^[BCDFGHJKLMNPQRSTVWXZ]{4}-[BCDFGHJKLMNPQRSTVWXZ]{4}$`)

func (e *testEnv) startDevice(t *testing.T, scopes ...string) *DeviceAuthorization {
	t.Helper()
	da, err := e.svc.StartDeviceAuthorization(context.Background(), "wt-cli", "wt on laptop", scopes,
		domain.SessionMeta{RemoteIP: "198.51.100.20", UserAgent: "worktree-sdk/0.8"})
	if err != nil {
		t.Fatalf("StartDeviceAuthorization: %v", err)
	}
	return da
}

func (e *testEnv) poll(da *DeviceAuthorization) (*TokenPair, error) {
	return e.svc.PollDeviceToken(context.Background(), da.DeviceCode, domain.SessionMeta{RemoteIP: "198.51.100.20"})
}

func wantDeviceErr(t *testing.T, err, want error) {
	t.Helper()
	if !errors.Is(err, want) {
		t.Fatalf("err = %v, want %v", err, want)
	}
}

func TestDeviceFlowApprove(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	da := e.startDevice(t, "tree:read", "tree:read", "mr:*")
	if !userCodeRe.MatchString(da.UserCode) || !strings.HasPrefix(da.DeviceCode, DeviceCodePrefix) {
		t.Fatalf("codes %q / %q", da.UserCode, da.DeviceCode)
	}
	if da.ExpiresIn != 10*time.Minute || da.Interval != 5*time.Second {
		t.Fatalf("expires_in %v interval %v", da.ExpiresIn, da.Interval)
	}

	_, err := e.poll(da)
	wantDeviceErr(t, err, ErrAuthorizationPending)
	// Polling again before the interval passed: slow down.
	e.clock.Advance(time.Second)
	_, err = e.poll(da)
	wantDeviceErr(t, err, ErrSlowDown)
	// The interval grew to 10 s: 6 s later is still too fast.
	e.clock.Advance(6 * time.Second)
	_, err = e.poll(da)
	wantDeviceErr(t, err, ErrSlowDown)
	e.clock.Advance(15 * time.Second)
	_, err = e.poll(da)
	wantDeviceErr(t, err, ErrAuthorizationPending)

	p := principalOf(alice)
	req, err := e.svc.DeviceRequest(ctx, strings.ToLower(strings.ReplaceAll(da.UserCode, "-", " ")), p)
	if err != nil {
		t.Fatalf("DeviceRequest: %v", err)
	}
	if req.UserCode != da.UserCode || req.ClientName != "wt on laptop" || req.ClientID != "wt-cli" || req.Status != DeviceStatusPending ||
		strings.Join(req.Scopes, ",") != "tree:read,mr:*" || req.RemoteIP != "198.51.100.20" || req.MFARequired {
		t.Fatalf("request = %+v", req)
	}
	got, err := e.svc.ApproveDevice(ctx, da.UserCode, p, "")
	if err != nil || got.Status != DeviceStatusApproved {
		t.Fatalf("ApproveDevice = %+v, %v", got, err)
	}
	if _, err := e.svc.DenyDevice(ctx, da.UserCode, p); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("deny after approve = %v", err)
	}

	e.clock.Advance(10 * time.Second)
	pair, err := e.poll(da)
	if err != nil {
		t.Fatalf("poll after approval: %v", err)
	}
	pr := e.authOK(t, pair.AccessToken)
	if pr.AccountID != alice.ID || pr.MFA || strings.Join(pr.Scopes, ",") != "tree:read,mr:*" {
		t.Fatalf("device principal = %+v", pr)
	}
	if pair.Session.Name != "device: wt on laptop" {
		t.Fatalf("session name %q", pair.Session.Name)
	}
	sessions, _ := e.svc.ListSessions(ctx, alice.ID)
	if len(sessions) != 1 || !strings.HasPrefix(sessions[0].Name, DeviceSessionPrefix) {
		t.Fatalf("device sessions are listed: %+v", sessions)
	}
	if _, err := e.svc.Refresh(ctx, pair.RefreshToken, domain.SessionMeta{}); err != nil {
		t.Fatalf("device refresh token: %v", err)
	}
	// Single use.
	e.clock.Advance(10 * time.Second)
	_, err = e.poll(da)
	wantDeviceErr(t, err, ErrInvalidGrant)
	if _, err := e.svc.DeviceRequest(ctx, da.UserCode, p); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("redeemed code = %v", err)
	}
}

func TestDeviceFlowDenyAndExpiry(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	p := principalOf(alice)

	da := e.startDevice(t)
	if req, err := e.svc.DenyDevice(ctx, da.UserCode, p); err != nil || req.Status != DeviceStatusDenied {
		t.Fatalf("DenyDevice = %+v, %v", req, err)
	}
	_, err := e.poll(da)
	wantDeviceErr(t, err, ErrAccessDenied)
	_, err = e.poll(da)
	wantDeviceErr(t, err, ErrInvalidGrant)

	da = e.startDevice(t)
	e.clock.Advance(DeviceCodeTTL)
	if _, err := e.svc.ApproveDevice(ctx, da.UserCode, p, ""); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("approve expired = %v", err)
	}
	_, err = e.poll(da)
	wantDeviceErr(t, err, ErrExpiredToken)

	for _, bad := range []string{"", "wtd_short", "wt_" + strings.Repeat("a", 43), DeviceCodePrefix + strings.Repeat("a", 43)} {
		if _, err := e.svc.PollDeviceToken(ctx, bad, domain.SessionMeta{}); !errors.Is(err, ErrInvalidGrant) {
			t.Errorf("PollDeviceToken(%q) = %v", bad, err)
		}
	}
}

func TestDeviceFlowMFA(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	secret, recovery := e.enableTOTP(t, alice)

	da := e.startDevice(t)
	p := principalOf(alice) // a session without a second factor
	req, err := e.svc.DeviceRequest(ctx, da.UserCode, p)
	if err != nil || !req.MFARequired {
		t.Fatalf("DeviceRequest = %+v, %v", req, err)
	}
	if _, err := e.svc.ApproveDevice(ctx, da.UserCode, p, ""); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("approve without code = %v", err)
	}
	if _, err := e.svc.ApproveDevice(ctx, da.UserCode, p, "000000"); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("approve with a wrong code = %v", err)
	}
	if _, err := e.svc.ApproveDevice(ctx, da.UserCode, p, e.code(secret)); err != nil {
		t.Fatalf("approve with TOTP: %v", err)
	}
	pair, err := e.poll(da)
	if err != nil {
		t.Fatal(err)
	}
	if pr := e.authOK(t, pair.AccessToken); !pr.MFA {
		t.Fatalf("device session must record the second factor: %+v", pr)
	}

	// An MFA session approves without a code; recovery codes work too.
	mp := principalOf(alice)
	mp.MFA = true
	da = e.startDevice(t)
	if req, _ := e.svc.DeviceRequest(ctx, da.UserCode, mp); req == nil || req.MFARequired {
		t.Fatalf("MFA session: %+v", req)
	}
	if _, err := e.svc.ApproveDevice(ctx, da.UserCode, mp, ""); err != nil {
		t.Fatal(err)
	}
	da = e.startDevice(t)
	if _, err := e.svc.ApproveDevice(ctx, da.UserCode, p, recovery[0]); err != nil {
		t.Fatalf("approve with a recovery code: %v", err)
	}
}

func TestDeviceRateLimits(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	alice := e.register(t, "alice")
	p := principalOf(alice)

	for i := 0; i < maxDeviceCodeFailures; i++ {
		if _, err := e.svc.DeviceRequest(ctx, "BBBB-BBBB", p); !errors.Is(err, domain.ErrNotFound) {
			t.Fatalf("lookup %d = %v", i, err)
		}
	}
	da := e.startDevice(t)
	if _, err := e.svc.DeviceRequest(ctx, da.UserCode, p); !errors.Is(err, ErrRateLimited) {
		t.Fatalf("after %d failures = %v", maxDeviceCodeFailures, err)
	}
	e.clock.Advance(deviceRateWindow)
	da = e.startDevice(t) // the old code expired with the window
	if _, err := e.svc.DeviceRequest(ctx, da.UserCode, p); err != nil {
		t.Fatalf("after the window: %v", err)
	}

	for i := 1; i < maxDeviceStartsPerIP; i++ { // one was started above
		e.startDevice(t)
	}
	if _, err := e.svc.StartDeviceAuthorization(ctx, "", "", nil, domain.SessionMeta{RemoteIP: "198.51.100.20"}); !errors.Is(err, ErrRateLimited) {
		t.Fatalf("start over the limit = %v", err)
	}
	if _, err := e.svc.StartDeviceAuthorization(ctx, "", "", nil, domain.SessionMeta{RemoteIP: "198.51.100.21"}); err != nil {
		t.Fatalf("another address: %v", err)
	}
}

func TestDeviceStartValidation(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	if _, err := e.svc.StartDeviceAuthorization(ctx, "bad id!", "", nil, domain.SessionMeta{}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad client id = %v", err)
	}
	if _, err := e.svc.StartDeviceAuthorization(ctx, "", "", []string{"nope:x"}, domain.SessionMeta{}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad scope = %v", err)
	}
	da, err := e.svc.StartDeviceAuthorization(ctx, "", " \x00 "+strings.Repeat("é", 80), nil, domain.SessionMeta{})
	if err != nil {
		t.Fatal(err)
	}
	alice := e.register(t, "alice")
	req, err := e.svc.DeviceRequest(ctx, da.UserCode, principalOf(alice))
	if err != nil {
		t.Fatal(err)
	}
	if len(req.ClientName) > maxDeviceClientName || strings.ContainsRune(req.ClientName, 0) || req.Scopes == nil {
		t.Fatalf("client name %q scopes %v", req.ClientName, req.Scopes)
	}
	da, _ = e.svc.StartDeviceAuthorization(ctx, "", "", nil, domain.SessionMeta{})
	if req, _ := e.svc.DeviceRequest(ctx, da.UserCode, principalOf(alice)); req.ClientName != "Device" {
		t.Fatalf("default client name %q", req.ClientName)
	}
	// Service accounts cannot approve.
	if _, err := e.svc.ApproveDevice(ctx, da.UserCode, &domain.Principal{Kind: domain.PrincipalServiceAccount, ServiceAccountID: "sa"}, ""); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("service account approve = %v", err)
	}
}

func TestDeviceSweep(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	e.startDevice(t)
	e.startDevice(t)
	e.clock.Advance(DeviceCodeTTL + deviceSweepMinInterval)
	e.startDevice(t) // sweeps the two expired requests
	raws, err := e.store.ListDocs(ctx, CollDeviceCodes, "")
	if err != nil {
		t.Fatal(err)
	}
	if len(raws) != 2 { // one request + its index
		t.Fatalf("%d device docs left", len(raws))
	}
}

func TestNormalizeUserCode(t *testing.T) {
	for in, want := range map[string]string{
		"BCDF-GHJK": "BCDF-GHJK", "bcdfghjk": "BCDF-GHJK", " bcdf ghjk ": "BCDF-GHJK", "B-C-D-F-G-H-J-K": "BCDF-GHJK",
		"BCDF-GHJA": "", "BCDF-GHJ": "", "BCDF-GHJKL": "", "": "", "BCDF_GHJK": "",
	} {
		got, ok := NormalizeUserCode(in)
		if got != want || ok != (want != "") {
			t.Errorf("NormalizeUserCode(%q) = %q, %v; want %q", in, got, ok, want)
		}
	}
	for i := 0; i < 200; i++ {
		c, err := newUserCode()
		if err != nil || !userCodeRe.MatchString(c) {
			t.Fatalf("newUserCode = %q, %v", c, err)
		}
	}
}
