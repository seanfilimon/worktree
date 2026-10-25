package httpapi

import (
	"context"
	"fmt"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ---- fakes specific to the IAM endpoints ----
//
// Like manage_test.go, they wrap the shared fakes instead of adding
// methods to them so other test files can extend those independently.

// iamStore adds the IAMStore methods to the shared fakeStore.
type iamStore struct {
	*fakeStore

	imu         sync.Mutex
	teams       map[string]domain.Team
	teamMembers map[[2]string]domain.TeamMember // team, account
	roles       map[[2]string]domain.CustomRole // tenant, name
	policies    map[string]domain.Policy
	grants      map[[3]string]domain.Grant // tree, subject type, subject id
	sas         map[string]domain.ServiceAccount
	invitations map[string]domain.Invitation
	resets      map[string]domain.PasswordReset
}

func newIAMStore(base *fakeStore) *iamStore {
	return &iamStore{
		fakeStore:   base,
		teams:       map[string]domain.Team{},
		teamMembers: map[[2]string]domain.TeamMember{},
		roles:       map[[2]string]domain.CustomRole{},
		policies:    map[string]domain.Policy{},
		grants:      map[[3]string]domain.Grant{},
		sas:         map[string]domain.ServiceAccount{},
		invitations: map[string]domain.Invitation{},
		resets:      map[string]domain.PasswordReset{},
	}
}

func iamExists(what string) error { return fmt.Errorf("%s: %w", what, domain.ErrAlreadyExists) }

// Teams.

func (s *iamStore) CreateTeam(_ context.Context, t *domain.Team) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	for _, x := range s.teams {
		if x.TenantID == t.TenantID && x.Slug == t.Slug {
			return iamExists("team")
		}
	}
	if t.ID == "" {
		t.ID = "team-" + newID()
	}
	s.teams[t.ID] = *t
	return nil
}

func (s *iamStore) GetTeam(_ context.Context, id string) (*domain.Team, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	t, ok := s.teams[id]
	if !ok {
		return nil, nf("team")
	}
	return &t, nil
}

func (s *iamStore) GetTeamBySlug(_ context.Context, tenantID, slug string) (*domain.Team, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	for _, t := range s.teams {
		if t.TenantID == tenantID && t.Slug == slug {
			return &t, nil
		}
	}
	return nil, nf("team")
}

func (s *iamStore) ListTeams(_ context.Context, tenantID string) ([]domain.Team, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.Team{}
	for _, t := range s.teams {
		if t.TenantID == tenantID {
			out = append(out, t)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Slug < out[j].Slug })
	return out, nil
}

func (s *iamStore) UpdateTeam(_ context.Context, t *domain.Team) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	if _, ok := s.teams[t.ID]; !ok {
		return nf("team")
	}
	s.teams[t.ID] = *t
	return nil
}

func (s *iamStore) DeleteTeam(_ context.Context, id string) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	if _, ok := s.teams[id]; !ok {
		return nf("team")
	}
	delete(s.teams, id)
	for k := range s.teamMembers {
		if k[0] == id {
			delete(s.teamMembers, k)
		}
	}
	for k := range s.grants {
		if k[1] == domain.SubjectTeam && k[2] == id {
			delete(s.grants, k)
		}
	}
	return nil
}

func (s *iamStore) PutTeamMember(_ context.Context, m *domain.TeamMember) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	s.teamMembers[[2]string{m.TeamID, m.AccountID}] = *m
	return nil
}

func (s *iamStore) DeleteTeamMember(_ context.Context, teamID, accountID string) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	k := [2]string{teamID, accountID}
	if _, ok := s.teamMembers[k]; !ok {
		return nf("team member")
	}
	delete(s.teamMembers, k)
	return nil
}

func (s *iamStore) ListTeamMembers(_ context.Context, teamID string) ([]domain.TeamMember, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.TeamMember{}
	for k, m := range s.teamMembers {
		if k[0] == teamID {
			out = append(out, m)
		}
	}
	return out, nil
}

func (s *iamStore) ListAccountTeams(_ context.Context, accountID string) ([]domain.TeamMember, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.TeamMember{}
	for k, m := range s.teamMembers {
		if k[1] == accountID {
			out = append(out, m)
		}
	}
	return out, nil
}

