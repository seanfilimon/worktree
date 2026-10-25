package iam

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// roleRank orders the built-in roles; higher ranks include all permissions
// of lower ones. Unknown roles rank 0 and grant nothing.
var roleRank = map[string]int{
	domain.RoleViewer:     1,
	domain.RoleDeveloper:  2,
	domain.RoleMaintainer: 3,
	domain.RoleAdmin:      4,
	domain.RoleOwner:      5,
}

// actionMinRole is the least privileged built-in role granting each
// tenant-scoped action (see the table in doc.go). Actions absent from the
// map (server:admin, unknown verbs) are never granted by a role.
var actionMinRole = map[domain.Action]string{
	domain.ActTenantRead:   domain.RoleViewer,
	domain.ActTreeRead:     domain.RoleViewer,
	domain.ActTreeWrite:    domain.RoleDeveloper,
	domain.ActBranchCreate: domain.RoleDeveloper,
	domain.ActMRCreate:     domain.RoleDeveloper,
	domain.ActBranchDelete: domain.RoleMaintainer,
	domain.ActMRApprove:    domain.RoleMaintainer,
	domain.ActMRMerge:      domain.RoleMaintainer,
	domain.ActTreeCreate:   domain.RoleMaintainer,
	domain.ActTreeAdmin:    domain.RoleAdmin,
	domain.ActTenantAdmin:  domain.RoleAdmin,
}

// RoleAllows reports whether the built-in role grants the tenant-scoped
// action. It ignores server-admin status and tree visibility.
func RoleAllows(role string, act domain.Action) bool {
	minRole, ok := actionMinRole[act]
	if !ok {
		return false
	}
	r := roleRank[strings.ToLower(role)]
	return r > 0 && r >= roleRank[minRole]
}

// RoleAtLeast reports whether role is a built-in role at least as
// privileged as min (e.g. RoleAtLeast("owner", "admin") is true).
func RoleAtLeast(role, min string) bool {
	r := roleRank[strings.ToLower(role)]
	return r > 0 && r >= roleRank[strings.ToLower(min)]
}

// Authorize decides whether p may perform act in tenantID (and on tree, when
// non-nil). When tree is given and tenantID is empty, the tree's tenant is
// used; a tree from a different tenant than tenantID is refused. The
// principal's account is re-read from the store so that admin promotion,
// demotion and disabling take effect immediately.
func (s *Service) Authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	if tree != nil {
		switch {
		case tenantID == "":
			tenantID = tree.TenantID
		case tenantID != tree.TenantID:
			return deny(p, act, "tree does not belong to tenant")
		}
	}

	if p == nil {
		if act == domain.ActTreeRead && tree != nil && tree.Visibility == domain.VisibilityPublic {
			return nil
		}
		return deny(nil, act, "authentication required")
	}
	if !ScopeAllows(p.Scopes, act) {
		return deny(p, act, "token scope does not permit it")
	}
	if p.IsServiceAccount() {
		return s.authorizeServiceAccount(ctx, p, act, tenantID, tree)
	}

	acct, err := s.store.GetAccount(ctx, p.AccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("iam: unknown account: %w", domain.ErrUnauthenticated)
		}
		return fmt.Errorf("iam: looking up account: %w", err)
	}
	if acct.Disabled {
		return deny(p, act, "account is disabled")
	}
	if acct.IsAdmin {
		return nil
	}
	if act == domain.ActServerAdmin {
		return deny(p, act, "server admin required")
	}
	if act == domain.ActTreeRead && tree != nil &&
		(tree.Visibility == domain.VisibilityPublic || tree.Visibility == domain.VisibilityInternal) {
		return nil
	}
	if tenantID == "" {
		return deny(p, act, "no tenant given")
	}
	role, err := s.RoleOf(ctx, acct.ID, tenantID)
	if err != nil {
		return err
	}
	if !RoleAllows(role, act) {
		if role == "" {
			return deny(p, act, "not a member of the tenant")
		}
		return deny(p, act, "role "+role+" does not permit it")
	}
	return nil
}

// authorizeServiceAccount applies the built-in role matrix to a service
// account: its Role grants actions in its own tenant only; tree visibility
// widens tree:read as for users. Disabled or deleted service accounts are
// denied.
func (s *Service) authorizeServiceAccount(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	sa, err := s.store.GetServiceAccount(ctx, p.ServiceAccountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("iam: unknown service account: %w", domain.ErrUnauthenticated)
		}
		return fmt.Errorf("iam: looking up service account: %w", err)
	}
	if sa.Disabled {
		return deny(p, act, "service account is disabled")
	}
	if act == domain.ActServerAdmin {
		return deny(p, act, "server admin required")
	}
	if act == domain.ActTreeRead && tree != nil &&
		(tree.Visibility == domain.VisibilityPublic || tree.Visibility == domain.VisibilityInternal) {
		return nil
	}
	if tenantID == "" || tenantID != sa.TenantID {
		return deny(p, act, "service account does not belong to the tenant")
	}
	if !RoleAllows(sa.Role, act) {
		return deny(p, act, "role "+sa.Role+" does not permit it")
	}
	return nil
}

// RoleOf returns the account's role in the tenant, or "" if it is not a
// member.
func (s *Service) RoleOf(ctx context.Context, accountID, tenantID string) (string, error) {
	m, err := s.store.GetMembership(ctx, tenantID, accountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return "", nil
		}
		return "", fmt.Errorf("iam: looking up membership: %w", err)
	}
	return strings.ToLower(m.Role), nil
}

// deny builds the refusal error: ErrUnauthenticated for anonymous callers,
// ErrForbidden otherwise.
func deny(p *domain.Principal, act domain.Action, reason string) error {
	if p == nil {
		return fmt.Errorf("iam: %s: %s: %w", act, reason, domain.ErrUnauthenticated)
	}
	return fmt.Errorf("iam: %s: %s: %w", act, reason, domain.ErrForbidden)
}
