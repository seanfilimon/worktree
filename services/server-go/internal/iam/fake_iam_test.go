package iam

import (
	"context"
	"fmt"
	"sort"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// fakeIAM holds the IAMStore part of fakeStore. Guarded by fakeStore.mu.
type fakeIAM struct {
	teams       map[string]domain.Team
	teamMembers map[string]domain.TeamMember // team/account
	roles       map[string]domain.CustomRole // tenant/name
	sas         map[string]domain.ServiceAccount
	mfa         map[string]domain.MFAState
	invitations map[string]domain.Invitation
	resets      map[string]domain.PasswordReset
	metaTouches int
	failPutMFA  bool
	failGetMFA  bool
}

func (f *fakeStore) iam() *fakeIAM {
	if f.x == nil {
		f.x = &fakeIAM{
			teams:       map[string]domain.Team{},
			teamMembers: map[string]domain.TeamMember{},
			roles:       map[string]domain.CustomRole{},
			sas:         map[string]domain.ServiceAccount{},
			mfa:         map[string]domain.MFAState{},
			invitations: map[string]domain.Invitation{},
			resets:      map[string]domain.PasswordReset{},
		}
	}
	return f.x
}

func cloneMFA(m domain.MFAState) domain.MFAState {
	m.RecoveryCodeHashes = append([]string(nil), m.RecoveryCodeHashes...)
	if m.EnabledAt != nil {
		t := *m.EnabledAt
		m.EnabledAt = &t
	}
	return m
}

func cloneToken(t domain.APIToken) domain.APIToken {
	t.Scopes = append([]string(nil), t.Scopes...)
	if len(t.Scopes) == 0 {
		t.Scopes = nil
	}
	return t
}

// ---- teams ----

func (f *fakeStore) CreateTeam(_ context.Context, t *domain.Team) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if t.ID == "" {
		t.ID = uuid.NewString()
	}
	for _, o := range x.teams {
		if o.TenantID == t.TenantID && o.Slug == t.Slug {
			return fmt.Errorf("team: %w", domain.ErrAlreadyExists)
		}
	}
	x.teams[t.ID] = *t
	return nil
}

func (f *fakeStore) GetTeam(_ context.Context, id string) (*domain.Team, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	t, ok := f.iam().teams[id]
	if !ok {
		return nil, notFound("team")
	}
	return &t, nil
}

func (f *fakeStore) GetTeamBySlug(_ context.Context, tenantID, slug string) (*domain.Team, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, t := range f.iam().teams {
		if t.TenantID == tenantID && t.Slug == slug {
			return &t, nil
		}
	}
	return nil, notFound("team")
}

func (f *fakeStore) DeleteTeam(_ context.Context, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if _, ok := x.teams[id]; !ok {
		return notFound("team")
	}
	delete(x.teams, id)
	for k, m := range x.teamMembers {
		if m.TeamID == id {
			delete(x.teamMembers, k)
		}
	}
	return nil
}

func (f *fakeStore) PutTeamMember(_ context.Context, m *domain.TeamMember) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.iam().teamMembers[m.TeamID+"/"+m.AccountID] = *m
	return nil
}

func (f *fakeStore) ListTeamMembers(_ context.Context, teamID string) ([]domain.TeamMember, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.TeamMember{}
	for _, m := range f.iam().teamMembers {
		if m.TeamID == teamID {
			out = append(out, m)
		}
	}
	return out, nil
}

func (f *fakeStore) ListAccountTeams(_ context.Context, accountID string) ([]domain.TeamMember, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.TeamMember{}
	for _, m := range f.iam().teamMembers {
		if m.AccountID == accountID {
			out = append(out, m)
		}
	}
	return out, nil
}

// ---- roles ----

func (f *fakeStore) PutRole(_ context.Context, r *domain.CustomRole) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.iam().roles[r.TenantID+"/"+r.Name] = *r
	return nil
}

func (f *fakeStore) GetRole(_ context.Context, tenantID, name string) (*domain.CustomRole, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	r, ok := f.iam().roles[tenantID+"/"+name]
	if !ok {
		return nil, notFound("role")
	}
	return &r, nil
}

// ---- service accounts ----

func (f *fakeStore) CreateServiceAccount(_ context.Context, sa *domain.ServiceAccount) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if sa.ID == "" {
		sa.ID = uuid.NewString()
	}
	x.sas[sa.ID] = *sa
	return nil
}

func (f *fakeStore) GetServiceAccount(_ context.Context, id string) (*domain.ServiceAccount, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	sa, ok := f.iam().sas[id]
	if !ok {
		return nil, notFound("service account")
	}
	return &sa, nil
}

