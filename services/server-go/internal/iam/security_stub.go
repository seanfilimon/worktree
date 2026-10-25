package iam

// STUB: domain.AccountSecurity for *Service, implemented by the iam-authn
// workstream (docs/ENGINEERING_PLAN.md §12). Delete this file when done.

import (
	"context"
	"errors"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

var errSecurityNotImplemented = errors.New("iam: account security not implemented")

var _ domain.AccountSecurity = (*Service)(nil)

func (s *Service) LoginWithMeta(ctx context.Context, username, password string, _ domain.SessionMeta) (*domain.LoginResult, error) {
	token, acct, err := s.Login(ctx, username, password)
	if err != nil {
		return nil, err
	}
	return &domain.LoginResult{Token: token, Account: acct}, nil
}

func (s *Service) CompleteMFALogin(context.Context, string, string, domain.SessionMeta) (string, *domain.Account, error) {
	return "", nil, errSecurityNotImplemented
}

func (s *Service) IssueSession(context.Context, string, bool, domain.SessionMeta) (string, error) {
	return "", errSecurityNotImplemented
}

func (s *Service) MFAStatus(_ context.Context, accountID string) (*domain.MFAState, error) {
	return &domain.MFAState{AccountID: accountID}, nil
}

func (s *Service) BeginTOTPEnroll(context.Context, string) (string, string, error) {
	return "", "", errSecurityNotImplemented
}

func (s *Service) ConfirmTOTP(context.Context, string, string) ([]string, error) {
	return nil, errSecurityNotImplemented
}

func (s *Service) DisableTOTP(context.Context, string, string) error {
	return errSecurityNotImplemented
}

func (s *Service) RegenerateRecoveryCodes(context.Context, string, string) ([]string, error) {
	return nil, errSecurityNotImplemented
}

func (s *Service) ListSessions(context.Context, string) ([]domain.APIToken, error) {
	return nil, errSecurityNotImplemented
}

func (s *Service) RevokeOtherSessions(context.Context, string, string) (int, error) {
	return 0, errSecurityNotImplemented
}

func (s *Service) CreateScopedToken(context.Context, string, string, time.Duration, []string) (string, *domain.APIToken, error) {
	return "", nil, errSecurityNotImplemented
}

func (s *Service) CreateServiceAccountKey(context.Context, string, string, time.Duration, []string) (string, *domain.APIToken, error) {
	return "", nil, errSecurityNotImplemented
}

func (s *Service) RevokeServiceAccountKey(context.Context, string, string) error {
	return errSecurityNotImplemented
}

func (s *Service) CreateInvitation(context.Context, *domain.Invitation, time.Duration) (string, error) {
	return "", errSecurityNotImplemented
}

func (s *Service) PreviewInvitation(context.Context, string) (*domain.Invitation, *domain.Tenant, error) {
	return nil, nil, errSecurityNotImplemented
}

func (s *Service) AcceptInvitation(context.Context, string, *domain.Principal) (*domain.Invitation, error) {
	return nil, errSecurityNotImplemented
}

func (s *Service) CreatePasswordReset(context.Context, string, string, time.Duration) (string, time.Time, error) {
	return "", time.Time{}, errSecurityNotImplemented
}

func (s *Service) ResetPassword(context.Context, string, string) error {
	return errSecurityNotImplemented
}