// Roles.

func (s *iamStore) PutRole(_ context.Context, r *domain.CustomRole) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	r2 := *r
	r2.Permissions = slices.Clone(r.Permissions)
	s.roles[[2]string{r.TenantID, r.Name}] = r2
	return nil
}

func (s *iamStore) GetRole(_ context.Context, tenantID, name string) (*domain.CustomRole, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	r, ok := s.roles[[2]string{tenantID, name}]
	if !ok {
		return nil, nf("role")
	}
	r.Permissions = slices.Clone(r.Permissions)
	return &r, nil
}

func (s *iamStore) ListRoles(_ context.Context, tenantID string) ([]domain.CustomRole, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.CustomRole{}
	for k, r := range s.roles {
		if k[0] == tenantID {
			out = append(out, r)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out, nil
}

func (s *iamStore) DeleteRole(_ context.Context, tenantID, name string) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	k := [2]string{tenantID, name}
	if _, ok := s.roles[k]; !ok {
		return nf("role")
	}
	delete(s.roles, k)
	return nil
}

// Policies.

func (s *iamStore) CreatePolicy(_ context.Context, p *domain.Policy) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	if p.ID == "" {
		p.ID = "pol-" + newID()
	}
	s.policies[p.ID] = *p
	return nil
}

func (s *iamStore) GetPolicy(_ context.Context, tenantID, id string) (*domain.Policy, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	p, ok := s.policies[id]
	if !ok || p.TenantID != tenantID {
		return nil, nf("policy")
	}
	return &p, nil
}

func (s *iamStore) ListPolicies(_ context.Context, tenantID string) ([]domain.Policy, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.Policy{}
	for _, p := range s.policies {
		if p.TenantID == tenantID {
			out = append(out, p)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out, nil
}

func (s *iamStore) UpdatePolicy(_ context.Context, p *domain.Policy) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	if _, ok := s.policies[p.ID]; !ok {
		return nf("policy")
	}
	s.policies[p.ID] = *p
	return nil
}

func (s *iamStore) DeletePolicy(_ context.Context, tenantID, id string) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	p, ok := s.policies[id]
	if !ok || p.TenantID != tenantID {
		return nf("policy")
	}
	delete(s.policies, id)
	return nil
}

// Grants.

func (s *iamStore) PutGrant(_ context.Context, g *domain.Grant) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	s.grants[[3]string{g.TreeID, g.SubjectType, g.SubjectID}] = *g
	return nil
}

func (s *iamStore) ListGrants(_ context.Context, treeID string) ([]domain.Grant, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.Grant{}
	for k, g := range s.grants {
		if k[0] == treeID {
			out = append(out, g)
		}
	}
	return out, nil
}

func (s *iamStore) ListSubjectGrants(_ context.Context, subjectType, subjectID string) ([]domain.Grant, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.Grant{}
	for k, g := range s.grants {
		if k[1] == subjectType && k[2] == subjectID {
			out = append(out, g)
		}
	}
	return out, nil
}

func (s *iamStore) DeleteGrant(_ context.Context, treeID, subjectType, subjectID string) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	k := [3]string{treeID, subjectType, subjectID}
	if _, ok := s.grants[k]; !ok {
		return nf("grant")
	}
	delete(s.grants, k)
	return nil
}

// Service accounts.

func (s *iamStore) CreateServiceAccount(_ context.Context, sa *domain.ServiceAccount) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	for _, x := range s.sas {
		if x.TenantID == sa.TenantID && x.Name == sa.Name {
			return iamExists("service account")
		}
	}
	if sa.ID == "" {
		sa.ID = "sa-" + newID()
	}
	s.sas[sa.ID] = *sa
	return nil
}

func (s *iamStore) GetServiceAccount(_ context.Context, id string) (*domain.ServiceAccount, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	sa, ok := s.sas[id]
	if !ok {
		return nil, nf("service account")
	}
	return &sa, nil
}

func (s *iamStore) GetServiceAccountByName(_ context.Context, tenantID, name string) (*domain.ServiceAccount, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	for _, sa := range s.sas {
		if sa.TenantID == tenantID && sa.Name == name {
			return &sa, nil
		}
	}
	return nil, nf("service account")
}

