package httpapi

import (
	"bytes"
	"context"
	"errors"
	"io"
	"net/http"
	"regexp"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// registerIAMRoutes registers the IAM endpoints of ENGINEERING_PLAN §12:
// teams, custom roles, policies, tree grants, service accounts and keys,
// scoped tokens, sessions, MFA, invitations, password resets, the policy
// simulator and effective permissions. tree is the
// "/api/v1/trees/{tenant}/{tree}" prefix.
//
// Tenant-scoped reads need iam:read on the tenant, mutations iam:admin
// (team maintainers may additionally manage their own team's members).
// Account-security endpoints act on the signed-in user. Every mutation is
// audited.
func (a *api) registerIAMRoutes(tree string) {
	const t = "/api/v1/tenants/{tenant}"

	// Action catalog.
	a.handle("GET /api/v1/iam/actions", a.handleIAMActions)

	// Teams.
	a.handle("GET "+t+"/teams", a.handleListTeams)
	a.handle("POST "+t+"/teams", a.handleCreateTeam)
	a.handle("GET "+t+"/teams/{team}", a.handleGetTeam)
	a.handle("PATCH "+t+"/teams/{team}", a.handlePatchTeam)
	a.handle("DELETE "+t+"/teams/{team}", a.handleDeleteTeam)
	a.handle("PUT "+t+"/teams/{team}/members/{username}", a.handlePutTeamMember)
	a.handle("DELETE "+t+"/teams/{team}/members/{username}", a.handleDeleteTeamMember)

	// Custom roles.
	a.handle("GET "+t+"/roles", a.handleListRoles)
	a.handle("POST "+t+"/roles", a.handleCreateRole)
	a.handle("GET "+t+"/roles/{name}", a.handleGetRole)
	a.handle("PATCH "+t+"/roles/{name}", a.handlePatchRole)
	a.handle("DELETE "+t+"/roles/{name}", a.handleDeleteRole)

	// Policies.
	a.handle("GET "+t+"/policies", a.handleListPolicies)
	a.handle("POST "+t+"/policies", a.handleCreatePolicy)
	a.handle("POST "+t+"/policies/validate", a.handleValidatePolicy)
	a.handle("GET "+t+"/policies/{id}", a.handleGetPolicy)
	a.handle("PUT "+t+"/policies/{id}", a.handlePutPolicy)
	a.handle("DELETE "+t+"/policies/{id}", a.handleDeletePolicy)

	// Tree grants.
	a.handle("GET "+tree+"/grants", a.handleListGrants)
	a.handle("PUT "+tree+"/grants", a.handlePutGrant)
	a.handle("DELETE "+tree+"/grants", a.handleDeleteGrant)

	// Service accounts and keys.
	a.handle("GET "+t+"/service-accounts", a.handleListServiceAccounts)
	a.handle("POST "+t+"/service-accounts", a.handleCreateServiceAccount)
	a.handle("GET "+t+"/service-accounts/{name}", a.handleGetServiceAccount)
	a.handle("PATCH "+t+"/service-accounts/{name}", a.handlePatchServiceAccount)
	a.handle("DELETE "+t+"/service-accounts/{name}", a.handleDeleteServiceAccount)
	a.handle("POST "+t+"/service-accounts/{name}/keys", a.handleCreateServiceAccountKey)
	a.handle("DELETE "+t+"/service-accounts/{name}/keys/{id}", a.handleDeleteServiceAccountKey)

	// Sessions and MFA (the signed-in account).
	a.handle("POST /api/v1/auth/login/mfa", a.handleLoginMFA)
	a.handle("GET /api/v1/auth/sessions", a.handleListSessions)
	a.handle("DELETE /api/v1/auth/sessions/{id}", a.handleRevokeSession)
	a.handle("POST /api/v1/auth/sessions/revoke-others", a.handleRevokeOtherSessions)
	a.handle("GET /api/v1/auth/mfa", a.handleMFAStatus)
	a.handle("POST /api/v1/auth/mfa/totp/enroll", a.handleMFAEnroll)
	a.handle("POST /api/v1/auth/mfa/totp/confirm", a.handleMFAConfirm)
	a.handle("DELETE /api/v1/auth/mfa/totp", a.handleMFADisable)
	a.handle("POST /api/v1/auth/mfa/recovery-codes", a.handleMFARecoveryCodes)

	// Invitations.
	a.handle("GET "+t+"/invitations", a.handleListInvitations)
	a.handle("POST "+t+"/invitations", a.handleCreateInvitation)
	a.handle("DELETE "+t+"/invitations/{id}", a.handleRevokeInvitation)
	a.handle("GET /api/v1/invitations/{code}", a.handlePreviewInvitation)
	a.handle("POST /api/v1/invitations/{code}/accept", a.handleAcceptInvitation)

	// Password resets.
	a.handle("POST /api/v1/admin/accounts/{username}/reset-code", a.handleAdminResetCode)
	a.handle("POST /api/v1/auth/password-reset", a.handlePasswordReset)

	// Simulator and effective permissions.
	a.handle("POST /api/v1/iam/simulate", a.handleIAMSimulate)
	a.handle("GET /api/v1/iam/effective", a.handleIAMEffective)
}

// ---- shared IAM helpers ----

// iamNameRe is the syntax of team slugs, custom role names and service
// account names.
var iamNameRe = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{1,39}$`)

func validateIAMName(field, s string) error {
	if !iamNameRe.MatchString(s) {
		return badRequestf("%s must be 2-40 characters of lowercase letters, digits and '-', starting with a letter or digit", field)
	}
	return nil
}

// errIAMNotImplemented answers 501 when an optional collaborator is absent.
func errIAMNotImplemented(feature string) error {
	return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
		msg: feature + " is not available on this server"}
}

// security returns Deps.Security or a 501 error.
func (a *api) security() (domain.AccountSecurity, error) {
	if a.d.Security == nil {
		return nil, errIAMNotImplemented("account security")
	}
	return a.d.Security, nil
}

// iamTenant requires a signed-in caller, resolves {tenant} and authorizes
// act (iam:read / iam:admin) on it.
func (a *api) iamTenant(r *http.Request, act domain.Action) (*domain.Tenant, error) {
	if _, err := requireLogin(r); err != nil {
		return nil, err
	}
	return a.tenantFor(r, act)
}

// requireUser returns the signed-in human caller: anonymous callers get
// 401, service accounts 403.
func requireUser(r *http.Request) (*domain.Principal, error) {
	p, err := requireLogin(r)
	if err != nil {
		return nil, err
	}
	if p.IsServiceAccount() || p.AccountID == "" {
		return nil, forbiddenf("service accounts cannot use this endpoint")
	}
	return p, nil
}

// requireFullUser is requireUser for account-security changes, which
// scoped API tokens may not perform.
func requireFullUser(r *http.Request) (*domain.Principal, error) {
	p, err := requireUser(r)
	if err != nil {
		return nil, err
	}
	if len(p.Scopes) > 0 {
		return nil, forbiddenf("scoped tokens cannot manage account security")
	}
	return p, nil
}

// checkTenantRole validates that role is a built-in role or an existing
// custom role of tenantID.
func (a *api) checkTenantRole(ctx context.Context, tenantID, role string) error {
	if role == "" {
		return badRequestf("role is required")
	}
	if isBuiltinRole(role) {
		return nil
	}
	if !iamNameRe.MatchString(role) {
		return badRequestf("role %q is not a built-in role or a custom role of the tenant", role)
	}
	_, err := a.d.Store.GetRole(ctx, tenantID, role)
	if errors.Is(err, domain.ErrNotFound) {
		return badRequestf("role %q is not a built-in role or a custom role of the tenant", role)
	}
	return err
}

// checkOwnerAssign allows granting the built-in owner role only to tenant
// owners and server admins.
func (a *api) checkOwnerAssign(r *http.Request, tenantID, role string) error {
	if role != domain.RoleOwner {
		return nil
	}
	p := principal(r)
	if p.IsAdmin {
		return nil
	}
	cur, err := a.d.Authz.RoleOf(r.Context(), p.AccountID, tenantID)
	if err != nil {
		return err
	}
	if cur != domain.RoleOwner {
		return forbiddenf("only tenant owners can assign the owner role")
	}
	return nil
}

// accountByUsername resolves a username, answering 404 when unknown.
func (a *api) accountByUsername(ctx context.Context, username string) (*domain.Account, error) {
	acct, err := a.d.Store.GetAccountByUsername(ctx, username)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("account %q not found", username)
	}
	return acct, err
}

// decodeOptionalJSON decodes the body into dst when there is one; an empty
// body leaves dst untouched.
func decodeOptionalJSON(w http.ResponseWriter, r *http.Request, dst any) error {
	if r.Body == nil || r.Body == http.NoBody {
		return nil
	}
	buf, err := io.ReadAll(http.MaxBytesReader(w, r.Body, maxJSONBody))
	if err != nil {
		return jsonDecodeError(err)
	}
	if len(bytes.TrimSpace(buf)) == 0 {
		return nil
	}
	r.Body = io.NopCloser(bytes.NewReader(buf))
	return decodeJSON(w, r, dst)
}

// trimPtr trims *s in place when s is non-nil.
func trimPtr(s *string) {
	if s != nil {
		*s = strings.TrimSpace(*s)
	}
}

// handleIAMActions answers GET /api/v1/iam/actions (public).
func (a *api) handleIAMActions(w http.ResponseWriter, r *http.Request) error {
	writeJSON(w, http.StatusOK, nonNil(iamCatalog.Actions()))
	return nil
}
