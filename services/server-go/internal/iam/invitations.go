package iam

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Invitation parameters.
const (
	// InvitationCodePrefix starts invitation codes.
	InvitationCodePrefix = "wti_"
	// DefaultInvitationTTL applies when CreateInvitation gets ttl 0.
	DefaultInvitationTTL = 7 * 24 * time.Hour
	// MaxInvitationTTL bounds invitation lifetimes.
	MaxInvitationTTL = 90 * 24 * time.Hour
	// DefaultInvitationRole applies when the invitation names no role.
	DefaultInvitationRole = domain.RoleViewer
)

var errInvalidInvitation = fmt.Errorf("invitation not found: %w", domain.ErrNotFound)

// isBuiltinRole reports whether role is one of the built-in roles.
func isBuiltinRole(role string) bool { return roleRank[role] > 0 }

// resolveRole checks that role is a built-in role or a custom role of the
// tenant and returns its canonical (lower-case) name.
func (s *Service) resolveRole(ctx context.Context, tenantID, role string) (string, error) {
	role = strings.ToLower(strings.TrimSpace(role))
	if isBuiltinRole(role) {
		return role, nil
	}
	if role == "" {
		return "", fmt.Errorf("iam: role is required: %w", domain.ErrInvalid)
	}
	if _, err := s.store.GetRole(ctx, tenantID, role); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return "", fmt.Errorf("iam: unknown role %q: %w", role, domain.ErrInvalid)
		}
		return "", fmt.Errorf("iam: looking up role: %w", err)
	}
	return role, nil
}

// resolveTeam finds a team of the tenant by id or, failing that, by slug.
func (s *Service) resolveTeam(ctx context.Context, tenantID, ref string) (*domain.Team, error) {
	t, err := s.store.GetTeam(ctx, ref)
	if err == nil {
		if t.TenantID != tenantID {
			return nil, fmt.Errorf("iam: team %q does not belong to the tenant: %w", ref, domain.ErrInvalid)
		}
		return t, nil
	}
	if !errors.Is(err, domain.ErrNotFound) {
		return nil, fmt.Errorf("iam: looking up team: %w", err)
	}
	t, err = s.store.GetTeamBySlug(ctx, tenantID, strings.ToLower(ref))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: unknown team %q: %w", ref, domain.ErrInvalid)
		}
		return nil, fmt.Errorf("iam: looking up team: %w", err)
	}
	return t, nil
}

// CreateInvitation validates and stores inv and returns its one-time code
// ("wti_" + 32 random bytes, base64url; only its sha256 is stored).
//
// The caller sets TenantID, optionally Email and/or Username (the account
// that may accept; neither means anyone holding the code), Role (default
// viewer; a built-in role or a custom role of the tenant), optionally
// TeamID (id or slug of a team of the tenant) and InvitedBy (the inviting
// username). Only owners of the tenant and server admins may invite as
// owner. ttl 0 means DefaultInvitationTTL; it must not be negative or
// exceed MaxInvitationTTL. On success inv is updated with the stored
// values (ID, normalized fields, timestamps).
func (s *Service) CreateInvitation(ctx context.Context, inv *domain.Invitation, ttl time.Duration) (string, error) {
	if inv == nil {
		return "", fmt.Errorf("iam: invitation is required: %w", domain.ErrInvalid)
	}
	switch {
	case ttl == 0:
		ttl = DefaultInvitationTTL
	case ttl < 0 || ttl > MaxInvitationTTL:
		return "", fmt.Errorf("iam: invitation ttl must be between 0 and %s: %w", MaxInvitationTTL, domain.ErrInvalid)
	}
	in := *inv
	if in.TenantID == "" {
		return "", fmt.Errorf("iam: tenant is required: %w", domain.ErrInvalid)
	}
	ten, err := s.store.GetTenant(ctx, in.TenantID)
	if err != nil {
		return "", fmt.Errorf("iam: looking up tenant: %w", err)
	}
	in.Email = strings.TrimSpace(in.Email)
	if in.Email != "" {
		if err := ValidateEmail(in.Email); err != nil {
			return "", fmt.Errorf("iam: %w", err)
		}
	}
	in.Username = NormalizeUsername(in.Username)
	if in.Username != "" {
		if err := validateUsername(in.Username, true); err != nil {
			return "", fmt.Errorf("iam: %w", err)
		}
		if _, err := s.store.GetAccountByUsername(ctx, in.Username); err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				return "", fmt.Errorf("iam: unknown user %q: %w", in.Username, domain.ErrInvalid)
			}
			return "", fmt.Errorf("iam: looking up account: %w", err)
		}
	}
	if strings.TrimSpace(in.Role) == "" {
		in.Role = DefaultInvitationRole
	}
	if in.Role, err = s.resolveRole(ctx, ten.ID, in.Role); err != nil {
		return "", err
	}
	if in.Role == domain.RoleOwner {
		if err := s.checkCanGrantOwner(ctx, ten.ID, in.InvitedBy); err != nil {
			return "", err
		}
	}
	if strings.TrimSpace(in.TeamID) != "" {
		team, err := s.resolveTeam(ctx, ten.ID, strings.TrimSpace(in.TeamID))
		if err != nil {
			return "", err
		}
		in.TeamID = team.ID
	} else {
		in.TeamID = ""
	}

	code, h, err := newSecret(InvitationCodePrefix)
	if err != nil {
		return "", err
	}
	now := s.now().UTC()
	in.TenantID = ten.ID
	in.CodeHash = h
	in.CreatedAt = now
	in.ExpiresAt = now.Add(ttl)
	in.AcceptedAt = nil
	in.AcceptedBy = ""
	in.Revoked = false
	if err := s.store.CreateInvitation(ctx, &in); err != nil {
		return "", fmt.Errorf("iam: storing invitation: %w", err)
	}
	*inv = in
	return code, nil
}