func (s *iamStore) ListServiceAccounts(_ context.Context, tenantID string) ([]domain.ServiceAccount, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.ServiceAccount{}
	for _, sa := range s.sas {
		if sa.TenantID == tenantID {
			out = append(out, sa)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out, nil
}

func (s *iamStore) UpdateServiceAccount(_ context.Context, sa *domain.ServiceAccount) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	if _, ok := s.sas[sa.ID]; !ok {
		return nf("service account")
	}
	s.sas[sa.ID] = *sa
	return nil
}

func (s *iamStore) DeleteServiceAccount(_ context.Context, id string) error {
	s.imu.Lock()
	if _, ok := s.sas[id]; !ok {
		s.imu.Unlock()
		return nf("service account")
	}
	delete(s.sas, id)
	for k := range s.grants {
		if k[1] == domain.SubjectServiceAccount && k[2] == id {
			delete(s.grants, k)
		}
	}
	s.imu.Unlock()
	s.fakeStore.mu.Lock()
	defer s.fakeStore.mu.Unlock()
	for tid, t := range s.tokens {
		if t.ServiceAccountID == id {
			delete(s.tokens, tid)
		}
	}
	return nil
}

func (s *iamStore) ListServiceAccountTokens(_ context.Context, id string) ([]domain.APIToken, error) {
	s.fakeStore.mu.Lock()
	defer s.fakeStore.mu.Unlock()
	out := []domain.APIToken{}
	for _, t := range s.tokens {
		if t.ServiceAccountID == id {
			out = append(out, t)
		}
	}
	return out, nil
}

// Invitations.

func (s *iamStore) CreateInvitation(_ context.Context, inv *domain.Invitation) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	if inv.ID == "" {
		inv.ID = "inv-" + newID()
	}
	s.invitations[inv.ID] = *inv
	return nil
}

func (s *iamStore) GetInvitation(_ context.Context, tenantID, id string) (*domain.Invitation, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	inv, ok := s.invitations[id]
	if !ok || inv.TenantID != tenantID {
		return nil, nf("invitation")
	}
	return &inv, nil
}

func (s *iamStore) GetInvitationByCodeHash(_ context.Context, h string) (*domain.Invitation, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	for _, inv := range s.invitations {
		if inv.CodeHash == h {
			return &inv, nil
		}
	}
	return nil, nf("invitation")
}

func (s *iamStore) ListInvitations(_ context.Context, tenantID string) ([]domain.Invitation, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	out := []domain.Invitation{}
	for _, inv := range s.invitations {
		if inv.TenantID == tenantID {
			out = append(out, inv)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].CreatedAt.After(out[j].CreatedAt) })
	return out, nil
}

func (s *iamStore) UpdateInvitation(_ context.Context, inv *domain.Invitation) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	if _, ok := s.invitations[inv.ID]; !ok {
		return nf("invitation")
	}
	s.invitations[inv.ID] = *inv
	return nil
}

// Password resets.

func (s *iamStore) PutPasswordReset(_ context.Context, r *domain.PasswordReset) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	s.resets[r.CodeHash] = *r
	return nil
}

func (s *iamStore) GetPasswordReset(_ context.Context, h string) (*domain.PasswordReset, error) {
	s.imu.Lock()
	defer s.imu.Unlock()
	r, ok := s.resets[h]
	if !ok {
		return nil, nf("password reset")
	}
	return &r, nil
}

func (s *iamStore) DeletePasswordReset(_ context.Context, h string) error {
	s.imu.Lock()
	defer s.imu.Unlock()
	delete(s.resets, h)
	return nil
}

// ---- Authorizer with the §12 IAM actions ----

// iamAuthz extends fakeAuthz: maintainers have iam:read (owners and admins
// already have everything), service accounts act with their Role.
type iamAuthz struct {
	*fakeAuthz
	store *iamStore
}

