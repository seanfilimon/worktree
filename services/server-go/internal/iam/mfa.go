package iam

import (
	"context"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Second-factor parameters.
const (
	// MFATokenPrefix starts the short-lived token returned by the first
	// login step of an account with MFA enabled.
	MFATokenPrefix = "wtm_"
	// MFATokenTTL is how long an MFA token may be exchanged.
	MFATokenTTL = 5 * time.Minute
	// MaxMFATokenAttempts is the number of wrong codes after which an MFA
	// token is discarded (the user must enter the password again).
	MaxMFATokenAttempts = 5
	// RecoveryCodeCount is the number of recovery codes issued at once.
	RecoveryCodeCount = 10

	// Per-account second-factor failure limit defaults (Options).
	DefaultMaxMFAFailures   = 10
	DefaultMFAFailureWindow = 15 * time.Minute

	// maxPendingMFA bounds the in-memory MFA token table.
	maxPendingMFA = 10000
)

var (
	errInvalidMFAToken = fmt.Errorf("invalid or expired mfa token: %w", domain.ErrUnauthenticated)
	errMFANotEnabled   = fmt.Errorf("two-factor authentication is not enabled: %w", domain.ErrInvalid)
)

// ---- pending first-step logins ----

// mfaPending holds the MFA tokens handed out by LoginWithMeta.
//
// Design choice: tokens are random 256-bit values kept (hashed) in process
// memory rather than self-contained signed values. This makes them single
// use and lets each token carry an attempt counter, at the cost that a
// server restart (or a request reaching another replica) invalidates
// in-flight logins, which only means the user enters the password again
// within the 5-minute window. The table is bounded: when full, expired
// entries are swept and then the entry closest to expiry is evicted.
type mfaPending struct {
	mu      sync.Mutex
	entries map[string]*pendingMFA // HashToken(raw) -> entry
}

type pendingMFA struct {
	accountID string
	expires   time.Time
	attempts  int
	busy      bool // claimed by an in-flight CompleteMFALogin
}

func newMFAPending() *mfaPending {
	return &mfaPending{entries: make(map[string]*pendingMFA)}
}

func (p *mfaPending) create(accountID string, now time.Time) (string, error) {
	raw, h, err := newSecret(MFATokenPrefix)
	if err != nil {
		return "", err
	}
	p.mu.Lock()
	defer p.mu.Unlock()
	if len(p.entries) >= maxPendingMFA {
		for k, e := range p.entries {
			if !now.Before(e.expires) {
				delete(p.entries, k)
			}
		}
	}
	if len(p.entries) >= maxPendingMFA {
		var oldest string
		var oldestExp time.Time
		for k, e := range p.entries {
			if oldest == "" || e.expires.Before(oldestExp) {
				oldest, oldestExp = k, e.expires
			}
		}
		delete(p.entries, oldest)
	}
	p.entries[h] = &pendingMFA{accountID: accountID, expires: now.Add(MFATokenTTL)}
	return raw, nil
}

// claim marks the token as in use and returns its account. It fails for
// malformed, unknown, expired or already claimed tokens.
func (p *mfaPending) claim(raw string, now time.Time) (string, error) {
	if !wellFormedSecret(MFATokenPrefix, raw) {
		return "", fmt.Errorf("iam: %w", errInvalidMFAToken)
	}
	h := HashToken(raw)
	p.mu.Lock()
	defer p.mu.Unlock()
	e, ok := p.entries[h]
	if !ok {
		return "", fmt.Errorf("iam: %w", errInvalidMFAToken)
	}
	if !now.Before(e.expires) {
		delete(p.entries, h)
		return "", fmt.Errorf("iam: %w", errInvalidMFAToken)
	}
	if e.busy {
		return "", fmt.Errorf("iam: mfa token is already being used: %w", domain.ErrUnauthenticated)
	}
	e.busy = true
	return e.accountID, nil
}

// release ends a claim without consuming the token.
func (p *mfaPending) release(raw string) {
	p.mu.Lock()
	if e, ok := p.entries[HashToken(raw)]; ok {
		e.busy = false
	}
	p.mu.Unlock()
}

// fail records a wrong code; the token is discarded after
// MaxMFATokenAttempts failures.
func (p *mfaPending) fail(raw string) {
	h := HashToken(raw)
	p.mu.Lock()
	if e, ok := p.entries[h]; ok {
		e.attempts++
		if e.attempts >= MaxMFATokenAttempts {
			delete(p.entries, h)
		}
	}
	p.mu.Unlock()
}

// consume discards the token.
func (p *mfaPending) consume(raw string) {
	p.mu.Lock()
	delete(p.entries, HashToken(raw))
	p.mu.Unlock()
}

// dropAccount discards every pending token of the account.
func (p *mfaPending) dropAccount(accountID string) {
	p.mu.Lock()
	for k, e := range p.entries {
		if e.accountID == accountID {
			delete(p.entries, k)
		}
	}
	p.mu.Unlock()
}

// ---- recovery codes ----

// recoveryAlphabet is Crockford's base32 alphabet (no i, l, o, u), lower
// case: 32 symbols, so each random byte maps uniformly via its low 5 bits.
const recoveryAlphabet = "0123456789abcdefghjkmnpqrstvwxyz"

// newRecoveryCodes returns RecoveryCodeCount distinct codes "xxxx-xxxx"
// (40 random bits each) and their storage hashes.
func newRecoveryCodes() (codes, hashes []string, err error) {
	seen := make(map[string]bool, RecoveryCodeCount)
	buf := make([]byte, 8)
	for len(codes) < RecoveryCodeCount {
		if _, err := rand.Read(buf); err != nil {
			return nil, nil, fmt.Errorf("iam: generating recovery codes: %w", err)
		}
		var b strings.Builder
		for i, c := range buf {
			if i == 4 {
				b.WriteByte('-')
			}
			b.WriteByte(recoveryAlphabet[c&31])
		}
		code := b.String()
		if seen[code] {
			continue
		}
		seen[code] = true
		codes = append(codes, code)
		hashes = append(hashes, hashRecoveryCode(code))
	}
	return codes, hashes, nil
}

// normalizeRecoveryCode accepts a recovery code in any case, with or
// without the hyphen and surrounding spaces, and returns the canonical
// "xxxx-xxxx" form.
func normalizeRecoveryCode(code string) (string, bool) {
	code = strings.ToLower(strings.TrimSpace(code))
	code = strings.ReplaceAll(code, "-", "")
	code = strings.ReplaceAll(code, " ", "")
	if len(code) != 8 {
		return "", false
	}
	for i := 0; i < len(code); i++ {
		if strings.IndexByte(recoveryAlphabet, code[i]) < 0 {
			return "", false
		}
	}
	return code[:4] + "-" + code[4:], true
}

func hashRecoveryCode(canonical string) string {
	sum := sha256.Sum256([]byte(canonical))
	return hex.EncodeToString(sum[:])
}

// ---- MFA state ----

// loadMFA returns the account's MFA state, or a fresh zero state (found
// false) when there is none.
func (s *Service) loadMFA(ctx context.Context, accountID string) (*domain.MFAState, bool, error) {
	st, err := s.store.GetMFA(ctx, accountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return &domain.MFAState{AccountID: accountID}, false, nil
		}
		return nil, false, fmt.Errorf("iam: looking up mfa state: %w", err)
	}
	st.AccountID = accountID
	st.RecoveryCodeHashes = append([]string(nil), st.RecoveryCodeHashes...)
	return st, true, nil
}

