package iam

import (
	"context"
	"errors"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

var recoveryCodeRE = regexp.MustCompile(`^[0-9a-hjkmnp-tv-z]{4}-[0-9a-hjkmnp-tv-z]{4}$`)

// enableTOTP enrolls and confirms TOTP for a, then advances the clock one
// step so the confirmation code's step is not replayed by the caller. It
// returns the raw secret and the recovery codes.
func (e *testEnv) enableTOTP(t *testing.T, a *domain.Account) ([]byte, []string) {
	t.Helper()
	ctx := context.Background()
	secret, _, err := e.svc.BeginTOTPEnroll(ctx, a.ID)
	if err != nil {
		t.Fatalf("BeginTOTPEnroll: %v", err)
	}
	raw, err := decodeTOTPSecret(secret)
	if err != nil {
		t.Fatal(err)
	}
	codes, err := e.svc.ConfirmTOTP(ctx, a.ID, TOTPCode(raw, e.clock.Now()))
	if err != nil {
		t.Fatalf("ConfirmTOTP: %v", err)
	}
	e.clock.Advance(TOTPPeriod)
	return raw, codes
}

func (e *testEnv) code(secret []byte) string { return TOTPCode(secret, e.clock.Now()) }

func TestMFAStatusZero(t *testing.T) {
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	st, err := e.svc.MFAStatus(context.Background(), a.ID)
	if err != nil || st.AccountID != a.ID || st.TOTPEnabled || st.RecoveryCodesLeft != 0 || st.EnabledAt != nil {
		t.Fatalf("MFAStatus = %+v, %v", st, err)
	}
}

func TestTOTPEnrollConfirm(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")

	if _, err := e.svc.ConfirmTOTP(ctx, a.ID, "123456"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("ConfirmTOTP without enrollment = %v, want ErrInvalid", err)
	}

	secret, url, err := e.svc.BeginTOTPEnroll(ctx, a.ID)
	if err != nil {
		t.Fatal(err)
	}
	if url != "otpauth://totp/W0rkTree:alice?secret="+secret+"&issuer=W0rkTree" {
		t.Fatalf("otpauth url = %q", url)
	}
	// Pending enrollment does not enable MFA: password login still works.
	if st, _ := e.svc.MFAStatus(ctx, a.ID); st.TOTPEnabled {
		t.Fatal("enabled before confirmation")
	}
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); err != nil {
		t.Fatalf("Login with pending enrollment: %v", err)
	}
	// Restarting enrollment replaces the secret.
	secret2, _, err := e.svc.BeginTOTPEnroll(ctx, a.ID)
	if err != nil || secret2 == secret {
		t.Fatalf("re-enroll = %q, %v", secret2, err)
	}
	raw, _ := decodeTOTPSecret(secret2)
	old, _ := decodeTOTPSecret(secret)

	if _, err := e.svc.ConfirmTOTP(ctx, a.ID, e.code(old)); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("ConfirmTOTP with superseded secret = %v, want ErrInvalid", err)
	}
	if _, err := e.svc.ConfirmTOTP(ctx, a.ID, "000000x"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("ConfirmTOTP garbage = %v, want ErrInvalid", err)
	}
	codes, err := e.svc.ConfirmTOTP(ctx, a.ID, e.code(raw))
	if err != nil {
		t.Fatal(err)
	}
	if len(codes) != RecoveryCodeCount {
		t.Fatalf("got %d recovery codes", len(codes))
	}
	seen := map[string]bool{}
	for _, c := range codes {
		if !recoveryCodeRE.MatchString(c) || seen[c] {
			t.Fatalf("bad or duplicate recovery code %q", c)
		}
		seen[c] = true
	}

	stored, _ := e.store.rawMFA(a.ID)
	if !stored.TOTPEnabled || stored.TOTPSecret != secret2 || stored.PendingSecret != "" || stored.EnabledAt == nil {
		t.Fatalf("stored state %+v", stored)
	}
	if len(stored.RecoveryCodeHashes) != RecoveryCodeCount {
		t.Fatalf("stored %d hashes", len(stored.RecoveryCodeHashes))
	}
	for i, h := range stored.RecoveryCodeHashes {
		if h != hashRecoveryCode(codes[i]) || strings.Contains(h, "-") {
			t.Fatalf("hash %d = %q is not sha256 of the code", i, h)
		}
	}

	st, err := e.svc.MFAStatus(ctx, a.ID)
	if err != nil || !st.TOTPEnabled || st.RecoveryCodesLeft != RecoveryCodeCount || st.EnabledAt == nil {
		t.Fatalf("MFAStatus = %+v, %v", st, err)
	}
	if st.TOTPSecret != "" || st.PendingSecret != "" || st.RecoveryCodeHashes != nil || st.LastUsedStep != 0 {
		t.Fatalf("MFAStatus leaks secrets: %+v", st)
	}

	if _, _, err := e.svc.BeginTOTPEnroll(ctx, a.ID); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("BeginTOTPEnroll when enabled = %v, want ErrConflict", err)
	}
	if _, err := e.svc.ConfirmTOTP(ctx, a.ID, e.code(raw)); !errors.Is(err, domain.ErrConflict) {
		t.Fatalf("ConfirmTOTP when enabled = %v, want ErrConflict", err)
	}
}

