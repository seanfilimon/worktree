package iam

import (
	"context"
	"fmt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// CreateAccount creates an account on behalf of a server administrator:
// like Register it creates the personal tenant (slug = username, kind user,
// plan free) and the owner membership, but it ignores
// Options.AllowRegistration and can mark the account as a server admin.
//
// Reserved usernames (including "admin") are rejected; the bootstrap admin
// is created with EnsureAdmin. Errors wrap ErrInvalid for bad input and
// ErrAlreadyExists when the username, tenant slug or email is taken.
func (s *Service) CreateAccount(ctx context.Context, username, email, password string, isAdmin bool) (*domain.Account, error) {
	username = NormalizeUsername(username)
	// register(asAdmin=true) would accept the reserved "admin" name, so
	// validate with the regular rules first.
	if err := validateUsername(username, false); err != nil {
		return nil, fmt.Errorf("iam: %w", err)
	}
	return s.register(ctx, username, email, password, isAdmin)
}