func (s *Service) saveMFA(ctx context.Context, st *domain.MFAState) error {
	st.RecoveryCodesLeft = len(st.RecoveryCodeHashes)
	if err := s.store.PutMFA(ctx, st); err != nil {
		return fmt.Errorf("iam: storing mfa state: %w", err)
	}
	return nil
}

// checkSecondFactorLocked verifies code against st: a TOTP code (replay
// protected: st.LastUsedStep advances) or, when allowRecovery is set, an
// unused recovery code (removed from st). It mutates st only on success;
// the caller (holding mfaMu) persists it.
func checkSecondFactorLocked(st *domain.MFAState, code string, now time.Time, allowRecovery bool) (bool, error) {
	if _, isTOTP := normalizeTOTPCode(code); isTOTP {
		secret, err := decodeTOTPSecret(st.TOTPSecret)
		if err != nil {
			return false, err
		}
		step, ok := verifyTOTP(secret, code, now, st.LastUsedStep)
		if !ok {
			return false, nil
		}
		st.LastUsedStep = step
		return true, nil
	}
	if !allowRecovery {
		return false, nil
	}
	canon, ok := normalizeRecoveryCode(code)
	if !ok {
		return false, nil
	}
	want := []byte(hashRecoveryCode(canon))
	idx := -1
	for i, h := range st.RecoveryCodeHashes {
		if subtle.ConstantTimeCompare([]byte(h), want) == 1 && idx < 0 {
			idx = i
		}
	}
	if idx < 0 {
		return false, nil
	}
	st.RecoveryCodeHashes = append(st.RecoveryCodeHashes[:idx:idx], st.RecoveryCodeHashes[idx+1:]...)
	return true, nil
}

