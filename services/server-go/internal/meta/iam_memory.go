package meta

import (
	"cmp"
	"context"
	"slices"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// memIAM holds Memory's IAM state. It is guarded by Memory.mu.
type memIAM struct {
	teams        map[string]domain.Team
	teamBySlug   map[string]string                       // key(tenant, slug) -> team id
	teamMembers  map[string]map[string]domain.TeamMember // team -> account
	roles        map[string]map[string]domain.CustomRole // tenant -> name
	policies     map[string]map[string]domain.Policy     // tenant -> id
	grants       map[string]map[string]domain.Grant      // tree -> key(subject type, subject id)
	sas          map[string]domain.ServiceAccount
	saByName     map[string]string // key(tenant, name) -> service account id
	mfa          map[string]domain.MFAState
	invites      map[string]map[string]domain.Invitation // tenant -> id
	inviteByCode map[string]inviteRef                    // code hash
	resets       map[string]domain.PasswordReset         // code hash
	sso          *domain.SSOConfig
	ssoIdents    map[string]domain.SSOIdentity // key(issuer, subject)
}

type inviteRef struct{ tenantID, id string }

func newMemIAM() memIAM {
	return memIAM{
		teams:        map[string]domain.Team{},
		teamBySlug:   map[string]string{},
		teamMembers:  map[string]map[string]domain.TeamMember{},
		roles:        map[string]map[string]domain.CustomRole{},
		policies:     map[string]map[string]domain.Policy{},
		grants:       map[string]map[string]domain.Grant{},
		sas:          map[string]domain.ServiceAccount{},
		saByName:     map[string]string{},
		mfa:          map[string]domain.MFAState{},
		invites:      map[string]map[string]domain.Invitation{},
		inviteByCode: map[string]inviteRef{},
		resets:       map[string]domain.PasswordReset{},
		ssoIdents:    map[string]domain.SSOIdentity{},
	}
}

// ---- teams ------------------------------------------------------------

// CreateTeam implements domain.IAMStore.
func (m *Memory) CreateTeam(ctx context.Context, t *domain.Team) error {
	if t == nil {
		return errNil("team")
	}
	return m.write(ctx, func() error {
		if err := prepareTeam(t); err != nil {
			return err
		}
		if hasKey(m.iam.teams, t.ID) {
			return alreadyExists("team", t.ID)
		}
		sk := key(t.TenantID, t.Slug)
		if hasKey(m.iam.teamBySlug, sk) {
			return alreadyExists("team", t.TenantID, t.Slug)
		}
		m.iam.teams[t.ID] = normTeam(*t)
		m.iam.teamBySlug[sk] = t.ID
		return nil
	})
}

// GetTeam implements domain.IAMStore.
func (m *Memory) GetTeam(ctx context.Context, id string) (*domain.Team, error) {
	var out domain.Team
	err := m.read(ctx, func() error {
		t, ok := m.iam.teams[id]
		if !ok {
			return notFound("team", id)
		}
		out = t
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// GetTeamBySlug implements domain.IAMStore.
func (m *Memory) GetTeamBySlug(ctx context.Context, tenantID, slug string) (*domain.Team, error) {
	var out domain.Team
	err := m.read(ctx, func() error {
		id, ok := m.iam.teamBySlug[key(tenantID, slug)]
		if !ok {
			return notFound("team", tenantID, slug)
		}
		out = m.iam.teams[id]
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListTeams implements domain.IAMStore (ordered by slug).
func (m *Memory) ListTeams(ctx context.Context, tenantID string) ([]domain.Team, error) {
	var out []domain.Team
	err := m.read(ctx, func() error {
		out = []domain.Team{}
		for _, t := range m.iam.teams {
			if t.TenantID == tenantID {
				out = append(out, t)
			}
		}
		slices.SortFunc(out, func(a, b domain.Team) int { return strings.Compare(a.Slug, b.Slug) })
		return nil
	})
	return out, err
}

// UpdateTeam implements domain.IAMStore. The slug may change (it stays
// unique within the tenant); the tenant may not. UpdatedAt is set to now.
func (m *Memory) UpdateTeam(ctx context.Context, t *domain.Team) error {
	if t == nil {
		return errNil("team")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("team id", t.ID, "tenant id", t.TenantID, "team slug", t.Slug); err != nil {
			return err
		}
		old, ok := m.iam.teams[t.ID]
		if !ok {
			return notFound("team", t.ID)
		}
		sk := key(t.TenantID, t.Slug)
		if owner, ok := m.iam.teamBySlug[sk]; ok && owner != t.ID {
			return alreadyExists("team", t.TenantID, t.Slug)
		}
		if err := checkTeamUpdate(t, &old); err != nil {
			return err
		}
		delete(m.iam.teamBySlug, key(old.TenantID, old.Slug))
		m.iam.teamBySlug[sk] = t.ID
		m.iam.teams[t.ID] = normTeam(*t)
		return nil
	})
}

// DeleteTeam implements domain.IAMStore. It removes the team's members and
// grants.
func (m *Memory) DeleteTeam(ctx context.Context, id string) error {
	return m.write(ctx, func() error {
		t, ok := m.iam.teams[id]
		if !ok {
			return notFound("team", id)
		}
		delete(m.iam.teams, id)
		delete(m.iam.teamBySlug, key(t.TenantID, t.Slug))
		delete(m.iam.teamMembers, id)
		m.deleteSubjectGrantsLocked(domain.SubjectTeam, id)
		return nil
	})
}

// PutTeamMember implements domain.IAMStore (insert or replace; the team
// must exist). An empty Role defaults to member.
func (m *Memory) PutTeamMember(ctx context.Context, tm *domain.TeamMember) error {
	if tm == nil {
		return errNil("team member")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("team id", tm.TeamID, "account id", tm.AccountID); err != nil {
			return err
		}
		if !hasKey(m.iam.teams, tm.TeamID) {
			return notFound("team", tm.TeamID)
		}
		if err := prepareTeamMember(tm); err != nil {
			return err
		}
		in := inner(m.iam.teamMembers, tm.TeamID)
		if tm.CreatedAt.IsZero() {
			if old, ok := in[tm.AccountID]; ok {
				tm.CreatedAt = old.CreatedAt
			} else {
				tm.CreatedAt = now()
			}
		}
		in[tm.AccountID] = normTeamMember(*tm)
		return nil
	})
}

// DeleteTeamMember implements domain.IAMStore.
func (m *Memory) DeleteTeamMember(ctx context.Context, teamID, accountID string) error {
	return m.write(ctx, func() error {
		if _, ok := m.iam.teamMembers[teamID][accountID]; !ok {
			return notFound("team member", teamID, accountID)
		}
		delete(m.iam.teamMembers[teamID], accountID)
		return nil
	})
}

// ListTeamMembers implements domain.IAMStore (ordered by account id).
func (m *Memory) ListTeamMembers(ctx context.Context, teamID string) ([]domain.TeamMember, error) {
	var out []domain.TeamMember
	err := m.read(ctx, func() error {
		out = valuesByKey(m.iam.teamMembers[teamID], normTeamMember)
		return nil
	})
	return out, err
}

// ListAccountTeams implements domain.IAMStore (ordered by team id).
func (m *Memory) ListAccountTeams(ctx context.Context, accountID string) ([]domain.TeamMember, error) {
	var out []domain.TeamMember
	err := m.read(ctx, func() error {
		out = []domain.TeamMember{}
		for _, in := range m.iam.teamMembers {
			if tm, ok := in[accountID]; ok {
				out = append(out, tm)
			}
		}
		slices.SortFunc(out, func(a, b domain.TeamMember) int { return strings.Compare(a.TeamID, b.TeamID) })
		return nil
	})
	return out, err
}

// ---- custom roles -----------------------------------------------------

// PutRole implements domain.IAMStore (insert or replace). Built-in role
// names are rejected with ErrInvalid.
func (m *Memory) PutRole(ctx context.Context, r *domain.CustomRole) error {
	if r == nil {
		return errNil("role")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tenant id", r.TenantID, "role name", r.Name); err != nil {
			return err
		}
		var stored *domain.CustomRole
		if old, ok := m.iam.roles[r.TenantID][r.Name]; ok {
			stored = &old
		}
		if err := prepareRole(r, stored); err != nil {
			return err
		}
		inner(m.iam.roles, r.TenantID)[r.Name] = normRole(*r)
		return nil
	})
}

// GetRole implements domain.IAMStore.
func (m *Memory) GetRole(ctx context.Context, tenantID, name string) (*domain.CustomRole, error) {
	var out domain.CustomRole
	err := m.read(ctx, func() error {
		r, ok := m.iam.roles[tenantID][name]
		if !ok {
			return notFound("role", tenantID, name)
		}
		out = normRole(r)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListRoles implements domain.IAMStore (ordered by name).
func (m *Memory) ListRoles(ctx context.Context, tenantID string) ([]domain.CustomRole, error) {
	var out []domain.CustomRole
	err := m.read(ctx, func() error {
		out = valuesByKey(m.iam.roles[tenantID], normRole)
		return nil
	})
	return out, err
}

// DeleteRole implements domain.IAMStore.
func (m *Memory) DeleteRole(ctx context.Context, tenantID, name string) error {
	return m.write(ctx, func() error {
		if _, ok := m.iam.roles[tenantID][name]; !ok {
			return notFound("role", tenantID, name)
		}
		delete(m.iam.roles[tenantID], name)
		return nil
	})
}

// ---- policies ---------------------------------------------------------

// CreatePolicy implements domain.IAMStore. Version is set to 1.
func (m *Memory) CreatePolicy(ctx context.Context, p *domain.Policy) error {
	if p == nil {
		return errNil("policy")
	}
	return m.write(ctx, func() error {
		if err := preparePolicy(p); err != nil {
			return err
		}
		in := inner(m.iam.policies, p.TenantID)
		if hasKey(in, p.ID) {
			return alreadyExists("policy", p.TenantID, p.ID)
		}
		in[p.ID] = normPolicy(*p)
		return nil
	})
}

// GetPolicy implements domain.IAMStore.
func (m *Memory) GetPolicy(ctx context.Context, tenantID, id string) (*domain.Policy, error) {
	var out domain.Policy
	err := m.read(ctx, func() error {
		p, ok := m.iam.policies[tenantID][id]
		if !ok {
			return notFound("policy", tenantID, id)
		}
		out = normPolicy(p)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListPolicies implements domain.IAMStore (ordered by name, then id).
func (m *Memory) ListPolicies(ctx context.Context, tenantID string) ([]domain.Policy, error) {
	var out []domain.Policy
	err := m.read(ctx, func() error {
		out = valuesByKey(m.iam.policies[tenantID], normPolicy)
		sortPolicies(out)
		return nil
	})
	return out, err
}

// UpdatePolicy implements domain.IAMStore. The policy is located by
// (TenantID, ID); Version becomes the stored version + 1 and UpdatedAt now.
func (m *Memory) UpdatePolicy(ctx context.Context, p *domain.Policy) error {
	if p == nil {
		return errNil("policy")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tenant id", p.TenantID, "policy id", p.ID); err != nil {
			return err
		}
		old, ok := m.iam.policies[p.TenantID][p.ID]
		if !ok {
			return notFound("policy", p.TenantID, p.ID)
		}
		if err := checkPolicyUpdate(p, &old); err != nil {
			return err
		}
		m.iam.policies[p.TenantID][p.ID] = normPolicy(*p)
		return nil
	})
}

// DeletePolicy implements domain.IAMStore.
func (m *Memory) DeletePolicy(ctx context.Context, tenantID, id string) error {
	return m.write(ctx, func() error {
		if _, ok := m.iam.policies[tenantID][id]; !ok {
			return notFound("policy", tenantID, id)
		}
		delete(m.iam.policies[tenantID], id)
		return nil
	})
}

// ---- grants -----------------------------------------------------------

// PutGrant implements domain.IAMStore (insert or replace, keyed by tree,
// subject type and subject id).
func (m *Memory) PutGrant(ctx context.Context, g *domain.Grant) error {
	if g == nil {
		return errNil("grant")
	}
	return m.write(ctx, func() error {
		if err := checkGrant(g); err != nil {
			return err
		}
		if !hasKey(m.trees, g.TreeID) {
			return notFound("tree", g.TreeID)
		}
		switch g.SubjectType {
		case domain.SubjectTeam:
			if !hasKey(m.iam.teams, g.SubjectID) {
				return notFound("team", g.SubjectID)
			}
		case domain.SubjectServiceAccount:
			if !hasKey(m.iam.sas, g.SubjectID) {
				return notFound("service account", g.SubjectID)
			}
		}
		in := inner(m.iam.grants, g.TreeID)
		k := key(g.SubjectType, g.SubjectID)
		var stored *domain.Grant
		if old, ok := in[k]; ok {
			stored = &old
		}
		if err := prepareGrant(g, stored); err != nil {
			return err
		}
		in[k] = normGrant(*g)
		return nil
	})
}

// ListGrants implements domain.IAMStore (ordered by subject type, subject
// name, subject id).
func (m *Memory) ListGrants(ctx context.Context, treeID string) ([]domain.Grant, error) {
	var out []domain.Grant
	err := m.read(ctx, func() error {
		out = valuesByKey(m.iam.grants[treeID], normGrant)
		sortGrantsBySubject(out)
		return nil
	})
	return out, err
}

// ListSubjectGrants implements domain.IAMStore (ordered by tree id).
func (m *Memory) ListSubjectGrants(ctx context.Context, subjectType, subjectID string) ([]domain.Grant, error) {
	var out []domain.Grant
	err := m.read(ctx, func() error {
		out = []domain.Grant{}
		k := key(subjectType, subjectID)
		for _, in := range m.iam.grants {
			if g, ok := in[k]; ok {
				out = append(out, g)
			}
		}
		slices.SortFunc(out, func(a, b domain.Grant) int { return strings.Compare(a.TreeID, b.TreeID) })
		return nil
	})
	return out, err
}

// DeleteGrant implements domain.IAMStore.
func (m *Memory) DeleteGrant(ctx context.Context, treeID, subjectType, subjectID string) error {
	return m.write(ctx, func() error {
		k := key(subjectType, subjectID)
		if _, ok := m.iam.grants[treeID][k]; !ok {
			return notFound("grant", treeID, subjectType, subjectID)
		}
		delete(m.iam.grants[treeID], k)
		return nil
	})
}

func (m *Memory) deleteSubjectGrantsLocked(subjectType, subjectID string) {
	k := key(subjectType, subjectID)
	for _, in := range m.iam.grants {
		delete(in, k)
	}
}

// ---- service accounts -------------------------------------------------

// CreateServiceAccount implements domain.IAMStore.
func (m *Memory) CreateServiceAccount(ctx context.Context, sa *domain.ServiceAccount) error {
	if sa == nil {
		return errNil("service account")
	}
	return m.write(ctx, func() error {
		if err := prepareServiceAccount(sa); err != nil {
			return err
		}
		if hasKey(m.iam.sas, sa.ID) {
			return alreadyExists("service account", sa.ID)
		}
		nk := key(sa.TenantID, sa.Name)
		if hasKey(m.iam.saByName, nk) {
			return alreadyExists("service account", sa.TenantID, sa.Name)
		}
		m.iam.sas[sa.ID] = normServiceAccount(*sa)
		m.iam.saByName[nk] = sa.ID
		return nil
	})
}

// GetServiceAccount implements domain.IAMStore.
func (m *Memory) GetServiceAccount(ctx context.Context, id string) (*domain.ServiceAccount, error) {
	var out domain.ServiceAccount
	err := m.read(ctx, func() error {
		sa, ok := m.iam.sas[id]
		if !ok {
			return notFound("service account", id)
		}
		out = sa
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// GetServiceAccountByName implements domain.IAMStore.
func (m *Memory) GetServiceAccountByName(ctx context.Context, tenantID, name string) (*domain.ServiceAccount, error) {
	var out domain.ServiceAccount
	err := m.read(ctx, func() error {
		id, ok := m.iam.saByName[key(tenantID, name)]
		if !ok {
			return notFound("service account", tenantID, name)
		}
		out = m.iam.sas[id]
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListServiceAccounts implements domain.IAMStore (ordered by name).
func (m *Memory) ListServiceAccounts(ctx context.Context, tenantID string) ([]domain.ServiceAccount, error) {
	var out []domain.ServiceAccount
	err := m.read(ctx, func() error {
		out = []domain.ServiceAccount{}
		for _, sa := range m.iam.sas {
			if sa.TenantID == tenantID {
				out = append(out, sa)
			}
		}
		slices.SortFunc(out, func(a, b domain.ServiceAccount) int { return strings.Compare(a.Name, b.Name) })
		return nil
	})
	return out, err
}

// UpdateServiceAccount implements domain.IAMStore. The name may change (it
// stays unique within the tenant); the tenant may not. UpdatedAt is set to
// now.
func (m *Memory) UpdateServiceAccount(ctx context.Context, sa *domain.ServiceAccount) error {
	if sa == nil {
		return errNil("service account")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("service account id", sa.ID, "tenant id", sa.TenantID, "service account name", sa.Name); err != nil {
			return err
		}
		old, ok := m.iam.sas[sa.ID]
		if !ok {
			return notFound("service account", sa.ID)
		}
		nk := key(sa.TenantID, sa.Name)
		if owner, ok := m.iam.saByName[nk]; ok && owner != sa.ID {
			return alreadyExists("service account", sa.TenantID, sa.Name)
		}
		if err := checkServiceAccountUpdate(sa, &old); err != nil {
			return err
		}
		delete(m.iam.saByName, key(old.TenantID, old.Name))
		m.iam.saByName[nk] = sa.ID
		m.iam.sas[sa.ID] = normServiceAccount(*sa)
		return nil
	})
}

// DeleteServiceAccount implements domain.IAMStore. It removes the
// account's keys and grants.
func (m *Memory) DeleteServiceAccount(ctx context.Context, id string) error {
	return m.write(ctx, func() error {
		sa, ok := m.iam.sas[id]
		if !ok {
			return notFound("service account", id)
		}
		for tid, t := range m.tokens {
			if t.ServiceAccountID == id {
				delete(m.tokens, tid)
				delete(m.tokenByHash, t.TokenHash)
			}
		}
		m.deleteSubjectGrantsLocked(domain.SubjectServiceAccount, id)
		delete(m.iam.saByName, key(sa.TenantID, sa.Name))
		delete(m.iam.sas, id)
		return nil
	})
}

// ListServiceAccountTokens implements domain.IAMStore (ordered by creation
// time).
func (m *Memory) ListServiceAccountTokens(ctx context.Context, serviceAccountID string) ([]domain.APIToken, error) {
	var out []domain.APIToken
	err := m.read(ctx, func() error {
		out = []domain.APIToken{}
		if serviceAccountID == "" {
			return nil
		}
		for _, t := range m.tokens {
			if t.ServiceAccountID == serviceAccountID {
				out = append(out, normToken(t))
			}
		}
		sortTokens(out)
		return nil
	})
	return out, err
}

// ---- MFA --------------------------------------------------------------

// GetMFA implements domain.IAMStore (ErrNotFound when the account has no
// MFA state).
func (m *Memory) GetMFA(ctx context.Context, accountID string) (*domain.MFAState, error) {
	var out domain.MFAState
	err := m.read(ctx, func() error {
		st, ok := m.iam.mfa[accountID]
		if !ok {
			return notFound("mfa state", accountID)
		}
		out = normMFA(st)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// PutMFA implements domain.IAMStore (insert or replace).
func (m *Memory) PutMFA(ctx context.Context, st *domain.MFAState) error {
	if st == nil {
		return errNil("mfa state")
	}
	return m.write(ctx, func() error {
		if err := prepareMFA(st); err != nil {
			return err
		}
		m.iam.mfa[st.AccountID] = normMFA(*st)
		return nil
	})
}

// DeleteMFA implements domain.IAMStore.
func (m *Memory) DeleteMFA(ctx context.Context, accountID string) error {
	return m.write(ctx, func() error {
		if !hasKey(m.iam.mfa, accountID) {
			return notFound("mfa state", accountID)
		}
		delete(m.iam.mfa, accountID)
		return nil
	})
}

// ---- invitations ------------------------------------------------------

// CreateInvitation implements domain.IAMStore. CodeHash is required and
// unique across all invitations.
func (m *Memory) CreateInvitation(ctx context.Context, inv *domain.Invitation) error {
	if inv == nil {
		return errNil("invitation")
	}
	return m.write(ctx, func() error {
		if err := prepareInvitation(inv); err != nil {
			return err
		}
		in := inner(m.iam.invites, inv.TenantID)
		if hasKey(in, inv.ID) {
			return alreadyExists("invitation", inv.TenantID, inv.ID)
		}
		if hasKey(m.iam.inviteByCode, inv.CodeHash) {
			return alreadyExists("invitation code hash", "")
		}
		in[inv.ID] = normInvitation(*inv)
		m.iam.inviteByCode[inv.CodeHash] = inviteRef{inv.TenantID, inv.ID}
		return nil
	})
}

// GetInvitation implements domain.IAMStore.
func (m *Memory) GetInvitation(ctx context.Context, tenantID, id string) (*domain.Invitation, error) {
	var out domain.Invitation
	err := m.read(ctx, func() error {
		inv, ok := m.iam.invites[tenantID][id]
		if !ok {
			return notFound("invitation", tenantID, id)
		}
		out = normInvitation(inv)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// GetInvitationByCodeHash implements domain.IAMStore.
func (m *Memory) GetInvitationByCodeHash(ctx context.Context, codeHash string) (*domain.Invitation, error) {
	var out domain.Invitation
	err := m.read(ctx, func() error {
		ref, ok := m.iam.inviteByCode[codeHash]
		if !ok {
			return notFound("invitation code hash", "")
		}
		out = normInvitation(m.iam.invites[ref.tenantID][ref.id])
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListInvitations implements domain.IAMStore (newest first).
func (m *Memory) ListInvitations(ctx context.Context, tenantID string) ([]domain.Invitation, error) {
	var out []domain.Invitation
	err := m.read(ctx, func() error {
		out = valuesByKey(m.iam.invites[tenantID], normInvitation)
		sortInvitations(out)
		return nil
	})
	return out, err
}

// UpdateInvitation implements domain.IAMStore. The invitation is located
// by (TenantID, ID); an empty CodeHash keeps the stored one.
func (m *Memory) UpdateInvitation(ctx context.Context, inv *domain.Invitation) error {
	if inv == nil {
		return errNil("invitation")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("tenant id", inv.TenantID, "invitation id", inv.ID); err != nil {
			return err
		}
		old, ok := m.iam.invites[inv.TenantID][inv.ID]
		if !ok {
			return notFound("invitation", inv.TenantID, inv.ID)
		}
		self := inviteRef{inv.TenantID, inv.ID}
		if inv.CodeHash != "" && inv.CodeHash != old.CodeHash {
			if owner, ok := m.iam.inviteByCode[inv.CodeHash]; ok && owner != self {
				return alreadyExists("invitation code hash", "")
			}
		}
		if err := checkInvitationUpdate(inv, &old); err != nil {
			return err
		}
		if inv.CodeHash != old.CodeHash {
			delete(m.iam.inviteByCode, old.CodeHash)
			m.iam.inviteByCode[inv.CodeHash] = self
		}
		m.iam.invites[inv.TenantID][inv.ID] = normInvitation(*inv)
		return nil
	})
}

// DeleteInvitation implements domain.IAMStore.
func (m *Memory) DeleteInvitation(ctx context.Context, tenantID, id string) error {
	return m.write(ctx, func() error {
		inv, ok := m.iam.invites[tenantID][id]
		if !ok {
			return notFound("invitation", tenantID, id)
		}
		delete(m.iam.inviteByCode, inv.CodeHash)
		delete(m.iam.invites[tenantID], id)
		return nil
	})
}

// ---- password resets --------------------------------------------------

// PutPasswordReset implements domain.IAMStore (insert or replace, keyed by
// code hash).
func (m *Memory) PutPasswordReset(ctx context.Context, r *domain.PasswordReset) error {
	if r == nil {
		return errNil("password reset")
	}
	return m.write(ctx, func() error {
		if err := preparePasswordReset(r); err != nil {
			return err
		}
		m.iam.resets[r.CodeHash] = normPasswordReset(*r)
		return nil
	})
}

// GetPasswordReset implements domain.IAMStore.
func (m *Memory) GetPasswordReset(ctx context.Context, codeHash string) (*domain.PasswordReset, error) {
	var out domain.PasswordReset
	err := m.read(ctx, func() error {
		r, ok := m.iam.resets[codeHash]
		if !ok {
			return notFound("password reset", "")
		}
		out = normPasswordReset(r)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// DeletePasswordReset implements domain.IAMStore.
func (m *Memory) DeletePasswordReset(ctx context.Context, codeHash string) error {
	return m.write(ctx, func() error {
		if !hasKey(m.iam.resets, codeHash) {
			return notFound("password reset", "")
		}
		delete(m.iam.resets, codeHash)
		return nil
	})
}

// ---- SSO --------------------------------------------------------------

// GetSSOConfig implements domain.IAMStore (ErrNotFound if never set).
func (m *Memory) GetSSOConfig(ctx context.Context) (*domain.SSOConfig, error) {
	var out domain.SSOConfig
	err := m.read(ctx, func() error {
		if m.iam.sso == nil {
			return notFound("sso config", "")
		}
		out = normSSOConfig(*m.iam.sso)
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// PutSSOConfig implements domain.IAMStore (replace). HasSecret is derived
// from ClientSecret and UpdatedAt is set to now.
func (m *Memory) PutSSOConfig(ctx context.Context, c *domain.SSOConfig) error {
	if c == nil {
		return errNil("sso config")
	}
	return m.write(ctx, func() error {
		prepareSSOConfig(c)
		n := normSSOConfig(*c)
		m.iam.sso = &n
		return nil
	})
}

// PutSSOIdentity implements domain.IAMStore (insert or replace, keyed by
// issuer and subject).
func (m *Memory) PutSSOIdentity(ctx context.Context, id *domain.SSOIdentity) error {
	if id == nil {
		return errNil("sso identity")
	}
	return m.write(ctx, func() error {
		if err := checkKeys("sso issuer", id.Issuer, "sso subject", id.Subject, "account id", id.AccountID); err != nil {
			return err
		}
		k := key(id.Issuer, id.Subject)
		var stored *domain.SSOIdentity
		if old, ok := m.iam.ssoIdents[k]; ok {
			stored = &old
		}
		if err := prepareSSOIdentity(id, stored); err != nil {
			return err
		}
		m.iam.ssoIdents[k] = normSSOIdentity(*id)
		return nil
	})
}

// GetSSOIdentity implements domain.IAMStore.
func (m *Memory) GetSSOIdentity(ctx context.Context, issuer, subject string) (*domain.SSOIdentity, error) {
	var out domain.SSOIdentity
	err := m.read(ctx, func() error {
		id, ok := m.iam.ssoIdents[key(issuer, subject)]
		if !ok {
			return notFound("sso identity", issuer, subject)
		}
		out = id
		return nil
	})
	if err != nil {
		return nil, err
	}
	return &out, nil
}

// ListAccountSSOIdentities implements domain.IAMStore (ordered by issuer,
// then subject).
func (m *Memory) ListAccountSSOIdentities(ctx context.Context, accountID string) ([]domain.SSOIdentity, error) {
	var out []domain.SSOIdentity
	err := m.read(ctx, func() error {
		out = []domain.SSOIdentity{}
		for _, id := range m.iam.ssoIdents {
			if id.AccountID == accountID {
				out = append(out, id)
			}
		}
		slices.SortFunc(out, func(a, b domain.SSOIdentity) int {
			return cmp.Or(strings.Compare(a.Issuer, b.Issuer), strings.Compare(a.Subject, b.Subject))
		})
		return nil
	})
	return out, err
}

var _ domain.IAMStore = (*Memory)(nil)