func TestTOTPEnrollAccountChecks(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	d := e.register(t, "dora")
	d.Disabled = true
	_ = e.store.UpdateAccount(ctx, d)
	if _, _, err := e.svc.BeginTOTPEnroll(ctx, d.ID); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("disabled = %v", err)
	}
	if _, _, err := e.svc.BeginTOTPEnroll(ctx, "nope"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown = %v", err)
	}
}

func TestDisableTOTP(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	// Disabling without MFA is a no-op that still checks the password.
	if err := e.svc.DisableTOTP(ctx, a.ID, "wrong password"); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("wrong password = %v, want ErrForbidden", err)
	}
	if err := e.svc.DisableTOTP(ctx, a.ID, testPassword); err != nil {
		t.Fatalf("disable without mfa: %v", err)
	}
	e.enableTOTP(t, a)
	if err := e.svc.DisableTOTP(ctx, a.ID, strings.Repeat("x", MaxPasswordLen+1)); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("oversized password = %v", err)
	}
	if err := e.svc.DisableTOTP(ctx, a.ID, testPassword); err != nil {
		t.Fatal(err)
	}
	if st, _ := e.svc.MFAStatus(ctx, a.ID); st.TOTPEnabled || st.RecoveryCodesLeft != 0 {
		t.Fatalf("still enabled: %+v", st)
	}
	if _, ok := e.store.rawMFA(a.ID); ok {
		t.Fatal("mfa state not deleted")
	}
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); err != nil {
		t.Fatalf("Login after disable: %v", err)
	}
}

func TestRegenerateRecoveryCodes(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	if _, err := e.svc.RegenerateRecoveryCodes(ctx, a.ID, "123456"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("without mfa = %v, want ErrInvalid", err)
	}
	secret, old := e.enableTOTP(t, a)
	if _, err := e.svc.RegenerateRecoveryCodes(ctx, a.ID, "000000"); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("wrong code = %v, want ErrForbidden", err)
	}
	if _, err := e.svc.RegenerateRecoveryCodes(ctx, a.ID, old[0]); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("recovery code accepted for regeneration: %v", err)
	}
	code := e.code(secret)
	fresh, err := e.svc.RegenerateRecoveryCodes(ctx, a.ID, code)
	if err != nil || len(fresh) != RecoveryCodeCount {
		t.Fatalf("Regenerate = %v, %v", fresh, err)
	}
	// The same TOTP step cannot be used again.
	if _, err := e.svc.RegenerateRecoveryCodes(ctx, a.ID, code); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("replayed code = %v, want ErrForbidden", err)
	}
	// Old codes no longer work, new ones do.
	e.clock.Advance(TOTPPeriod)
	res, _ := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, old[1], domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("old recovery code = %v", err)
	}
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, fresh[0], domain.SessionMeta{}); err != nil {
		t.Fatalf("new recovery code: %v", err)
	}
}