// checkCanGrantOwner requires inviter to be a server admin or an owner of
// the tenant.
func (s *Service) checkCanGrantOwner(ctx context.Context, tenantID, inviter string) error {
	forbidden := fmt.Errorf("iam: only tenant owners may invite owners: %w", domain.ErrForbidden)
	if inviter == "" {
		return forbidden
	}
	acct, err := s.store.GetAccountByUsername(ctx, NormalizeUsername(inviter))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return forbidden
		}
		return fmt.Errorf("iam: looking up inviter: %w", err)
	}
	if acct.Disabled {
		return forbidden
	}
	if acct.IsAdmin {
		return nil
	}
	role, err := s.RoleOf(ctx, acct.ID, tenantID)
	if err != nil {
		return err
	}
	if role != domain.RoleOwner {
		return forbidden
	}
	return nil
}

// invitationByCode looks up an invitation by its raw code.
func (s *Service) invitationByCode(ctx context.Context, code string) (*domain.Invitation, error) {
	code = strings.TrimSpace(code)
	if !wellFormedSecret(InvitationCodePrefix, code) {
		return nil, fmt.Errorf("iam: %w", errInvalidInvitation)
	}
	inv, err := s.store.GetInvitationByCodeHash(ctx, HashToken(code))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidInvitation)
		}
		return nil, fmt.Errorf("iam: looking up invitation: %w", err)
	}
	return inv, nil
}

// PreviewInvitation returns the invitation and its tenant for a code, in
// whatever state it is (callers show expired, revoked or accepted
// invitations as such). Unknown or malformed codes yield ErrNotFound.
func (s *Service) PreviewInvitation(ctx context.Context, code string) (*domain.Invitation, *domain.Tenant, error) {
	inv, err := s.invitationByCode(ctx, code)
	if err != nil {
		return nil, nil, err
	}
	ten, err := s.store.GetTenant(ctx, inv.TenantID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil, fmt.Errorf("iam: %w", errInvalidInvitation)
		}
		return nil, nil, fmt.Errorf("iam: looking up tenant: %w", err)
	}
	return inv, ten, nil
}

