package meta

import (
	"fmt"
	"slices"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Shared IAM (docs/ENGINEERING_PLAN.md §12) semantics for Bolt and Memory.
//
// In addition to the package-wide conventions:
//
//   - Teams are ordered by slug, team members by account id, an account's
//     team memberships by team id, custom roles and service accounts by
//     name, policies by name (then id), grants of a tree by subject type,
//     subject display name and subject id, a subject's grants by tree id,
//     service-account keys by creation time, invitations newest first and
//     SSO identities by issuer, then subject.
//   - Referential checks: PutTeamMember needs an existing team; PutGrant
//     needs an existing tree (and, for team / service-account subjects, an
//     existing subject); CreateToken with ServiceAccountID needs an existing
//     service account. Violations wrap domain.ErrNotFound.
//   - Cascades: DeleteTeam removes the team's members and grants;
//     DeleteServiceAccount removes its keys and grants; DeleteTree removes
//     the tree's grants.
//   - Store-derived fields: Policy.Version is 1 on create and the stored
//     version + 1 on every update; MFAState.RecoveryCodesLeft is
//     len(RecoveryCodeHashes); SSOConfig.HasSecret is ClientSecret != "".
//     They are written back into the argument.
//   - Secret fields hidden from JSON (MFA secrets and code hashes,
//     Invitation.CodeHash, PasswordReset.CodeHash, SSOConfig.ClientSecret)
//     are persisted and returned by the getters.

// builtinRoles may not be redefined as custom roles.
var builtinRoles = map[string]bool{
	domain.RoleOwner:      true,
	domain.RoleAdmin:      true,
	domain.RoleMaintainer: true,
	domain.RoleDeveloper:  true,
	domain.RoleViewer:     true,
}

func invalid(format string, args ...any) error {
	return fmt.Errorf("meta: "+format+": %w", append(args, domain.ErrInvalid)...)
}

// ---- normalization ----------------------------------------------------

func normTeam(t domain.Team) domain.Team {
	t.CreatedAt = utc(t.CreatedAt)
	t.UpdatedAt = utc(t.UpdatedAt)
	return t
}

func normTeamMember(m domain.TeamMember) domain.TeamMember {
	m.CreatedAt = utc(m.CreatedAt)
	return m
}

func normRole(r domain.CustomRole) domain.CustomRole {
	if r.Permissions == nil {
		r.Permissions = []domain.Action{}
	} else {
		r.Permissions = slices.Clone(r.Permissions)
	}
	r.CreatedAt = utc(r.CreatedAt)
	r.UpdatedAt = utc(r.UpdatedAt)
	return r
}

// cloneOmitEmpty copies s, mapping an empty slice to nil (matching the JSON
// round trip of an omitempty field).
func cloneOmitEmpty(s []string) []string {
	if len(s) == 0 {
		return nil
	}
	return slices.Clone(s)
}

func normStatement(st domain.PolicyStatement) domain.PolicyStatement {
	st.Actions = cloneStrings(st.Actions)
	st.Resources = cloneStrings(st.Resources)
	st.Principals = cloneStrings(st.Principals)
	c := &st.Conditions
	c.IPRanges = cloneOmitEmpty(c.IPRanges)
	c.NotBefore = utcPtr(c.NotBefore)
	c.NotAfter = utcPtr(c.NotAfter)
	if c.TimeWindow != nil {
		tw := *c.TimeWindow
		tw.Days = cloneStrings(tw.Days)
		c.TimeWindow = &tw
	}
	return st
}

func normPolicy(p domain.Policy) domain.Policy {
	sts := make([]domain.PolicyStatement, len(p.Statements))
	for i, st := range p.Statements {
		sts[i] = normStatement(st)
	}
	p.Statements = sts
	p.CreatedAt = utc(p.CreatedAt)
	p.UpdatedAt = utc(p.UpdatedAt)
	return p
}

func normGrant(g domain.Grant) domain.Grant {
	g.CreatedAt = utc(g.CreatedAt)
	return g
}

func normServiceAccount(sa domain.ServiceAccount) domain.ServiceAccount {
	sa.CreatedAt = utc(sa.CreatedAt)
	sa.UpdatedAt = utc(sa.UpdatedAt)
	return sa
}

func normMFA(m domain.MFAState) domain.MFAState {
	m.RecoveryCodeHashes = cloneStrings(m.RecoveryCodeHashes)
	m.RecoveryCodesLeft = len(m.RecoveryCodeHashes)
	m.EnabledAt = utcPtr(m.EnabledAt)
	return m
}

func normInvitation(inv domain.Invitation) domain.Invitation {
	inv.CreatedAt = utc(inv.CreatedAt)
	inv.ExpiresAt = utc(inv.ExpiresAt)
	inv.AcceptedAt = utcPtr(inv.AcceptedAt)
	return inv
}

func normPasswordReset(r domain.PasswordReset) domain.PasswordReset {
	r.CreatedAt = utc(r.CreatedAt)
	r.ExpiresAt = utc(r.ExpiresAt)
	r.UsedAt = utcPtr(r.UsedAt)
	return r
}

func normSSOConfig(c domain.SSOConfig) domain.SSOConfig {
	c.Scopes = cloneStrings(c.Scopes)
	c.AllowedDomains = cloneStrings(c.AllowedDomains)
	c.HasSecret = c.ClientSecret != ""
	c.UpdatedAt = utc(c.UpdatedAt)
	return c
}

func normSSOIdentity(id domain.SSOIdentity) domain.SSOIdentity {
	id.CreatedAt = utc(id.CreatedAt)
	id.LastLoginAt = utc(id.LastLoginAt)
	return id
}

// ---- preparation ------------------------------------------------------

func prepareTeam(t *domain.Team) error {
	if err := checkOptionalKey("team id", t.ID); err != nil {
		return err
	}
	if err := checkKeys("tenant id", t.TenantID, "team slug", t.Slug); err != nil {
		return err
	}
	if t.ID == "" {
		t.ID = newID()
	}
	if t.CreatedAt.IsZero() {
		t.CreatedAt = now()
	}
	if t.UpdatedAt.IsZero() {
		t.UpdatedAt = t.CreatedAt
	}
	return nil
}

// checkTeamUpdate validates an update against the stored team and fills
// the store-managed fields of t.
func checkTeamUpdate(t, stored *domain.Team) error {
	if t.TenantID != stored.TenantID {
		return invalid("team %q belongs to tenant %q, not %q", t.ID, stored.TenantID, t.TenantID)
	}
	if t.CreatedAt.IsZero() {
		t.CreatedAt = stored.CreatedAt
	}
	t.UpdatedAt = now()
	return nil
}

func prepareTeamMember(m *domain.TeamMember) error {
	if err := checkKeys("team id", m.TeamID, "account id", m.AccountID); err != nil {
		return err
	}
	switch m.Role {
	case "":
		m.Role = domain.TeamRoleMember
	case domain.TeamRoleMember, domain.TeamRoleMaintainer:
	default:
		return invalid("unknown team role %q", m.Role)
	}
	return nil
}

func prepareRole(r *domain.CustomRole, stored *domain.CustomRole) error {
	if err := checkKeys("tenant id", r.TenantID, "role name", r.Name); err != nil {
		return err
	}
	if builtinRoles[r.Name] {
		return invalid("role name %q is a built-in role", r.Name)
	}
	t := now()
	if r.CreatedAt.IsZero() {
		if stored != nil {
			r.CreatedAt = stored.CreatedAt
		} else {
			r.CreatedAt = t
		}
	}
	r.UpdatedAt = t
	if r.Permissions == nil {
		r.Permissions = []domain.Action{}
	}
	return nil
}

func preparePolicy(p *domain.Policy) error {
	if err := checkOptionalKey("policy id", p.ID); err != nil {
		return err
	}
	if err := checkKeys("tenant id", p.TenantID, "policy name", p.Name); err != nil {
		return err
	}
	if p.ID == "" {
		p.ID = newID()
	}
	p.Version = 1
	if p.CreatedAt.IsZero() {
		p.CreatedAt = now()
	}
	if p.UpdatedAt.IsZero() {
		p.UpdatedAt = p.CreatedAt
	}
	if p.Statements == nil {
		p.Statements = []domain.PolicyStatement{}
	}
	return nil
}

func checkPolicyUpdate(p, stored *domain.Policy) error {
	if err := checkKey("policy name", p.Name); err != nil {
		return err
	}
	p.Version = stored.Version + 1
	if p.CreatedAt.IsZero() {
		p.CreatedAt = stored.CreatedAt
	}
	if p.CreatedBy == "" {
		p.CreatedBy = stored.CreatedBy
	}
	p.UpdatedAt = now()
	if p.Statements == nil {
		p.Statements = []domain.PolicyStatement{}
	}
	return nil
}

func checkSubjectType(st string) error {
	switch st {
	case domain.SubjectUser, domain.SubjectTeam, domain.SubjectServiceAccount:
		return nil
	case domain.SubjectTenant: // v0.5 access workstream (§13.7): tenant grants
		return nil
	}
	return invalid("unknown grant subject type %q", st)
}

// checkGrant validates the key fields of g.
func checkGrant(g *domain.Grant) error {
	if err := checkKeys("tree id", g.TreeID, "subject id", g.SubjectID, "role", g.Role); err != nil {
		return err
	}
	return checkSubjectType(g.SubjectType)
}

// prepareGrant validates g and fills CreatedAt (from the replaced grant,
// if any).
func prepareGrant(g *domain.Grant, stored *domain.Grant) error {
	if err := checkGrant(g); err != nil {
		return err
	}
	if g.CreatedAt.IsZero() {
		if stored != nil {
			g.CreatedAt = stored.CreatedAt
		} else {
			g.CreatedAt = now()
		}
	}
	return nil
}

func prepareServiceAccount(sa *domain.ServiceAccount) error {
	if err := checkOptionalKey("service account id", sa.ID); err != nil {
		return err
	}
	if err := checkKeys("tenant id", sa.TenantID, "service account name", sa.Name); err != nil {
		return err
	}
	if sa.ID == "" {
		sa.ID = newID()
	}
	if sa.CreatedAt.IsZero() {
		sa.CreatedAt = now()
	}
	if sa.UpdatedAt.IsZero() {
		sa.UpdatedAt = sa.CreatedAt
	}
	return nil
}

func checkServiceAccountUpdate(sa, stored *domain.ServiceAccount) error {
	if sa.TenantID != stored.TenantID {
		return invalid("service account %q belongs to tenant %q, not %q", sa.ID, stored.TenantID, sa.TenantID)
	}
	if sa.CreatedAt.IsZero() {
		sa.CreatedAt = stored.CreatedAt
	}
	if sa.CreatedBy == "" {
		sa.CreatedBy = stored.CreatedBy
	}
	sa.UpdatedAt = now()
	return nil
}

func prepareMFA(m *domain.MFAState) error {
	if err := checkKey("account id", m.AccountID); err != nil {
		return err
	}
	if m.RecoveryCodeHashes == nil {
		m.RecoveryCodeHashes = []string{}
	}
	m.RecoveryCodesLeft = len(m.RecoveryCodeHashes)
	return nil
}

func prepareInvitation(inv *domain.Invitation) error {
	if err := checkOptionalKey("invitation id", inv.ID); err != nil {
		return err
	}
	if err := checkKeys("tenant id", inv.TenantID, "invitation code hash", inv.CodeHash); err != nil {
		return err
	}
	if inv.ID == "" {
		inv.ID = newID()
	}
	if inv.CreatedAt.IsZero() {
		inv.CreatedAt = now()
	}
	return nil
}

// checkInvitationUpdate validates an update; an empty CodeHash keeps the
// stored one.
func checkInvitationUpdate(inv, stored *domain.Invitation) error {
	if err := checkOptionalKey("invitation code hash", inv.CodeHash); err != nil {
		return err
	}
	if inv.CodeHash == "" {
		inv.CodeHash = stored.CodeHash
	}
	if inv.CreatedAt.IsZero() {
		inv.CreatedAt = stored.CreatedAt
	}
	return nil
}

func preparePasswordReset(r *domain.PasswordReset) error {
	if err := checkKeys("password reset code hash", r.CodeHash, "account id", r.AccountID); err != nil {
		return err
	}
	if r.CreatedAt.IsZero() {
		r.CreatedAt = now()
	}
	return nil
}

func prepareSSOConfig(c *domain.SSOConfig) {
	if c.Scopes == nil {
		c.Scopes = []string{}
	}
	if c.AllowedDomains == nil {
		c.AllowedDomains = []string{}
	}
	c.HasSecret = c.ClientSecret != ""
	c.UpdatedAt = now()
}

func prepareSSOIdentity(id *domain.SSOIdentity, stored *domain.SSOIdentity) error {
	if err := checkKeys("sso issuer", id.Issuer, "sso subject", id.Subject, "account id", id.AccountID); err != nil {
		return err
	}
	if id.CreatedAt.IsZero() {
		if stored != nil {
			id.CreatedAt = stored.CreatedAt
		} else {
			id.CreatedAt = now()
		}
	}
	if id.LastLoginAt.IsZero() {
		id.LastLoginAt = id.CreatedAt
	}
	return nil
}

// ---- deterministic orderings ------------------------------------------

func sortGrantsBySubject(gs []domain.Grant) {
	slices.SortFunc(gs, func(a, b domain.Grant) int {
		if c := strings.Compare(a.SubjectType, b.SubjectType); c != 0 {
			return c
		}
		if c := strings.Compare(a.Subject, b.Subject); c != 0 {
			return c
		}
		return strings.Compare(a.SubjectID, b.SubjectID)
	})
}

func sortPolicies(ps []domain.Policy) {
	slices.SortFunc(ps, func(a, b domain.Policy) int {
		if c := strings.Compare(a.Name, b.Name); c != 0 {
			return c
		}
		return strings.Compare(a.ID, b.ID)
	})
}

// sortInvitations orders newest first, ties broken by id.
func sortInvitations(is []domain.Invitation) {
	slices.SortFunc(is, func(a, b domain.Invitation) int {
		if c := b.CreatedAt.Compare(a.CreatedAt); c != 0 {
			return c
		}
		return strings.Compare(a.ID, b.ID)
	})
}

// touchTokenMeta records a token use: LastUsedAt plus the client address and
// user agent when known.
func touchTokenMeta(t *domain.APIToken, at time.Time, lastIP, userAgent string) {
	at = utc(at)
	t.LastUsedAt = &at
	if lastIP != "" {
		t.LastIP = lastIP
	}
	if userAgent != "" {
		t.UserAgent = userAgent
	}
}