func TestMFALoginFlow(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	secret, _ := e.enableTOTP(t, a)

	// The single-step Login refuses after a correct password only.
	if _, _, err := e.svc.Login(ctx, "alice", "wrong password"); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("Login wrong password = %v", err)
	}
	_, _, err := e.svc.Login(ctx, "alice", testPassword)
	if !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), "mfa required") {
		t.Fatalf("Login = %v, want ErrForbidden mfa required", err)
	}

	meta := domain.SessionMeta{RemoteIP: "203.0.113.7:4444", UserAgent: "wt-cli/0.4"}
	res, err := e.svc.LoginWithMeta(ctx, "Alice", testPassword, meta)
	if err != nil {
		t.Fatal(err)
	}
	if !res.MFARequired || res.Token != "" || !strings.HasPrefix(res.MFAToken, MFATokenPrefix) ||
		res.Account == nil || res.Account.Username != "alice" {
		t.Fatalf("LoginWithMeta = %+v", res)
	}
	if toks, _ := e.store.ListTokens(ctx, a.ID); len(toks) != 0 {
		t.Fatalf("session issued before second factor: %+v", toks)
	}

	raw, acct, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, e.code(secret), meta)
	if err != nil || acct.ID != a.ID {
		t.Fatalf("CompleteMFALogin = %v, %v", acct, err)
	}
	p, err := e.svc.Authenticate(ctx, raw)
	if err != nil || !p.MFA || p.Kind != domain.PrincipalUser || p.TokenID == "" || p.AccountID != a.ID {
		t.Fatalf("Authenticate = %+v, %v", p, err)
	}
	tok, _ := e.store.token(p.TokenID)
	if tok.Kind != domain.TokenSession || !tok.MFA || tok.LastIP != "203.0.113.7" || tok.UserAgent != "wt-cli/0.4" ||
		tok.ExpiresAt == nil {
		t.Fatalf("session token %+v", tok)
	}
	// MFA tokens are single use.
	e.clock.Advance(TOTPPeriod)
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, e.code(secret), meta); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("reused mfa token = %v", err)
	}
}

func TestMFALoginReplayAndRecovery(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	secret, codes := e.enableTOTP(t, a)
	login := func() string {
		t.Helper()
		res, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
		if err != nil || !res.MFARequired {
			t.Fatalf("LoginWithMeta = %+v, %v", res, err)
		}
		return res.MFAToken
	}

	code := e.code(secret)
	if _, _, err := e.svc.CompleteMFALogin(ctx, login(), code, domain.SessionMeta{}); err != nil {
		t.Fatal(err)
	}
	// Same code again within its step: replay.
	if _, _, err := e.svc.CompleteMFALogin(ctx, login(), code, domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("replayed TOTP = %v, want ErrUnauthenticated", err)
	}

	// Recovery code, accepted in any case and without the hyphen, once.
	rc := strings.ToUpper(strings.ReplaceAll(codes[3], "-", ""))
	raw, _, err := e.svc.CompleteMFALogin(ctx, login(), rc, domain.SessionMeta{})
	if err != nil {
		t.Fatalf("recovery code: %v", err)
	}
	if p, _ := e.svc.Authenticate(ctx, raw); !p.MFA {
		t.Fatal("recovery-code session lacks MFA")
	}
	if st, _ := e.svc.MFAStatus(ctx, a.ID); st.RecoveryCodesLeft != RecoveryCodeCount-1 {
		t.Fatalf("RecoveryCodesLeft = %d", st.RecoveryCodesLeft)
	}
	if _, _, err := e.svc.CompleteMFALogin(ctx, login(), codes[3], domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("reused recovery code = %v", err)
	}
	if _, _, err := e.svc.CompleteMFALogin(ctx, login(), "zzzz-zzzz", domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("unknown recovery code = %v", err)
	}
}