// AcceptInvitation redeems a code for the signed-in user p: it marks the
// invitation accepted (single use), then adds the tenant membership with
// the invitation's role and, when the invitation names a team that still
// exists, the team membership. An existing membership is never
// downgraded: it is raised only when both roles are built-in and the
// invited role ranks higher; otherwise (including custom roles, whose rank
// is unknown) the existing role is kept. Existing team membership is kept
// as is.
//
// Errors: ErrUnauthenticated (no principal), ErrForbidden (service
// account, disabled account, revoked, expired or already accepted
// invitation, or an invitation addressed to another email/username),
// ErrNotFound (unknown code).
func (s *Service) AcceptInvitation(ctx context.Context, code string, p *domain.Principal) (*domain.Invitation, error) {
	if p == nil {
		return nil, fmt.Errorf("iam: sign in to accept an invitation: %w", domain.ErrUnauthenticated)
	}
	if p.IsServiceAccount() || p.AccountID == "" {
		return nil, fmt.Errorf("iam: service accounts cannot accept invitations: %w", domain.ErrForbidden)
	}
	acct, err := s.store.GetAccount(ctx, p.AccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: unknown account: %w", domain.ErrUnauthenticated)
		}
		return nil, fmt.Errorf("iam: looking up account: %w", err)
	}
	if acct.Disabled {
		return nil, fmt.Errorf("iam: account is disabled: %w", domain.ErrForbidden)
	}

	s.invMu.Lock()
	defer s.invMu.Unlock()
	inv, err := s.invitationByCode(ctx, code)
	if err != nil {
		return nil, err
	}
	now := s.now().UTC()
	switch {
	case inv.Revoked:
		return nil, fmt.Errorf("iam: invitation has been revoked: %w", domain.ErrForbidden)
	case inv.AcceptedAt != nil:
		return nil, fmt.Errorf("iam: invitation has already been accepted: %w", domain.ErrForbidden)
	case !now.Before(inv.ExpiresAt):
		return nil, fmt.Errorf("iam: invitation has expired: %w", domain.ErrForbidden)
	case inv.Email != "" && !strings.EqualFold(strings.TrimSpace(acct.Email), inv.Email):
		return nil, fmt.Errorf("iam: invitation was issued for a different email address: %w", domain.ErrForbidden)
	case inv.Username != "" && acct.Username != inv.Username:
		return nil, fmt.Errorf("iam: invitation was issued for a different user: %w", domain.ErrForbidden)
	}
	if _, err := s.store.GetTenant(ctx, inv.TenantID); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("iam: %w", errInvalidInvitation)
		}
		return nil, fmt.Errorf("iam: looking up tenant: %w", err)
	}

	// Consume first: a failure below must not leave a reusable code.
	inv.AcceptedAt = &now
	inv.AcceptedBy = acct.Username
	if err := s.store.UpdateInvitation(ctx, inv); err != nil {
		return nil, fmt.Errorf("iam: updating invitation: %w", err)
	}

	if err := s.addInvitedMember(ctx, inv, acct.ID, now); err != nil {
		return nil, err
	}
	if inv.TeamID != "" {
		if err := s.addInvitedTeamMember(ctx, inv, acct.ID, now); err != nil {
			return nil, err
		}
	}
	out := *inv
	return &out, nil
}

func (s *Service) addInvitedMember(ctx context.Context, inv *domain.Invitation, accountID string, now time.Time) error {
	role := strings.ToLower(inv.Role)
	cur, err := s.store.GetMembership(ctx, inv.TenantID, accountID)
	switch {
	case err == nil:
		curRole := strings.ToLower(cur.Role)
		if !(isBuiltinRole(curRole) && isBuiltinRole(role) && roleRank[role] > roleRank[curRole]) {
			return nil // keep the existing (equal, higher or incomparable) role
		}
		cur.Role = role
		if err := s.store.PutMembership(ctx, cur); err != nil {
			return fmt.Errorf("iam: updating membership: %w", err)
		}
		return nil
	case errors.Is(err, domain.ErrNotFound):
		if err := s.store.PutMembership(ctx, &domain.Membership{
			TenantID:  inv.TenantID,
			AccountID: accountID,
			Role:      role,
			CreatedAt: now,
		}); err != nil {
			return fmt.Errorf("iam: creating membership: %w", err)
		}
		return nil
	default:
		return fmt.Errorf("iam: looking up membership: %w", err)
	}
}

func (s *Service) addInvitedTeamMember(ctx context.Context, inv *domain.Invitation, accountID string, now time.Time) error {
	team, err := s.store.GetTeam(ctx, inv.TeamID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil // the team was deleted since; the tenant membership stands
		}
		return fmt.Errorf("iam: looking up team: %w", err)
	}
	if team.TenantID != inv.TenantID {
		return nil
	}
	teams, err := s.store.ListAccountTeams(ctx, accountID)
	if err != nil {
		return fmt.Errorf("iam: listing teams: %w", err)
	}
	for _, m := range teams {
		if m.TeamID == team.ID {
			return nil
		}
	}
	if err := s.store.PutTeamMember(ctx, &domain.TeamMember{
		TeamID:    team.ID,
		AccountID: accountID,
		Role:      domain.TeamRoleMember,
		CreatedAt: now,
	}); err != nil {
		return fmt.Errorf("iam: adding team member: %w", err)
	}
	return nil
}
