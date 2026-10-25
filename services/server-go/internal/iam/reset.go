package iam

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Password reset parameters.
const (
	// ResetCodePrefix starts password-reset codes.
	ResetCodePrefix = "wtr_"
	// DefaultResetTTL applies when CreatePasswordReset gets ttl 0.
	DefaultResetTTL = time.Hour
	// MaxResetTTL bounds reset-code lifetimes.
	MaxResetTTL = 7 * 24 * time.Hour
)

// errInvalidReset is the single error for every unusable reset code
// (malformed, unknown, expired, used), so the public endpoint reveals
// nothing about which.
var errInvalidReset = fmt.Errorf("invalid, expired or already used reset code: %w", domain.ErrInvalid)

// CreatePasswordReset issues a one-time password-reset code ("wtr_" + 32
// random bytes, base64url; only its sha256 is stored) for accountID,
// recorded as created by createdBy (the admin's username). ttl 0 means
// DefaultResetTTL; it must not be negative or exceed MaxResetTTL. Errors:
// ErrNotFound (unknown account), ErrInvalid (bad ttl).
func (s *Service) CreatePasswordReset(ctx context.Context, accountID, createdBy string, ttl time.Duration) (string, time.Time, error) {
	switch {
	case ttl == 0:
		ttl = DefaultResetTTL
	case ttl < 0 || ttl > MaxResetTTL:
		return "", time.Time{}, fmt.Errorf("iam: reset ttl must be between 0 and %s: %w", MaxResetTTL, domain.ErrInvalid)
	}
	acct, err := s.store.GetAccount(ctx, accountID)
	if err != nil {
		return "", time.Time{}, fmt.Errorf("iam: looking up account: %w", err)
	}
	code, h, err := newSecret(ResetCodePrefix)
	if err != nil {
		return "", time.Time{}, err
	}
	now := s.now().UTC()
	r := &domain.PasswordReset{
		CodeHash:  h,
		AccountID: acct.ID,
		CreatedBy: createdBy,
		CreatedAt: now,
		ExpiresAt: now.Add(ttl),
	}
	if err := s.store.PutPasswordReset(ctx, r); err != nil {
		return "", time.Time{}, fmt.Errorf("iam: storing password reset: %w", err)
	}
	return code, r.ExpiresAt, nil
}

// ResetPassword redeems a reset code: it validates the new password
// (ErrInvalid), marks the code used, sets the password and revokes every
// token of the account (sessions, API tokens and pending MFA logins). Any
// unusable code yields the same ErrInvalid. The account's second factor is
// left in place.
func (s *Service) ResetPassword(ctx context.Context, code, newPassword string) error {
	if err := ValidatePassword(newPassword); err != nil {
		return fmt.Errorf("iam: %w", err)
	}
	code = strings.TrimSpace(code)
	if !wellFormedSecret(ResetCodePrefix, code) {
		return fmt.Errorf("iam: %w", errInvalidReset)
	}
	h := HashToken(code)
	// Hash before taking the lock: it is the expensive part.
	pwHash, err := s.hashPassword(ctx, newPassword)
	if err != nil {
		return err
	}

	s.resetMu.Lock()
	defer s.resetMu.Unlock()
	r, err := s.store.GetPasswordReset(ctx, h)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("iam: %w", errInvalidReset)
		}
		return fmt.Errorf("iam: looking up password reset: %w", err)
	}
	now := s.now().UTC()
	if r.UsedAt != nil || !now.Before(r.ExpiresAt) {
		return fmt.Errorf("iam: %w", errInvalidReset)
	}
	acct, err := s.store.GetAccount(ctx, r.AccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("iam: %w", errInvalidReset)
		}
		return fmt.Errorf("iam: looking up account: %w", err)
	}
	// Consume first: a failure below must not leave a reusable code.
	r.UsedAt = &now
	if err := s.store.PutPasswordReset(ctx, r); err != nil {
		return fmt.Errorf("iam: updating password reset: %w", err)
	}
	acct.PasswordHash = pwHash
	if err := s.store.UpdateAccount(ctx, acct); err != nil {
		return fmt.Errorf("iam: updating account: %w", err)
	}
	return s.revokeAllTokens(ctx, acct.ID)
}