func (f *fakeStore) UpdateServiceAccount(_ context.Context, sa *domain.ServiceAccount) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if _, ok := x.sas[sa.ID]; !ok {
		return notFound("service account")
	}
	x.sas[sa.ID] = *sa
	return nil
}

func (f *fakeStore) DeleteServiceAccount(_ context.Context, id string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if _, ok := x.sas[id]; !ok {
		return notFound("service account")
	}
	delete(x.sas, id)
	for k, t := range f.tokens {
		if t.ServiceAccountID == id {
			delete(f.tokens, k)
		}
	}
	return nil
}

func (f *fakeStore) ListServiceAccountTokens(_ context.Context, id string) ([]domain.APIToken, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.APIToken{}
	for _, t := range f.tokens {
		if t.ServiceAccountID == id {
			out = append(out, cloneToken(t))
		}
	}
	return out, nil
}

// ---- MFA ----

func (f *fakeStore) GetMFA(_ context.Context, accountID string) (*domain.MFAState, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if x.failGetMFA {
		return nil, fmt.Errorf("fake: store unavailable")
	}
	m, ok := x.mfa[accountID]
	if !ok {
		return nil, notFound("mfa")
	}
	m = cloneMFA(m)
	return &m, nil
}

func (f *fakeStore) PutMFA(_ context.Context, m *domain.MFAState) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if x.failPutMFA {
		return fmt.Errorf("fake: store unavailable")
	}
	x.mfa[m.AccountID] = cloneMFA(*m)
	return nil
}

func (f *fakeStore) DeleteMFA(_ context.Context, accountID string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if _, ok := x.mfa[accountID]; !ok {
		return notFound("mfa")
	}
	delete(x.mfa, accountID)
	return nil
}

func (f *fakeStore) rawMFA(accountID string) (domain.MFAState, bool) {
	f.mu.Lock()
	defer f.mu.Unlock()
	m, ok := f.iam().mfa[accountID]
	return cloneMFA(m), ok
}

// ---- invitations ----

func (f *fakeStore) CreateInvitation(_ context.Context, inv *domain.Invitation) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	if inv.ID == "" {
		inv.ID = uuid.NewString()
	}
	f.iam().invitations[inv.ID] = *inv
	return nil
}

func (f *fakeStore) GetInvitation(_ context.Context, tenantID, id string) (*domain.Invitation, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	inv, ok := f.iam().invitations[id]
	if !ok || inv.TenantID != tenantID {
		return nil, notFound("invitation")
	}
	return &inv, nil
}

func (f *fakeStore) GetInvitationByCodeHash(_ context.Context, h string) (*domain.Invitation, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	for _, inv := range f.iam().invitations {
		if inv.CodeHash == h {
			return &inv, nil
		}
	}
	return nil, notFound("invitation")
}

func (f *fakeStore) ListInvitations(_ context.Context, tenantID string) ([]domain.Invitation, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	out := []domain.Invitation{}
	for _, inv := range f.iam().invitations {
		if inv.TenantID == tenantID {
			out = append(out, inv)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].CreatedAt.After(out[j].CreatedAt) })
	return out, nil
}

func (f *fakeStore) UpdateInvitation(_ context.Context, inv *domain.Invitation) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	x := f.iam()
	if _, ok := x.invitations[inv.ID]; !ok {
		return notFound("invitation")
	}
	x.invitations[inv.ID] = *inv
	return nil
}

// ---- password resets ----

func (f *fakeStore) PutPasswordReset(_ context.Context, r *domain.PasswordReset) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	f.iam().resets[r.CodeHash] = *r
	return nil
}

func (f *fakeStore) GetPasswordReset(_ context.Context, h string) (*domain.PasswordReset, error) {
	f.mu.Lock()
	defer f.mu.Unlock()
	r, ok := f.iam().resets[h]
	if !ok {
		return nil, notFound("password reset")
	}
	return &r, nil
}

func (f *fakeStore) DeletePasswordReset(_ context.Context, h string) error {
	f.mu.Lock()
	defer f.mu.Unlock()
	delete(f.iam().resets, h)
	return nil
}

// metaStore wraps fakeStore and additionally implements TokenMetaToucher.
type metaStore struct{ *fakeStore }

func (m metaStore) TouchTokenMeta(_ context.Context, id string, at time.Time, ip, ua string) error {
	m.mu.Lock()
	defer m.mu.Unlock()
	t, ok := m.tokens[id]
	if !ok {
		return notFound("token")
	}
	t.LastUsedAt = &at
	t.LastIP = ip
	t.UserAgent = ua
	m.tokens[id] = t
	m.iam().metaTouches++
	return nil
}

func (f *fakeStore) token(id string) (domain.APIToken, bool) {
	f.mu.Lock()
	defer f.mu.Unlock()
	t, ok := f.tokens[id]
	return cloneToken(t), ok
}