// verifySecondFactor checks and consumes a second factor of an account
// with TOTP enabled, persisting the replay state. It returns
// errMFANotEnabled when the account has no confirmed TOTP.
func (s *Service) verifySecondFactor(ctx context.Context, accountID, code string, now time.Time, allowRecovery bool) (bool, error) {
	s.mfaMu.Lock()
	defer s.mfaMu.Unlock()
	st, _, err := s.loadMFA(ctx, accountID)
	if err != nil {
		return false, err
	}
	if !st.TOTPEnabled {
		return false, errMFANotEnabled
	}
	ok, err := checkSecondFactorLocked(st, code, now, allowRecovery)
	if err != nil || !ok {
		return false, err
	}
	if err := s.saveMFA(ctx, st); err != nil {
		return false, err
	}
	return true, nil
}

func (s *Service) mfaBlocked(accountID string) error {
	if s.mfaLimiter.blocked(accountID, s.now()) {
		return fmt.Errorf("iam: too many second-factor attempts, try again later: %w", domain.ErrForbidden)
	}
	return nil
}

// MFAStatus returns the account's second-factor configuration with
// RecoveryCodesLeft computed; secrets and code hashes are cleared. An
// account without MFA yields a zero state.
func (s *Service) MFAStatus(ctx context.Context, accountID string) (*domain.MFAState, error) {
	st, found, err := s.loadMFA(ctx, accountID)
	if err != nil {
		return nil, err
	}
	out := &domain.MFAState{AccountID: accountID}
	if !found || !st.TOTPEnabled {
		return out, nil
	}
	out.TOTPEnabled = true
	if st.EnabledAt != nil {
		at := *st.EnabledAt
		out.EnabledAt = &at
	}
	out.RecoveryCodesLeft = len(st.RecoveryCodeHashes)
	return out, nil
}

// BeginTOTPEnroll starts (or restarts) TOTP enrollment: it stores a fresh
// pending secret and returns it (base32) with its otpauth:// URI. The
// secret takes effect only after ConfirmTOTP. Fails with ErrConflict when
// TOTP is already enabled.
func (s *Service) BeginTOTPEnroll(ctx context.Context, accountID string) (string, string, error) {
	acct, err := s.activeAccount(ctx, accountID)
	if err != nil {
		return "", "", err
	}
	secret, err := newTOTPSecret()
	if err != nil {
		return "", "", err
	}
	s.mfaMu.Lock()
	defer s.mfaMu.Unlock()
	st, _, err := s.loadMFA(ctx, acct.ID)
	if err != nil {
		return "", "", err
	}
	if st.TOTPEnabled {
		return "", "", fmt.Errorf("iam: two-factor authentication is already enabled; disable it first: %w", domain.ErrConflict)
	}
	st.PendingSecret = secret
	if err := s.saveMFA(ctx, st); err != nil {
		return "", "", err
	}
	return secret, totpURL(acct.Username, secret), nil
}