func (z *iamAuthz) Authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	if tenantID == "" && tree != nil {
		tenantID = tree.TenantID
	}
	if p.IsServiceAccount() {
		sa, err := z.store.GetServiceAccount(ctx, p.ServiceAccountID)
		if err != nil || sa.TenantID != tenantID || sa.Disabled {
			return fmt.Errorf("service account: %w", domain.ErrForbidden)
		}
		if sa.Role == domain.RoleAdmin || (act == domain.ActIAMRead && sa.Role == domain.RoleMaintainer) {
			return nil
		}
		return fmt.Errorf("service account role %s: %w", sa.Role, domain.ErrForbidden)
	}
	if p != nil && act == domain.ActIAMRead {
		if role, _ := z.RoleOf(ctx, p.AccountID, tenantID); role == domain.RoleMaintainer {
			return nil
		}
	}
	return z.fakeAuthz.Authorize(ctx, p, act, tenantID, tree)
}

// ---- AccountSecurity ----

const iamTestTOTP = "123456"

type iamSecurity struct {
	store *iamStore
	auth  *fakeAuth

	mu         sync.Mutex
	mfaUsers   map[string]bool // usernames with MFA enabled (login needs a second step)
	mfa        map[string]*domain.MFAState
	lastMeta   domain.SessionMeta
	lastScopes []string
	revoked    map[string]string // account -> kept token hash of RevokeOtherSessions
}

func newIAMSecurity(store *iamStore, auth *fakeAuth) *iamSecurity {
	return &iamSecurity{store: store, auth: auth, mfaUsers: map[string]bool{}, mfa: map[string]*domain.MFAState{}, revoked: map[string]string{}}
}

func (s *iamSecurity) meta() domain.SessionMeta {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.lastMeta
}

func (s *iamSecurity) LoginWithMeta(ctx context.Context, username, password string, meta domain.SessionMeta) (*domain.LoginResult, error) {
	s.mu.Lock()
	s.lastMeta = meta
	mfa := s.mfaUsers[username]
	s.mu.Unlock()
	s.auth.mu.Lock()
	pw, ok := s.auth.passwords[username]
	s.auth.mu.Unlock()
	if !ok || pw != password {
		return nil, fmt.Errorf("bad credentials: %w", domain.ErrUnauthenticated)
	}
	acct, err := s.store.GetAccountByUsername(ctx, username)
	if err != nil {
		return nil, err
	}
	if mfa {
		return &domain.LoginResult{Account: acct, MFARequired: true, MFAToken: "mfa-" + username}, nil
	}
	tok, _, err := s.auth.CreateToken(ctx, acct.ID, "session", 0)
	return &domain.LoginResult{Token: tok, Account: acct}, err
}

func (s *iamSecurity) CompleteMFALogin(ctx context.Context, mfaToken, code string, meta domain.SessionMeta) (string, *domain.Account, error) {
	user, ok := strings.CutPrefix(mfaToken, "mfa-")
	if !ok || code != iamTestTOTP {
		return "", nil, fmt.Errorf("bad code: %w", domain.ErrUnauthenticated)
	}
	acct, err := s.store.GetAccountByUsername(ctx, user)
	if err != nil {
		return "", nil, err
	}
	s.mu.Lock()
	s.lastMeta = meta
	s.mu.Unlock()
	tok, _, err := s.auth.CreateToken(ctx, acct.ID, "session", 0)
	return tok, acct, err
}

func (s *iamSecurity) IssueSession(ctx context.Context, accountID string, _ bool, _ domain.SessionMeta) (string, error) {
	tok, _, err := s.auth.CreateToken(ctx, accountID, "session", 0)
	return tok, err
}

func (s *iamSecurity) MFAStatus(_ context.Context, accountID string) (*domain.MFAState, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if st := s.mfa[accountID]; st != nil {
		c := *st
		return &c, nil
	}
	return &domain.MFAState{AccountID: accountID}, nil
}

func (s *iamSecurity) BeginTOTPEnroll(_ context.Context, accountID string) (string, string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.mfa[accountID] = &domain.MFAState{AccountID: accountID, PendingSecret: "JBSWY3DPEHPK3PXP"}
	return "JBSWY3DPEHPK3PXP", "otpauth://totp/W0rkTree:x?secret=JBSWY3DPEHPK3PXP&issuer=W0rkTree", nil
}

func (s *iamSecurity) ConfirmTOTP(_ context.Context, accountID, code string) ([]string, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	st := s.mfa[accountID]
	if st == nil || st.PendingSecret == "" {
		return nil, fmt.Errorf("no enrollment: %w", domain.ErrConflict)
	}
	if code != iamTestTOTP {
		return nil, fmt.Errorf("bad code: %w", domain.ErrUnauthenticated)
	}
	now := time.Now()
	st.TOTPEnabled, st.TOTPSecret, st.PendingSecret, st.EnabledAt, st.RecoveryCodesLeft = true, st.PendingSecret, "", &now, 10
	return []string{"aaaa-bbbb", "cccc-dddd"}, nil
}

