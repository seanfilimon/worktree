package iam

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ServiceAccountPrefix starts the Username of service-account principals:
// "sa:<tenant slug>/<name>".
const ServiceAccountPrefix = "sa:"

// defaultKeyName names service-account keys created without a name.
const defaultKeyName = "key"

// ServiceAccountUsername returns the principal username of a service
// account: "sa:<tenantSlug>/<name>".
func ServiceAccountUsername(tenantSlug, name string) string {
	return ServiceAccountPrefix + tenantSlug + "/" + name
}

// serviceAccountPrincipal resolves a service-account key. The key must
// name a service account (and no personal account); the service account
// and its tenant must exist and the service account must be enabled, else
// the error wraps ErrUnauthenticated.
func (s *Service) serviceAccountPrincipal(ctx context.Context, tok *domain.APIToken) (*domain.Principal, error) {
	if tok.ServiceAccountID == "" || tok.AccountID != "" {
		return nil, fmt.Errorf("iam: %w", errInvalidToken)
	}
	sa, err := s.store.GetServiceAccount(ctx, tok.ServiceAccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidToken)
		}
		return nil, fmt.Errorf("iam: looking up service account: %w", err)
	}
	if sa.Disabled {
		return nil, fmt.Errorf("iam: service account is disabled: %w", domain.ErrUnauthenticated)
	}
	ten, err := s.store.GetTenant(ctx, sa.TenantID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidToken)
		}
		return nil, fmt.Errorf("iam: looking up tenant: %w", err)
	}
	return &domain.Principal{
		Username:         ServiceAccountUsername(ten.Slug, sa.Name),
		TenantID:         sa.TenantID,
		Kind:             domain.PrincipalServiceAccount,
		ServiceAccountID: sa.ID,
	}, nil
}

// CreateServiceAccountKey issues a key (APIToken of Kind service, no
// AccountID) for an enabled service account, optionally restricted to
// scopes (see ValidateScopes). An empty name becomes "key"; ttl 0 never
// expires; negative ttl is invalid. Errors: ErrNotFound (unknown service
// account), ErrForbidden (disabled), ErrInvalid (bad input).
func (s *Service) CreateServiceAccountKey(ctx context.Context, serviceAccountID, name string, ttl time.Duration, scopes []string) (string, *domain.APIToken, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		name = defaultKeyName
	}
	if err := validateTokenName(name); err != nil {
		return "", nil, fmt.Errorf("iam: %w", err)
	}
	if ttl < 0 {
		return "", nil, fmt.Errorf("iam: key ttl must not be negative: %w", domain.ErrInvalid)
	}
	sc, err := ValidateScopes(scopes)
	if err != nil {
		return "", nil, fmt.Errorf("iam: %w", err)
	}
	if serviceAccountID == "" {
		return "", nil, fmt.Errorf("iam: service account id is required: %w", domain.ErrInvalid)
	}
	sa, err := s.store.GetServiceAccount(ctx, serviceAccountID)
	if err != nil {
		return "", nil, fmt.Errorf("iam: looking up service account: %w", err)
	}
	if sa.Disabled {
		return "", nil, fmt.Errorf("iam: service account is disabled: %w", domain.ErrForbidden)
	}
	return s.issue(ctx, &domain.APIToken{
		ServiceAccountID: sa.ID,
		Name:             name,
		Kind:             domain.TokenService,
		Scopes:           sc,
	}, ttl)
}

// RevokeServiceAccountKey deletes key tokenID of the service account. A
// key belonging to another service account (or no key at all) is reported
// as ErrNotFound.
func (s *Service) RevokeServiceAccountKey(ctx context.Context, serviceAccountID, tokenID string) error {
	if serviceAccountID == "" || tokenID == "" {
		return fmt.Errorf("iam: key %q: %w", tokenID, domain.ErrNotFound)
	}
	keys, err := s.store.ListServiceAccountTokens(ctx, serviceAccountID)
	if err != nil {
		return fmt.Errorf("iam: listing service account keys: %w", err)
	}
	for _, k := range keys {
		if k.ID == tokenID && k.ServiceAccountID == serviceAccountID {
			if err := s.store.DeleteToken(ctx, tokenID); err != nil {
				return fmt.Errorf("iam: deleting key: %w", err)
			}
			s.touchMu.Lock()
			delete(s.touched, tokenID)
			s.touchMu.Unlock()
			return nil
		}
	}
	return fmt.Errorf("iam: key %q: %w", tokenID, domain.ErrNotFound)
}