func TestMFATokenExpiryAndAttempts(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	secret, _ := e.enableTOTP(t, a)
	login := func() string {
		res, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
		if err != nil {
			t.Fatal(err)
		}
		return res.MFAToken
	}

	tok := login()
	e.clock.Advance(MFATokenTTL)
	if _, _, err := e.svc.CompleteMFALogin(ctx, tok, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("expired mfa token = %v", err)
	}

	tok = login()
	for i := 0; i < MaxMFATokenAttempts; i++ {
		if _, _, err := e.svc.CompleteMFALogin(ctx, tok, "000000", domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
			t.Fatalf("attempt %d = %v", i, err)
		}
	}
	if _, _, err := e.svc.CompleteMFALogin(ctx, tok, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("exhausted mfa token accepted: %v", err)
	}

	for _, bad := range []string{"", "garbage", MFATokenPrefix + strings.Repeat("A", 43), TokenPrefix + strings.Repeat("A", 43)} {
		if _, _, err := e.svc.CompleteMFALogin(ctx, bad, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
			t.Fatalf("CompleteMFALogin(%q) = %v", bad, err)
		}
	}
}

func TestMFAAccountRateLimit(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, func(o *Options) {
		o.MaxMFAFailures = 3
		o.MFAFailureWindow = 10 * time.Minute
	})
	a := e.register(t, "alice")
	secret, _ := e.enableTOTP(t, a)
	login := func() string {
		res, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
		if err != nil {
			t.Fatal(err)
		}
		return res.MFAToken
	}
	// Failures across different MFA tokens and operations add up.
	if _, _, err := e.svc.CompleteMFALogin(ctx, login(), "000000", domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatal(err)
	}
	if _, err := e.svc.RegenerateRecoveryCodes(ctx, a.ID, "000000"); !errors.Is(err, domain.ErrForbidden) {
		t.Fatal(err)
	}
	if _, _, err := e.svc.CompleteMFALogin(ctx, login(), "000000", domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatal(err)
	}
	tok := login()
	_, _, err := e.svc.CompleteMFALogin(ctx, tok, e.code(secret), domain.SessionMeta{})
	if !errors.Is(err, domain.ErrForbidden) || !strings.Contains(err.Error(), "too many") {
		t.Fatalf("blocked account = %v, want ErrForbidden", err)
	}
	if _, err := e.svc.RegenerateRecoveryCodes(ctx, a.ID, e.code(secret)); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("blocked regenerate = %v", err)
	}
	if err := e.svc.DisableTOTP(ctx, a.ID, testPassword); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("blocked disable = %v", err)
	}
	e.clock.Advance(6 * time.Minute) // mfa token expired meanwhile
	if _, _, err := e.svc.CompleteMFALogin(ctx, tok, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("expired token = %v", err)
	}
	e.clock.Advance(5 * time.Minute) // window over
	if _, _, err := e.svc.CompleteMFALogin(ctx, login(), e.code(secret), domain.SessionMeta{}); err != nil {
		t.Fatalf("after window: %v", err)
	}
}

func TestMFALoginStateChanges(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	secret, _ := e.enableTOTP(t, a)

	// Account disabled between the steps.
	res, _ := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
	a.Disabled = true
	_ = e.store.UpdateAccount(ctx, a)
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("disabled between steps = %v", err)
	}
	a.Disabled = false
	_ = e.store.UpdateAccount(ctx, a)

	// MFA disabled between the steps: the token is void.
	res, _ = e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
	e.store.mu.Lock()
	delete(e.store.iam().mfa, a.ID)
	e.store.mu.Unlock()
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("mfa disabled between steps = %v", err)
	}

	// Disabling MFA cancels pending logins.
	secret, _ = e.enableTOTP(t, a)
	res, _ = e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
	if err := e.svc.DisableTOTP(ctx, a.ID, testPassword); err != nil {
		t.Fatal(err)
	}
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, e.code(secret), domain.SessionMeta{}); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("pending login survived DisableTOTP: %v", err)
	}
}

