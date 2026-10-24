package iam

import (
	"context"
	"errors"
	"fmt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// EnsureAdmin creates (or promotes) the server admin account. It is
// idempotent and intended to run at every server start:
//
//   - If no account named username exists, it is created like Register
//     would (personal tenant + owner membership, email "<username>@localhost")
//     but regardless of Options.AllowRegistration, and marked IsAdmin. A
//     password is required in that case.
//   - If the account exists, IsAdmin is set, and when password is non-empty
//     and differs from the current one, the password is reset (existing
//     tokens stay valid). The Disabled flag is left untouched.
//
// The reserved name "admin" is accepted here (and only here). When auth is
// not a *Service, account creation goes through auth.Register and is
// therefore subject to that implementation's registration policy.
func EnsureAdmin(ctx context.Context, store domain.MetaStore, auth domain.Authenticator, username, password string) error {
	username = NormalizeUsername(username)
	if err := validateUsername(username, true); err != nil {
		return fmt.Errorf("iam: admin account: %w", err)
	}
	if password != "" {
		if err := ValidatePassword(password); err != nil {
			return fmt.Errorf("iam: admin account: %w", err)
		}
	}

	acct, err := store.GetAccountByUsername(ctx, username)
	if errors.Is(err, domain.ErrNotFound) {
		if password == "" {
			return fmt.Errorf("iam: admin account %q does not exist and no password was given: %w", username, domain.ErrInvalid)
		}
		email := username + "@localhost"
		var created *domain.Account
		var cerr error
		if svc, ok := auth.(*Service); ok {
			created, cerr = svc.register(ctx, username, email, password, true)
		} else {
			created, cerr = auth.Register(ctx, username, email, password)
		}
		switch {
		case cerr == nil && created.IsAdmin:
			return nil
		case cerr == nil:
			acct = created
		case errors.Is(cerr, domain.ErrAlreadyExists):
			// Lost a race with a concurrent creation, or the email is taken:
			// fall back to promoting the account if it now exists.
			acct, err = store.GetAccountByUsername(ctx, username)
			if err != nil {
				return fmt.Errorf("iam: creating admin account: %w", cerr)
			}
		default:
			return fmt.Errorf("iam: creating admin account: %w", cerr)
		}
	} else if err != nil {
		return fmt.Errorf("iam: looking up admin account: %w", err)
	}

	if !acct.IsAdmin {
		acct.IsAdmin = true
		if err := store.UpdateAccount(ctx, acct); err != nil {
			return fmt.Errorf("iam: promoting admin account: %w", err)
		}
	}
	if password != "" {
		if ok, verr := VerifyPassword(password, acct.PasswordHash); verr == nil && ok {
			return nil
		}
		if err := auth.SetPassword(ctx, acct.ID, password); err != nil {
			return fmt.Errorf("iam: resetting admin password: %w", err)
		}
	}
	return nil
}
