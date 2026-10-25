// Code generated for compile-only stubs. DO NOT EDIT.
// STUB: replaced by the iam-store workstream (docs/ENGINEERING_PLAN.md §12).

package meta

import (
	"context"
	"errors"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

var errIAMNotImplemented = errors.New("meta: IAM store not implemented")

func (*Bolt) CreateTeam(_ context.Context, _ *domain.Team) error {
	return errIAMNotImplemented
}

func (*Bolt) GetTeam(_ context.Context, _ string) (*domain.Team, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) GetTeamBySlug(_ context.Context, _ string, _ string) (*domain.Team, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListTeams(_ context.Context, _ string) ([]domain.Team, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) UpdateTeam(_ context.Context, _ *domain.Team) error {
	return errIAMNotImplemented
}

func (*Bolt) DeleteTeam(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) PutTeamMember(_ context.Context, _ *domain.TeamMember) error {
	return errIAMNotImplemented
}

func (*Bolt) DeleteTeamMember(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) ListTeamMembers(_ context.Context, _ string) ([]domain.TeamMember, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListAccountTeams(_ context.Context, _ string) ([]domain.TeamMember, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) PutRole(_ context.Context, _ *domain.CustomRole) error {
	return errIAMNotImplemented
}

func (*Bolt) GetRole(_ context.Context, _ string, _ string) (*domain.CustomRole, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListRoles(_ context.Context, _ string) ([]domain.CustomRole, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) DeleteRole(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) CreatePolicy(_ context.Context, _ *domain.Policy) error {
	return errIAMNotImplemented
}

func (*Bolt) GetPolicy(_ context.Context, _ string, _ string) (*domain.Policy, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListPolicies(_ context.Context, _ string) ([]domain.Policy, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) UpdatePolicy(_ context.Context, _ *domain.Policy) error {
	return errIAMNotImplemented
}

func (*Bolt) DeletePolicy(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) PutGrant(_ context.Context, _ *domain.Grant) error {
	return errIAMNotImplemented
}

func (*Bolt) ListGrants(_ context.Context, _ string) ([]domain.Grant, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListSubjectGrants(_ context.Context, _ string, _ string) ([]domain.Grant, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) DeleteGrant(_ context.Context, _ string, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) CreateServiceAccount(_ context.Context, _ *domain.ServiceAccount) error {
	return errIAMNotImplemented
}

func (*Bolt) GetServiceAccount(_ context.Context, _ string) (*domain.ServiceAccount, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) GetServiceAccountByName(_ context.Context, _ string, _ string) (*domain.ServiceAccount, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListServiceAccounts(_ context.Context, _ string) ([]domain.ServiceAccount, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) UpdateServiceAccount(_ context.Context, _ *domain.ServiceAccount) error {
	return errIAMNotImplemented
}

func (*Bolt) DeleteServiceAccount(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) ListServiceAccountTokens(_ context.Context, _ string) ([]domain.APIToken, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) GetMFA(_ context.Context, _ string) (*domain.MFAState, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) PutMFA(_ context.Context, _ *domain.MFAState) error {
	return errIAMNotImplemented
}

func (*Bolt) DeleteMFA(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) CreateInvitation(_ context.Context, _ *domain.Invitation) error {
	return errIAMNotImplemented
}

func (*Bolt) GetInvitation(_ context.Context, _ string, _ string) (*domain.Invitation, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) GetInvitationByCodeHash(_ context.Context, _ string) (*domain.Invitation, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListInvitations(_ context.Context, _ string) ([]domain.Invitation, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) UpdateInvitation(_ context.Context, _ *domain.Invitation) error {
	return errIAMNotImplemented
}

func (*Bolt) DeleteInvitation(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) PutPasswordReset(_ context.Context, _ *domain.PasswordReset) error {
	return errIAMNotImplemented
}

func (*Bolt) GetPasswordReset(_ context.Context, _ string) (*domain.PasswordReset, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) DeletePasswordReset(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Bolt) GetSSOConfig(_ context.Context) (*domain.SSOConfig, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) PutSSOConfig(_ context.Context, _ *domain.SSOConfig) error {
	return errIAMNotImplemented
}

func (*Bolt) PutSSOIdentity(_ context.Context, _ *domain.SSOIdentity) error {
	return errIAMNotImplemented
}

func (*Bolt) GetSSOIdentity(_ context.Context, _ string, _ string) (*domain.SSOIdentity, error) {
	return nil, errIAMNotImplemented
}

func (*Bolt) ListAccountSSOIdentities(_ context.Context, _ string) ([]domain.SSOIdentity, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) CreateTeam(_ context.Context, _ *domain.Team) error {
	return errIAMNotImplemented
}

func (*Memory) GetTeam(_ context.Context, _ string) (*domain.Team, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) GetTeamBySlug(_ context.Context, _ string, _ string) (*domain.Team, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListTeams(_ context.Context, _ string) ([]domain.Team, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) UpdateTeam(_ context.Context, _ *domain.Team) error {
	return errIAMNotImplemented
}

func (*Memory) DeleteTeam(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) PutTeamMember(_ context.Context, _ *domain.TeamMember) error {
	return errIAMNotImplemented
}

func (*Memory) DeleteTeamMember(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) ListTeamMembers(_ context.Context, _ string) ([]domain.TeamMember, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListAccountTeams(_ context.Context, _ string) ([]domain.TeamMember, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) PutRole(_ context.Context, _ *domain.CustomRole) error {
	return errIAMNotImplemented
}

func (*Memory) GetRole(_ context.Context, _ string, _ string) (*domain.CustomRole, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListRoles(_ context.Context, _ string) ([]domain.CustomRole, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) DeleteRole(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) CreatePolicy(_ context.Context, _ *domain.Policy) error {
	return errIAMNotImplemented
}

func (*Memory) GetPolicy(_ context.Context, _ string, _ string) (*domain.Policy, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListPolicies(_ context.Context, _ string) ([]domain.Policy, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) UpdatePolicy(_ context.Context, _ *domain.Policy) error {
	return errIAMNotImplemented
}

func (*Memory) DeletePolicy(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) PutGrant(_ context.Context, _ *domain.Grant) error {
	return errIAMNotImplemented
}

func (*Memory) ListGrants(_ context.Context, _ string) ([]domain.Grant, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListSubjectGrants(_ context.Context, _ string, _ string) ([]domain.Grant, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) DeleteGrant(_ context.Context, _ string, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) CreateServiceAccount(_ context.Context, _ *domain.ServiceAccount) error {
	return errIAMNotImplemented
}

func (*Memory) GetServiceAccount(_ context.Context, _ string) (*domain.ServiceAccount, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) GetServiceAccountByName(_ context.Context, _ string, _ string) (*domain.ServiceAccount, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListServiceAccounts(_ context.Context, _ string) ([]domain.ServiceAccount, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) UpdateServiceAccount(_ context.Context, _ *domain.ServiceAccount) error {
	return errIAMNotImplemented
}

func (*Memory) DeleteServiceAccount(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) ListServiceAccountTokens(_ context.Context, _ string) ([]domain.APIToken, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) GetMFA(_ context.Context, _ string) (*domain.MFAState, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) PutMFA(_ context.Context, _ *domain.MFAState) error {
	return errIAMNotImplemented
}

func (*Memory) DeleteMFA(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) CreateInvitation(_ context.Context, _ *domain.Invitation) error {
	return errIAMNotImplemented
}

func (*Memory) GetInvitation(_ context.Context, _ string, _ string) (*domain.Invitation, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) GetInvitationByCodeHash(_ context.Context, _ string) (*domain.Invitation, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListInvitations(_ context.Context, _ string) ([]domain.Invitation, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) UpdateInvitation(_ context.Context, _ *domain.Invitation) error {
	return errIAMNotImplemented
}

func (*Memory) DeleteInvitation(_ context.Context, _ string, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) PutPasswordReset(_ context.Context, _ *domain.PasswordReset) error {
	return errIAMNotImplemented
}

func (*Memory) GetPasswordReset(_ context.Context, _ string) (*domain.PasswordReset, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) DeletePasswordReset(_ context.Context, _ string) error {
	return errIAMNotImplemented
}

func (*Memory) GetSSOConfig(_ context.Context) (*domain.SSOConfig, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) PutSSOConfig(_ context.Context, _ *domain.SSOConfig) error {
	return errIAMNotImplemented
}

func (*Memory) PutSSOIdentity(_ context.Context, _ *domain.SSOIdentity) error {
	return errIAMNotImplemented
}

func (*Memory) GetSSOIdentity(_ context.Context, _ string, _ string) (*domain.SSOIdentity, error) {
	return nil, errIAMNotImplemented
}

func (*Memory) ListAccountSSOIdentities(_ context.Context, _ string) ([]domain.SSOIdentity, error) {
	return nil, errIAMNotImplemented
}