// ConfirmTOTP completes enrollment with a code from the pending secret: it
// enables TOTP and returns RecoveryCodeCount fresh recovery codes (shown
// once; only their hashes are stored). A wrong code fails with ErrInvalid
// and counts against the account's MFA rate limit.
func (s *Service) ConfirmTOTP(ctx context.Context, accountID, code string) ([]string, error) {
	acct, err := s.activeAccount(ctx, accountID)
	if err != nil {
		return nil, err
	}
	if err := s.mfaBlocked(acct.ID); err != nil {
		return nil, err
	}
	codes, hashes, err := newRecoveryCodes()
	if err != nil {
		return nil, err
	}
	now := s.now()
	s.mfaMu.Lock()
	defer s.mfaMu.Unlock()
	st, _, err := s.loadMFA(ctx, acct.ID)
	if err != nil {
		return nil, err
	}
	if st.TOTPEnabled {
		return nil, fmt.Errorf("iam: two-factor authentication is already enabled: %w", domain.ErrConflict)
	}
	if st.PendingSecret == "" {
		return nil, fmt.Errorf("iam: no pending totp enrollment; start one first: %w", domain.ErrInvalid)
	}
	secret, err := decodeTOTPSecret(st.PendingSecret)
	if err != nil {
		return nil, err
	}
	step, ok := verifyTOTP(secret, code, now, st.LastUsedStep)
	if !ok {
		s.mfaLimiter.fail(acct.ID, now)
		return nil, fmt.Errorf("iam: invalid authentication code: %w", domain.ErrInvalid)
	}
	enabledAt := now.UTC()
	st.TOTPEnabled = true
	st.TOTPSecret = st.PendingSecret
	st.PendingSecret = ""
	st.LastUsedStep = step
	st.EnabledAt = &enabledAt
	st.RecoveryCodeHashes = hashes
	if err := s.saveMFA(ctx, st); err != nil {
		return nil, err
	}
	s.mfaLimiter.reset(acct.ID)
	return codes, nil
}

// DisableTOTP removes the account's second factor (including a pending
// enrollment and all recovery codes) after re-checking the password. A
// wrong password fails with ErrForbidden (the caller is signed in; this is
// not a session problem) and counts against the MFA rate limit. Disabling
// an account without MFA succeeds.
func (s *Service) DisableTOTP(ctx context.Context, accountID, password string) error {
	acct, err := s.activeAccount(ctx, accountID)
	if err != nil {
		return err
	}
	if err := s.mfaBlocked(acct.ID); err != nil {
		return err
	}
	if acct.PasswordHash == "" {
		return fmt.Errorf("iam: account has no password to confirm with: %w", domain.ErrForbidden)
	}
	ok := false
	if len(password) <= MaxPasswordLen {
		ok, err = s.verifyPassword(ctx, password, acct.PasswordHash)
		if ctxErr := ctx.Err(); ctxErr != nil {
			return ctxErr
		}
	}
	if !ok || err != nil {
		s.mfaLimiter.fail(acct.ID, s.now())
		return fmt.Errorf("iam: password is incorrect: %w", domain.ErrForbidden)
	}
	s.mfaMu.Lock()
	defer s.mfaMu.Unlock()
	if err := s.store.DeleteMFA(ctx, acct.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
		return fmt.Errorf("iam: deleting mfa state: %w", err)
	}
	s.pending.dropAccount(acct.ID)
	return nil
}

// RegenerateRecoveryCodes replaces all recovery codes after verifying a
// current TOTP code (recovery codes are not accepted here). A wrong code
// fails with ErrForbidden and counts against the MFA rate limit; an
// account without TOTP fails with ErrInvalid.
func (s *Service) RegenerateRecoveryCodes(ctx context.Context, accountID, code string) ([]string, error) {
	acct, err := s.activeAccount(ctx, accountID)
	if err != nil {
		return nil, err
	}
	if err := s.mfaBlocked(acct.ID); err != nil {
		return nil, err
	}
	codes, hashes, err := newRecoveryCodes()
	if err != nil {
		return nil, err
	}
	now := s.now()
	s.mfaMu.Lock()
	defer s.mfaMu.Unlock()
	st, _, err := s.loadMFA(ctx, acct.ID)
	if err != nil {
		return nil, err
	}
	if !st.TOTPEnabled {
		return nil, fmt.Errorf("iam: %w", errMFANotEnabled)
	}
	ok, err := checkSecondFactorLocked(st, code, now, false)
	if err != nil {
		return nil, err
	}
	if !ok {
		s.mfaLimiter.fail(acct.ID, now)
		return nil, fmt.Errorf("iam: invalid authentication code: %w", domain.ErrForbidden)
	}
	st.RecoveryCodeHashes = hashes
	if err := s.saveMFA(ctx, st); err != nil {
		return nil, err
	}
	s.mfaLimiter.reset(acct.ID)
	return codes, nil
}