func (s *iamSecurity) DisableTOTP(ctx context.Context, accountID, password string) error {
	acct, err := s.store.GetAccount(ctx, accountID)
	if err != nil {
		return err
	}
	s.auth.mu.Lock()
	ok := s.auth.passwords[acct.Username] == password
	s.auth.mu.Unlock()
	if !ok {
		return fmt.Errorf("bad password: %w", domain.ErrUnauthenticated)
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	delete(s.mfa, accountID)
	return nil
}

func (s *iamSecurity) RegenerateRecoveryCodes(_ context.Context, accountID, code string) ([]string, error) {
	if code != iamTestTOTP {
		return nil, fmt.Errorf("bad code: %w", domain.ErrUnauthenticated)
	}
	return []string{"eeee-ffff"}, nil
}

func (s *iamSecurity) ListSessions(_ context.Context, accountID string) ([]domain.APIToken, error) {
	s.store.fakeStore.mu.Lock()
	defer s.store.fakeStore.mu.Unlock()
	out := []domain.APIToken{}
	for _, t := range s.store.tokens {
		if t.AccountID == accountID && (t.Kind == domain.TokenSession || (t.Kind == "" && t.Name == "session")) {
			out = append(out, t)
		}
	}
	return out, nil
}

func (s *iamSecurity) RevokeOtherSessions(ctx context.Context, accountID, keep string) (int, error) {
	toks, _ := s.ListSessions(ctx, accountID)
	n := 0
	for _, t := range toks {
		if t.TokenHash != keep {
			_ = s.store.DeleteToken(ctx, t.ID)
			n++
		}
	}
	s.mu.Lock()
	s.revoked[accountID] = keep
	s.mu.Unlock()
	return n, nil
}

func (s *iamSecurity) newToken(ctx context.Context, t *domain.APIToken, ttl time.Duration) (string, *domain.APIToken, error) {
	raw := "wt_" + newID()
	t.ID, t.TokenHash, t.CreatedAt = newID(), hashToken(raw), time.Now()
	if ttl > 0 {
		exp := t.CreatedAt.Add(ttl)
		t.ExpiresAt = &exp
	}
	return raw, t, s.store.CreateToken(ctx, t)
}

func (s *iamSecurity) CreateScopedToken(ctx context.Context, accountID, name string, ttl time.Duration, scopes []string) (string, *domain.APIToken, error) {
	s.mu.Lock()
	s.lastScopes = slices.Clone(scopes)
	s.mu.Unlock()
	return s.newToken(ctx, &domain.APIToken{AccountID: accountID, Name: name, Kind: domain.TokenAPI, Scopes: scopes}, ttl)
}

func (s *iamSecurity) CreateServiceAccountKey(ctx context.Context, saID, name string, ttl time.Duration, scopes []string) (string, *domain.APIToken, error) {
	return s.newToken(ctx, &domain.APIToken{ServiceAccountID: saID, Name: name, Kind: domain.TokenService, Scopes: scopes}, ttl)
}

func (s *iamSecurity) RevokeServiceAccountKey(ctx context.Context, saID, tokenID string) error {
	s.store.fakeStore.mu.Lock()
	t, ok := s.store.tokens[tokenID]
	s.store.fakeStore.mu.Unlock()
	if !ok || t.ServiceAccountID != saID {
		return nf("key")
	}
	return s.store.DeleteToken(ctx, tokenID)
}

func (s *iamSecurity) CreateInvitation(ctx context.Context, inv *domain.Invitation, ttl time.Duration) (string, error) {
	code := "wti_" + newID()
	inv.CodeHash = hashToken(code)
	inv.CreatedAt = time.Now().UTC()
	inv.ExpiresAt = inv.CreatedAt.Add(ttl)
	return code, s.store.CreateInvitation(ctx, inv)
}

func (s *iamSecurity) PreviewInvitation(ctx context.Context, code string) (*domain.Invitation, *domain.Tenant, error) {
	inv, err := s.store.GetInvitationByCodeHash(ctx, hashToken(code))
	if err != nil {
		return nil, nil, err
	}
	if inv.Revoked || inv.AcceptedAt != nil || time.Now().After(inv.ExpiresAt) {
		return nil, nil, nf("invitation")
	}
	t, err := s.store.GetTenant(ctx, inv.TenantID)
	return inv, t, err
}

func (s *iamSecurity) AcceptInvitation(ctx context.Context, code string, p *domain.Principal) (*domain.Invitation, error) {
	inv, _, err := s.PreviewInvitation(ctx, code)
	if err != nil {
		return nil, err
	}
	if inv.Username != "" && inv.Username != p.Username {
		return nil, fmt.Errorf("invitation is for another account: %w", domain.ErrForbidden)
	}
	if err := s.store.PutMembership(ctx, &domain.Membership{TenantID: inv.TenantID, AccountID: p.AccountID, Role: inv.Role}); err != nil {
		return nil, err
	}
	now := time.Now().UTC()
	inv.AcceptedAt, inv.AcceptedBy = &now, p.Username
	return inv, s.store.UpdateInvitation(ctx, inv)
}

func (s *iamSecurity) CreatePasswordReset(ctx context.Context, accountID, createdBy string, ttl time.Duration) (string, time.Time, error) {
	code := "wtr_" + newID()
	now := time.Now().UTC()
	r := &domain.PasswordReset{CodeHash: hashToken(code), AccountID: accountID, CreatedBy: createdBy, CreatedAt: now, ExpiresAt: now.Add(ttl)}
	return code, r.ExpiresAt, s.store.PutPasswordReset(ctx, r)
}

func (s *iamSecurity) ResetPassword(ctx context.Context, code, newPassword string) error {
	r, err := s.store.GetPasswordReset(ctx, hashToken(code))
	if err != nil {
		return err
	}
	if time.Now().After(r.ExpiresAt) {
		return nf("password reset")
	}
	acct, err := s.store.GetAccount(ctx, r.AccountID)
	if err != nil {
		return err
	}
	s.auth.mu.Lock()
	s.auth.passwords[acct.Username] = newPassword
	s.auth.mu.Unlock()
	return s.store.DeletePasswordReset(ctx, r.CodeHash)
}

// ---- AccessEvaluator ----

type iamAccess struct {
	mu       sync.Mutex
	lastP    *domain.Principal
	lastAct  domain.Action
	lastRes  domain.Resource
	lastEnv  domain.RequestEnv
	lastTree *domain.Tree
}

func (e *iamAccess) Evaluate(_ context.Context, p *domain.Principal, act domain.Action, res domain.Resource, env domain.RequestEnv) (domain.Decision, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	pc := *p
	e.lastP, e.lastAct, e.lastRes, e.lastEnv = &pc, act, res, env
	if act == domain.ActTreeRead {
		return domain.Decision{Allowed: true, Effect: "allow", Reasons: []domain.DecisionReason{{Source: "tenant_role", Detail: "viewer", Effect: "allow"}}}, nil
	}
	return domain.Decision{Allowed: false, Effect: "implicit_deny"}, nil
}

func (e *iamAccess) EffectivePermissions(_ context.Context, p *domain.Principal, _ string, tree *domain.Tree) ([]domain.Action, error) {
	e.mu.Lock()
	defer e.mu.Unlock()
	pc := *p
	e.lastP, e.lastTree = &pc, tree
	return []domain.Action{domain.ActTreeWrite, domain.ActTreeRead, domain.ActTreeRead}, nil
}

// foreignValidationErr mimics a validation result type defined by another
// package (e.g. authz), exercising the reflective adapter.
type foreignValidationErr struct {
	Index   int
	Path    string
	Message string
}

// validatingAccess is an AccessEvaluator that also validates statements.
type validatingAccess struct{ iamAccess }

func (*validatingAccess) ValidateStatements(stmts []domain.PolicyStatement) []*foreignValidationErr {
	var out []*foreignValidationErr
	for i, s := range stmts {
		if s.Sid == "bad" {
			out = append(out, &foreignValidationErr{Index: i, Path: "sid", Message: "engine rejects sid bad"})
		}
	}
	return out
}