func TestMFAStoreFailuresFailClosed(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	secret, _ := e.enableTOTP(t, a)

	res, _ := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
	e.store.mu.Lock()
	e.store.iam().failPutMFA = true
	e.store.mu.Unlock()
	// The replay state cannot be persisted: the code must not be accepted.
	if _, _, err := e.svc.CompleteMFALogin(ctx, res.MFAToken, e.code(secret), domain.SessionMeta{}); err == nil {
		t.Fatal("accepted a code whose use could not be recorded")
	}
	e.store.mu.Lock()
	e.store.iam().failPutMFA = false
	e.store.iam().failGetMFA = true
	e.store.mu.Unlock()
	if _, _, err := e.svc.Login(ctx, "alice", testPassword); err == nil {
		t.Fatal("Login succeeded although the mfa state could not be read")
	}
	if _, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{}); err == nil {
		t.Fatal("LoginWithMeta succeeded although the mfa state could not be read")
	}
}

func TestMFAPendingTable(t *testing.T) {
	p := newMFAPending()
	now := time.Unix(1000, 0)
	tok, err := p.create("acct", now)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := p.claim(tok, now); err != nil {
		t.Fatal(err)
	}
	// A concurrent second claim of the same token fails.
	if _, err := p.claim(tok, now); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("double claim = %v", err)
	}
	p.release(tok)
	if id, err := p.claim(tok, now); err != nil || id != "acct" {
		t.Fatalf("claim after release = %q, %v", id, err)
	}
	p.consume(tok)
	if _, err := p.claim(tok, now); err == nil {
		t.Fatal("consumed token claimed")
	}

	// Bounded size: the oldest entry is evicted.
	small := newMFAPending()
	first, _ := small.create("a", now)
	for i := 1; i < maxPendingMFA; i++ {
		if _, err := small.create("b", now.Add(time.Second)); err != nil {
			t.Fatal(err)
		}
	}
	last, _ := small.create("c", now.Add(2*time.Second))
	if len(small.entries) != maxPendingMFA {
		t.Fatalf("table size %d", len(small.entries))
	}
	if _, err := small.claim(first, now); err == nil {
		t.Fatal("oldest entry not evicted")
	}
	if _, err := small.claim(last, now); err != nil {
		t.Fatalf("newest entry missing: %v", err)
	}
}

func TestConcurrentRecoveryCodeSingleUse(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	_, codes := e.enableTOTP(t, a)
	const n = 8
	toks := make([]string, n)
	for i := range toks {
		res, err := e.svc.LoginWithMeta(ctx, "alice", testPassword, domain.SessionMeta{})
		if err != nil {
			t.Fatal(err)
		}
		toks[i] = res.MFAToken
	}
	var wg sync.WaitGroup
	var mu sync.Mutex
	ok := 0
	for _, tok := range toks {
		wg.Add(1)
		go func(tok string) {
			defer wg.Done()
			if _, _, err := e.svc.CompleteMFALogin(ctx, tok, codes[0], domain.SessionMeta{}); err == nil {
				mu.Lock()
				ok++
				mu.Unlock()
			}
		}(tok)
	}
	wg.Wait()
	if ok != 1 {
		t.Fatalf("recovery code accepted %d times", ok)
	}
}

func TestRecoveryCodeNormalization(t *testing.T) {
	tests := []struct {
		in, want string
		ok       bool
	}{
		{"abcd-efgh", "abcd-efgh", true},
		{" ABCD-EFGH ", "abcd-efgh", true},
		{"abcdefgh", "abcd-efgh", true},
		{"abcd efgh", "abcd-efgh", true},
		{"abcd-efg", "", false},
		{"abcd-efgi", "", false}, // i is not in the alphabet
		{"abcd-efgh-1", "", false},
		{"", "", false},
	}
	for _, tt := range tests {
		got, ok := normalizeRecoveryCode(tt.in)
		if got != tt.want || ok != tt.ok {
			t.Errorf("normalizeRecoveryCode(%q) = %q, %v", tt.in, got, ok)
		}
	}
}
