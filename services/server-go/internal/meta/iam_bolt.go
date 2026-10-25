package meta

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"slices"

	bolt "go.etcd.io/bbolt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Stored forms of IAM records whose domain types hide secrets from JSON
// (json:"-"). As with accountRec / tokenRec, the secret is persisted
// through a shadowing field of the wrapper.

type mfaRec struct {
	domain.MFAState
	TOTPSecret         string   `json:"totp_secret"`
	PendingSecret      string   `json:"pending_secret"`
	RecoveryCodeHashes []string `json:"recovery_code_hashes"`
	LastUsedStep       int64    `json:"last_used_step"`
}

type invitationRec struct {
	domain.Invitation
	CodeHash string `json:"code_hash"`
}

type passwordResetRec struct {
	domain.PasswordReset
	CodeHash string `json:"code_hash"`
}

type ssoConfigRec struct {
	domain.SSOConfig
	ClientSecret string `json:"client_secret"`
}

var ssoConfigKey = []byte("config")

// getRec decodes the value stored under k, returning nil when missing.
func getRec[T any](bk *bolt.Bucket, k []byte) (*T, error) {
	var v T
	ok, err := getJSON(bk, k, &v)
	if err != nil || !ok {
		return nil, err
	}
	return &v, nil
}

// lastComponent returns the part of k after the scan prefix p.
func lastComponent(k, p []byte) string {
	return string(bytes.TrimPrefix(k, p))
}

// ---- teams ------------------------------------------------------------

func getTeamTx(tx *bolt.Tx, id string) (*domain.Team, error) {
	t, err := getRec[domain.Team](tx.Bucket(bktTeams), []byte(id))
	if err != nil {
		return nil, err
	}
	if t == nil {
		return nil, notFound("team", id)
	}
	return t, nil
}

// CreateTeam implements domain.IAMStore.
func (b *Bolt) CreateTeam(ctx context.Context, t *domain.Team) error {
	if t == nil {
		return errNil("team")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareTeam(t); err != nil {
			return err
		}
		tb, ib := tx.Bucket(bktTeams), tx.Bucket(bktTeamSlug)
		if tb.Get([]byte(t.ID)) != nil {
			return alreadyExists("team", t.ID)
		}
		sk := []byte(key(t.TenantID, t.Slug))
		if ib.Get(sk) != nil {
			return alreadyExists("team", t.TenantID, t.Slug)
		}
		if err := ib.Put(sk, []byte(t.ID)); err != nil {
			return err
		}
		return putJSON(tb, []byte(t.ID), normTeam(*t))
	})
}

// GetTeam implements domain.IAMStore.
func (b *Bolt) GetTeam(ctx context.Context, id string) (*domain.Team, error) {
	var out *domain.Team
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getTeamTx(tx, id)
		return err
	})
	return out, err
}

// GetTeamBySlug implements domain.IAMStore.
func (b *Bolt) GetTeamBySlug(ctx context.Context, tenantID, slug string) (*domain.Team, error) {
	var out *domain.Team
	err := b.view(ctx, func(tx *bolt.Tx) error {
		id := tx.Bucket(bktTeamSlug).Get([]byte(key(tenantID, slug)))
		if id == nil {
			return notFound("team", tenantID, slug)
		}
		var err error
		out, err = getTeamTx(tx, string(id))
		return err
	})
	return out, err
}

// ListTeams implements domain.IAMStore (ordered by slug).
func (b *Bolt) ListTeams(ctx context.Context, tenantID string) ([]domain.Team, error) {
	out := []domain.Team{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return scanPrefix(tx.Bucket(bktTeamSlug), []byte(prefix(tenantID)), func(_, id []byte) error {
			t, err := getTeamTx(tx, string(id))
			if err != nil {
				return err
			}
			out = append(out, *t)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// UpdateTeam implements domain.IAMStore. The slug may change (it stays
// unique within the tenant); the tenant may not. UpdatedAt is set to now.
func (b *Bolt) UpdateTeam(ctx context.Context, t *domain.Team) error {
	if t == nil {
		return errNil("team")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("team id", t.ID, "tenant id", t.TenantID, "team slug", t.Slug); err != nil {
			return err
		}
		old, err := getTeamTx(tx, t.ID)
		if err != nil {
			return err
		}
		ib := tx.Bucket(bktTeamSlug)
		sk := []byte(key(t.TenantID, t.Slug))
		if owner := ib.Get(sk); owner != nil && string(owner) != t.ID {
			return alreadyExists("team", t.TenantID, t.Slug)
		}
		if err := checkTeamUpdate(t, old); err != nil {
			return err
		}
		if err := ib.Delete([]byte(key(old.TenantID, old.Slug))); err != nil {
			return err
		}
		if err := ib.Put(sk, []byte(t.ID)); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktTeams), []byte(t.ID), normTeam(*t))
	})
}

// DeleteTeam implements domain.IAMStore. It removes the team's members and
// grants.
func (b *Bolt) DeleteTeam(ctx context.Context, id string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		t, err := getTeamTx(tx, id)
		if err != nil {
			return err
		}
		mb, xb := tx.Bucket(bktTeamMembers), tx.Bucket(bktTeamMembersByAcct)
		p := []byte(prefix(id))
		var accounts []string
		_ = scanPrefix(mb, p, func(k, _ []byte) error {
			accounts = append(accounts, lastComponent(k, p))
			return nil
		})
		for _, acct := range accounts {
			if err := xb.Delete([]byte(key(acct, id))); err != nil {
				return err
			}
		}
		if _, err := deletePrefix(mb, p); err != nil {
			return err
		}
		if err := deleteSubjectGrantsTx(tx, domain.SubjectTeam, id); err != nil {
			return err
		}
		if err := tx.Bucket(bktTeamSlug).Delete([]byte(key(t.TenantID, t.Slug))); err != nil {
			return err
		}
		return tx.Bucket(bktTeams).Delete([]byte(id))
	})
}

// PutTeamMember implements domain.IAMStore (insert or replace; the team
// must exist). An empty Role defaults to member.
func (b *Bolt) PutTeamMember(ctx context.Context, m *domain.TeamMember) error {
	if m == nil {
		return errNil("team member")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("team id", m.TeamID, "account id", m.AccountID); err != nil {
			return err
		}
		if tx.Bucket(bktTeams).Get([]byte(m.TeamID)) == nil {
			return notFound("team", m.TeamID)
		}
		if err := prepareTeamMember(m); err != nil {
			return err
		}
		mb := tx.Bucket(bktTeamMembers)
		k := []byte(key(m.TeamID, m.AccountID))
		if m.CreatedAt.IsZero() {
			old, err := getRec[domain.TeamMember](mb, k)
			if err != nil {
				return err
			}
			if old != nil {
				m.CreatedAt = old.CreatedAt
			} else {
				m.CreatedAt = now()
			}
		}
		if err := tx.Bucket(bktTeamMembersByAcct).Put([]byte(key(m.AccountID, m.TeamID)), k); err != nil {
			return err
		}
		return putJSON(mb, k, normTeamMember(*m))
	})
}

// DeleteTeamMember implements domain.IAMStore.
func (b *Bolt) DeleteTeamMember(ctx context.Context, teamID, accountID string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		mb := tx.Bucket(bktTeamMembers)
		k := []byte(key(teamID, accountID))
		if mb.Get(k) == nil {
			return notFound("team member", teamID, accountID)
		}
		if err := tx.Bucket(bktTeamMembersByAcct).Delete([]byte(key(accountID, teamID))); err != nil {
			return err
		}
		return mb.Delete(k)
	})
}

// ListTeamMembers implements domain.IAMStore (ordered by account id).
func (b *Bolt) ListTeamMembers(ctx context.Context, teamID string) ([]domain.TeamMember, error) {
	return listPrefix[domain.TeamMember](ctx, b, bktTeamMembers, teamID)
}

// ListAccountTeams implements domain.IAMStore (ordered by team id).
func (b *Bolt) ListAccountTeams(ctx context.Context, accountID string) ([]domain.TeamMember, error) {
	out := []domain.TeamMember{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		mb := tx.Bucket(bktTeamMembers)
		return scanPrefix(tx.Bucket(bktTeamMembersByAcct), []byte(prefix(accountID)), func(_, pk []byte) error {
			m, err := getRec[domain.TeamMember](mb, pk)
			if err != nil {
				return err
			}
			if m != nil {
				out = append(out, *m)
			}
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// ---- custom roles -----------------------------------------------------

// PutRole implements domain.IAMStore (insert or replace). Built-in role
// names are rejected with ErrInvalid.
func (b *Bolt) PutRole(ctx context.Context, r *domain.CustomRole) error {
	if r == nil {
		return errNil("role")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tenant id", r.TenantID, "role name", r.Name); err != nil {
			return err
		}
		bk := tx.Bucket(bktRoles)
		k := []byte(key(r.TenantID, r.Name))
		stored, err := getRec[domain.CustomRole](bk, k)
		if err != nil {
			return err
		}
		if err := prepareRole(r, stored); err != nil {
			return err
		}
		return putJSON(bk, k, normRole(*r))
	})
}

// GetRole implements domain.IAMStore.
func (b *Bolt) GetRole(ctx context.Context, tenantID, name string) (*domain.CustomRole, error) {
	var out *domain.CustomRole
	err := b.view(ctx, func(tx *bolt.Tx) error {
		r, err := getRec[domain.CustomRole](tx.Bucket(bktRoles), []byte(key(tenantID, name)))
		if err != nil {
			return err
		}
		if r == nil {
			return notFound("role", tenantID, name)
		}
		n := normRole(*r)
		out = &n
		return nil
	})
	return out, err
}

// ListRoles implements domain.IAMStore (ordered by name).
func (b *Bolt) ListRoles(ctx context.Context, tenantID string) ([]domain.CustomRole, error) {
	out, err := listPrefix[domain.CustomRole](ctx, b, bktRoles, tenantID)
	if err != nil {
		return nil, err
	}
	for i := range out {
		out[i] = normRole(out[i])
	}
	return out, nil
}

// DeleteRole implements domain.IAMStore.
func (b *Bolt) DeleteRole(ctx context.Context, tenantID, name string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktRoles)
		k := []byte(key(tenantID, name))
		if bk.Get(k) == nil {
			return notFound("role", tenantID, name)
		}
		return bk.Delete(k)
	})
}

// ---- policies ---------------------------------------------------------

func getPolicyTx(tx *bolt.Tx, tenantID, id string) (*domain.Policy, error) {
	p, err := getRec[domain.Policy](tx.Bucket(bktPolicies), []byte(key(tenantID, id)))
	if err != nil {
		return nil, err
	}
	if p == nil {
		return nil, notFound("policy", tenantID, id)
	}
	n := normPolicy(*p)
	return &n, nil
}

// CreatePolicy implements domain.IAMStore. Version is set to 1.
func (b *Bolt) CreatePolicy(ctx context.Context, p *domain.Policy) error {
	if p == nil {
		return errNil("policy")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := preparePolicy(p); err != nil {
			return err
		}
		bk := tx.Bucket(bktPolicies)
		k := []byte(key(p.TenantID, p.ID))
		if bk.Get(k) != nil {
			return alreadyExists("policy", p.TenantID, p.ID)
		}
		return putJSON(bk, k, normPolicy(*p))
	})
}

// GetPolicy implements domain.IAMStore.
func (b *Bolt) GetPolicy(ctx context.Context, tenantID, id string) (*domain.Policy, error) {
	var out *domain.Policy
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getPolicyTx(tx, tenantID, id)
		return err
	})
	return out, err
}

// ListPolicies implements domain.IAMStore (ordered by name, then id).
func (b *Bolt) ListPolicies(ctx context.Context, tenantID string) ([]domain.Policy, error) {
	out, err := listPrefix[domain.Policy](ctx, b, bktPolicies, tenantID)
	if err != nil {
		return nil, err
	}
	for i := range out {
		out[i] = normPolicy(out[i])
	}
	sortPolicies(out)
	return out, nil
}

// UpdatePolicy implements domain.IAMStore. The policy is located by
// (TenantID, ID); Version becomes the stored version + 1 and UpdatedAt now.
func (b *Bolt) UpdatePolicy(ctx context.Context, p *domain.Policy) error {
	if p == nil {
		return errNil("policy")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tenant id", p.TenantID, "policy id", p.ID); err != nil {
			return err
		}
		old, err := getPolicyTx(tx, p.TenantID, p.ID)
		if err != nil {
			return err
		}
		if err := checkPolicyUpdate(p, old); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktPolicies), []byte(key(p.TenantID, p.ID)), normPolicy(*p))
	})
}

// DeletePolicy implements domain.IAMStore.
func (b *Bolt) DeletePolicy(ctx context.Context, tenantID, id string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktPolicies)
		k := []byte(key(tenantID, id))
		if bk.Get(k) == nil {
			return notFound("policy", tenantID, id)
		}
		return bk.Delete(k)
	})
}

// ---- grants -----------------------------------------------------------

// checkGrantRefsTx verifies that the tree and (for team / service-account
// subjects) the subject of g exist.
func checkGrantRefsTx(tx *bolt.Tx, g *domain.Grant) error {
	if tx.Bucket(bktTrees).Get([]byte(g.TreeID)) == nil {
		return notFound("tree", g.TreeID)
	}
	switch g.SubjectType {
	case domain.SubjectTeam:
		if tx.Bucket(bktTeams).Get([]byte(g.SubjectID)) == nil {
			return notFound("team", g.SubjectID)
		}
	case domain.SubjectServiceAccount:
		if tx.Bucket(bktSAs).Get([]byte(g.SubjectID)) == nil {
			return notFound("service account", g.SubjectID)
		}
	}
	return nil
}

// PutGrant implements domain.IAMStore (insert or replace, keyed by tree,
// subject type and subject id).
func (b *Bolt) PutGrant(ctx context.Context, g *domain.Grant) error {
	if g == nil {
		return errNil("grant")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkGrant(g); err != nil {
			return err
		}
		if err := checkGrantRefsTx(tx, g); err != nil {
			return err
		}
		gb := tx.Bucket(bktGrants)
		k := []byte(key(g.TreeID, g.SubjectType, g.SubjectID))
		stored, err := getRec[domain.Grant](gb, k)
		if err != nil {
			return err
		}
		if err := prepareGrant(g, stored); err != nil {
			return err
		}
		if err := tx.Bucket(bktGrantsBySubject).Put([]byte(key(g.SubjectType, g.SubjectID, g.TreeID)), k); err != nil {
			return err
		}
		return putJSON(gb, k, normGrant(*g))
	})
}

// ListGrants implements domain.IAMStore (ordered by subject type, subject
// name, subject id).
func (b *Bolt) ListGrants(ctx context.Context, treeID string) ([]domain.Grant, error) {
	out, err := listPrefix[domain.Grant](ctx, b, bktGrants, treeID)
	if err != nil {
		return nil, err
	}
	sortGrantsBySubject(out)
	return out, nil
}

// ListSubjectGrants implements domain.IAMStore (ordered by tree id).
func (b *Bolt) ListSubjectGrants(ctx context.Context, subjectType, subjectID string) ([]domain.Grant, error) {
	out := []domain.Grant{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		gb := tx.Bucket(bktGrants)
		return scanPrefix(tx.Bucket(bktGrantsBySubject), []byte(prefix(key(subjectType, subjectID))), func(_, pk []byte) error {
			g, err := getRec[domain.Grant](gb, pk)
			if err != nil {
				return err
			}
			if g != nil {
				out = append(out, *g)
			}
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// DeleteGrant implements domain.IAMStore.
func (b *Bolt) DeleteGrant(ctx context.Context, treeID, subjectType, subjectID string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		gb := tx.Bucket(bktGrants)
		k := []byte(key(treeID, subjectType, subjectID))
		if gb.Get(k) == nil {
			return notFound("grant", treeID, subjectType, subjectID)
		}
		if err := tx.Bucket(bktGrantsBySubject).Delete([]byte(key(subjectType, subjectID, treeID))); err != nil {
			return err
		}
		return gb.Delete(k)
	})
}

// deleteTreeGrantsTx removes every grant on treeID.
func deleteTreeGrantsTx(tx *bolt.Tx, treeID string) error {
	gb, xb := tx.Bucket(bktGrants), tx.Bucket(bktGrantsBySubject)
	p := []byte(prefix(treeID))
	var subjectKeys [][]byte
	err := scanPrefix(gb, p, func(_, v []byte) error {
		var g domain.Grant
		if err := json.Unmarshal(v, &g); err != nil {
			return fmt.Errorf("meta: decode grant: %w", err)
		}
		subjectKeys = append(subjectKeys, []byte(key(g.SubjectType, g.SubjectID, treeID)))
		return nil
	})
	if err != nil {
		return err
	}
	for _, k := range subjectKeys {
		if err := xb.Delete(k); err != nil {
			return err
		}
	}
	_, err = deletePrefix(gb, p)
	return err
}

// deleteSubjectGrantsTx removes every grant held by the subject.
func deleteSubjectGrantsTx(tx *bolt.Tx, subjectType, subjectID string) error {
	gb, xb := tx.Bucket(bktGrants), tx.Bucket(bktGrantsBySubject)
	p := []byte(prefix(key(subjectType, subjectID)))
	var grantKeys [][]byte
	_ = scanPrefix(xb, p, func(_, pk []byte) error {
		grantKeys = append(grantKeys, slices.Clone(pk))
		return nil
	})
	for _, k := range grantKeys {
		if err := gb.Delete(k); err != nil {
			return err
		}
	}
	_, err := deletePrefix(xb, p)
	return err
}

// ---- service accounts -------------------------------------------------

func getServiceAccountTx(tx *bolt.Tx, id string) (*domain.ServiceAccount, error) {
	sa, err := getRec[domain.ServiceAccount](tx.Bucket(bktSAs), []byte(id))
	if err != nil {
		return nil, err
	}
	if sa == nil {
		return nil, notFound("service account", id)
	}
	return sa, nil
}

// CreateServiceAccount implements domain.IAMStore.
func (b *Bolt) CreateServiceAccount(ctx context.Context, sa *domain.ServiceAccount) error {
	if sa == nil {
		return errNil("service account")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareServiceAccount(sa); err != nil {
			return err
		}
		sb, ib := tx.Bucket(bktSAs), tx.Bucket(bktSAName)
		if sb.Get([]byte(sa.ID)) != nil {
			return alreadyExists("service account", sa.ID)
		}
		nk := []byte(key(sa.TenantID, sa.Name))
		if ib.Get(nk) != nil {
			return alreadyExists("service account", sa.TenantID, sa.Name)
		}
		if err := ib.Put(nk, []byte(sa.ID)); err != nil {
			return err
		}
		return putJSON(sb, []byte(sa.ID), normServiceAccount(*sa))
	})
}

// GetServiceAccount implements domain.IAMStore.
func (b *Bolt) GetServiceAccount(ctx context.Context, id string) (*domain.ServiceAccount, error) {
	var out *domain.ServiceAccount
	err := b.view(ctx, func(tx *bolt.Tx) error {
		var err error
		out, err = getServiceAccountTx(tx, id)
		return err
	})
	return out, err
}

// GetServiceAccountByName implements domain.IAMStore.
func (b *Bolt) GetServiceAccountByName(ctx context.Context, tenantID, name string) (*domain.ServiceAccount, error) {
	var out *domain.ServiceAccount
	err := b.view(ctx, func(tx *bolt.Tx) error {
		id := tx.Bucket(bktSAName).Get([]byte(key(tenantID, name)))
		if id == nil {
			return notFound("service account", tenantID, name)
		}
		var err error
		out, err = getServiceAccountTx(tx, string(id))
		return err
	})
	return out, err
}

// ListServiceAccounts implements domain.IAMStore (ordered by name).
func (b *Bolt) ListServiceAccounts(ctx context.Context, tenantID string) ([]domain.ServiceAccount, error) {
	out := []domain.ServiceAccount{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return scanPrefix(tx.Bucket(bktSAName), []byte(prefix(tenantID)), func(_, id []byte) error {
			sa, err := getServiceAccountTx(tx, string(id))
			if err != nil {
				return err
			}
			out = append(out, *sa)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// UpdateServiceAccount implements domain.IAMStore. The name may change (it
// stays unique within the tenant); the tenant may not. UpdatedAt is set to
// now.
func (b *Bolt) UpdateServiceAccount(ctx context.Context, sa *domain.ServiceAccount) error {
	if sa == nil {
		return errNil("service account")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("service account id", sa.ID, "tenant id", sa.TenantID, "service account name", sa.Name); err != nil {
			return err
		}
		old, err := getServiceAccountTx(tx, sa.ID)
		if err != nil {
			return err
		}
		ib := tx.Bucket(bktSAName)
		nk := []byte(key(sa.TenantID, sa.Name))
		if owner := ib.Get(nk); owner != nil && string(owner) != sa.ID {
			return alreadyExists("service account", sa.TenantID, sa.Name)
		}
		if err := checkServiceAccountUpdate(sa, old); err != nil {
			return err
		}
		if err := ib.Delete([]byte(key(old.TenantID, old.Name))); err != nil {
			return err
		}
		if err := ib.Put(nk, []byte(sa.ID)); err != nil {
			return err
		}
		return putJSON(tx.Bucket(bktSAs), []byte(sa.ID), normServiceAccount(*sa))
	})
}

// DeleteServiceAccount implements domain.IAMStore. It removes the
// account's keys and grants.
func (b *Bolt) DeleteServiceAccount(ctx context.Context, id string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		sa, err := getServiceAccountTx(tx, id)
		if err != nil {
			return err
		}
		var tokenIDs []string
		_ = scanPrefix(tx.Bucket(bktTokensBySA), []byte(prefix(id)), func(_, tid []byte) error {
			tokenIDs = append(tokenIDs, string(tid))
			return nil
		})
		for _, tid := range tokenIDs {
			t, err := getTokenTx(tx, tid)
			if err != nil {
				return err
			}
			if err := deleteTokenTx(tx, t); err != nil {
				return err
			}
		}
		if err := deleteSubjectGrantsTx(tx, domain.SubjectServiceAccount, id); err != nil {
			return err
		}
		if err := tx.Bucket(bktSAName).Delete([]byte(key(sa.TenantID, sa.Name))); err != nil {
			return err
		}
		return tx.Bucket(bktSAs).Delete([]byte(id))
	})
}

// ListServiceAccountTokens implements domain.IAMStore (ordered by creation
// time).
func (b *Bolt) ListServiceAccountTokens(ctx context.Context, serviceAccountID string) ([]domain.APIToken, error) {
	out := []domain.APIToken{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return scanPrefix(tx.Bucket(bktTokensBySA), []byte(prefix(serviceAccountID)), func(_, id []byte) error {
			t, err := getTokenTx(tx, string(id))
			if err != nil {
				return err
			}
			out = append(out, *t)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	sortTokens(out)
	return out, nil
}

// ---- MFA --------------------------------------------------------------

// GetMFA implements domain.IAMStore (ErrNotFound when the account has no
// MFA state).
func (b *Bolt) GetMFA(ctx context.Context, accountID string) (*domain.MFAState, error) {
	var out *domain.MFAState
	err := b.view(ctx, func(tx *bolt.Tx) error {
		rec, err := getRec[mfaRec](tx.Bucket(bktMFA), []byte(accountID))
		if err != nil {
			return err
		}
		if rec == nil {
			return notFound("mfa state", accountID)
		}
		m := rec.MFAState
		m.TOTPSecret = rec.TOTPSecret
		m.PendingSecret = rec.PendingSecret
		m.RecoveryCodeHashes = rec.RecoveryCodeHashes
		m.LastUsedStep = rec.LastUsedStep
		m = normMFA(m)
		out = &m
		return nil
	})
	return out, err
}

// PutMFA implements domain.IAMStore (insert or replace).
func (b *Bolt) PutMFA(ctx context.Context, m *domain.MFAState) error {
	if m == nil {
		return errNil("mfa state")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareMFA(m); err != nil {
			return err
		}
		n := normMFA(*m)
		return putJSON(tx.Bucket(bktMFA), []byte(n.AccountID), mfaRec{
			MFAState:           n,
			TOTPSecret:         n.TOTPSecret,
			PendingSecret:      n.PendingSecret,
			RecoveryCodeHashes: n.RecoveryCodeHashes,
			LastUsedStep:       n.LastUsedStep,
		})
	})
}

// DeleteMFA implements domain.IAMStore.
func (b *Bolt) DeleteMFA(ctx context.Context, accountID string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktMFA)
		if bk.Get([]byte(accountID)) == nil {
			return notFound("mfa state", accountID)
		}
		return bk.Delete([]byte(accountID))
	})
}

// ---- invitations ------------------------------------------------------

func decodeInvitation(v []byte) (domain.Invitation, error) {
	var rec invitationRec
	if err := json.Unmarshal(v, &rec); err != nil {
		return domain.Invitation{}, fmt.Errorf("meta: decode invitation: %w", err)
	}
	inv := rec.Invitation
	inv.CodeHash = rec.CodeHash
	return inv, nil
}

func getInvitationTx(tx *bolt.Tx, pk []byte) (*domain.Invitation, bool, error) {
	v := tx.Bucket(bktInvites).Get(pk)
	if v == nil {
		return nil, false, nil
	}
	inv, err := decodeInvitation(v)
	if err != nil {
		return nil, false, err
	}
	return &inv, true, nil
}

func putInvitationTx(tx *bolt.Tx, inv domain.Invitation) error {
	n := normInvitation(inv)
	return putJSON(tx.Bucket(bktInvites), []byte(key(n.TenantID, n.ID)), invitationRec{Invitation: n, CodeHash: n.CodeHash})
}

// CreateInvitation implements domain.IAMStore. CodeHash is required and
// unique across all invitations.
func (b *Bolt) CreateInvitation(ctx context.Context, inv *domain.Invitation) error {
	if inv == nil {
		return errNil("invitation")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := prepareInvitation(inv); err != nil {
			return err
		}
		pk := []byte(key(inv.TenantID, inv.ID))
		if tx.Bucket(bktInvites).Get(pk) != nil {
			return alreadyExists("invitation", inv.TenantID, inv.ID)
		}
		cb := tx.Bucket(bktInviteCode)
		if cb.Get([]byte(inv.CodeHash)) != nil {
			return alreadyExists("invitation code hash", "")
		}
		if err := cb.Put([]byte(inv.CodeHash), pk); err != nil {
			return err
		}
		return putInvitationTx(tx, *inv)
	})
}

// GetInvitation implements domain.IAMStore.
func (b *Bolt) GetInvitation(ctx context.Context, tenantID, id string) (*domain.Invitation, error) {
	var out *domain.Invitation
	err := b.view(ctx, func(tx *bolt.Tx) error {
		inv, ok, err := getInvitationTx(tx, []byte(key(tenantID, id)))
		if err != nil {
			return err
		}
		if !ok {
			return notFound("invitation", tenantID, id)
		}
		out = inv
		return nil
	})
	return out, err
}

// GetInvitationByCodeHash implements domain.IAMStore.
func (b *Bolt) GetInvitationByCodeHash(ctx context.Context, codeHash string) (*domain.Invitation, error) {
	var out *domain.Invitation
	err := b.view(ctx, func(tx *bolt.Tx) error {
		pk := tx.Bucket(bktInviteCode).Get([]byte(codeHash))
		if pk == nil {
			return notFound("invitation code hash", "")
		}
		inv, ok, err := getInvitationTx(tx, pk)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("invitation code hash", "")
		}
		out = inv
		return nil
	})
	return out, err
}

// ListInvitations implements domain.IAMStore (newest first).
func (b *Bolt) ListInvitations(ctx context.Context, tenantID string) ([]domain.Invitation, error) {
	out := []domain.Invitation{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		return scanPrefix(tx.Bucket(bktInvites), []byte(prefix(tenantID)), func(_, v []byte) error {
			inv, err := decodeInvitation(v)
			if err != nil {
				return err
			}
			out = append(out, inv)
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	sortInvitations(out)
	return out, nil
}

// UpdateInvitation implements domain.IAMStore. The invitation is located
// by (TenantID, ID); an empty CodeHash keeps the stored one.
func (b *Bolt) UpdateInvitation(ctx context.Context, inv *domain.Invitation) error {
	if inv == nil {
		return errNil("invitation")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("tenant id", inv.TenantID, "invitation id", inv.ID); err != nil {
			return err
		}
		pk := []byte(key(inv.TenantID, inv.ID))
		old, ok, err := getInvitationTx(tx, pk)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("invitation", inv.TenantID, inv.ID)
		}
		cb := tx.Bucket(bktInviteCode)
		if inv.CodeHash != "" && inv.CodeHash != old.CodeHash {
			if owner := cb.Get([]byte(inv.CodeHash)); owner != nil && !bytes.Equal(owner, pk) {
				return alreadyExists("invitation code hash", "")
			}
		}
		if err := checkInvitationUpdate(inv, old); err != nil {
			return err
		}
		if inv.CodeHash != old.CodeHash {
			if err := cb.Delete([]byte(old.CodeHash)); err != nil {
				return err
			}
			if err := cb.Put([]byte(inv.CodeHash), pk); err != nil {
				return err
			}
		}
		return putInvitationTx(tx, *inv)
	})
}

// DeleteInvitation implements domain.IAMStore.
func (b *Bolt) DeleteInvitation(ctx context.Context, tenantID, id string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		pk := []byte(key(tenantID, id))
		inv, ok, err := getInvitationTx(tx, pk)
		if err != nil {
			return err
		}
		if !ok {
			return notFound("invitation", tenantID, id)
		}
		if err := tx.Bucket(bktInviteCode).Delete([]byte(inv.CodeHash)); err != nil {
			return err
		}
		return tx.Bucket(bktInvites).Delete(pk)
	})
}

// ---- password resets --------------------------------------------------

// PutPasswordReset implements domain.IAMStore (insert or replace, keyed by
// code hash).
func (b *Bolt) PutPasswordReset(ctx context.Context, r *domain.PasswordReset) error {
	if r == nil {
		return errNil("password reset")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := preparePasswordReset(r); err != nil {
			return err
		}
		n := normPasswordReset(*r)
		return putJSON(tx.Bucket(bktResets), []byte(n.CodeHash), passwordResetRec{PasswordReset: n, CodeHash: n.CodeHash})
	})
}

// GetPasswordReset implements domain.IAMStore.
func (b *Bolt) GetPasswordReset(ctx context.Context, codeHash string) (*domain.PasswordReset, error) {
	var out *domain.PasswordReset
	err := b.view(ctx, func(tx *bolt.Tx) error {
		rec, err := getRec[passwordResetRec](tx.Bucket(bktResets), []byte(codeHash))
		if err != nil {
			return err
		}
		if rec == nil {
			return notFound("password reset", "")
		}
		r := rec.PasswordReset
		r.CodeHash = rec.CodeHash
		out = &r
		return nil
	})
	return out, err
}

// DeletePasswordReset implements domain.IAMStore.
func (b *Bolt) DeletePasswordReset(ctx context.Context, codeHash string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		bk := tx.Bucket(bktResets)
		if bk.Get([]byte(codeHash)) == nil {
			return notFound("password reset", "")
		}
		return bk.Delete([]byte(codeHash))
	})
}

// ---- SSO --------------------------------------------------------------

// GetSSOConfig implements domain.IAMStore (ErrNotFound if never set).
func (b *Bolt) GetSSOConfig(ctx context.Context) (*domain.SSOConfig, error) {
	var out *domain.SSOConfig
	err := b.view(ctx, func(tx *bolt.Tx) error {
		rec, err := getRec[ssoConfigRec](tx.Bucket(bktSSO), ssoConfigKey)
		if err != nil {
			return err
		}
		if rec == nil {
			return notFound("sso config", "")
		}
		c := rec.SSOConfig
		c.ClientSecret = rec.ClientSecret
		c = normSSOConfig(c)
		out = &c
		return nil
	})
	return out, err
}

// PutSSOConfig implements domain.IAMStore (replace). HasSecret is derived
// from ClientSecret and UpdatedAt is set to now.
func (b *Bolt) PutSSOConfig(ctx context.Context, c *domain.SSOConfig) error {
	if c == nil {
		return errNil("sso config")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		prepareSSOConfig(c)
		n := normSSOConfig(*c)
		return putJSON(tx.Bucket(bktSSO), ssoConfigKey, ssoConfigRec{SSOConfig: n, ClientSecret: n.ClientSecret})
	})
}

// PutSSOIdentity implements domain.IAMStore (insert or replace, keyed by
// issuer and subject).
func (b *Bolt) PutSSOIdentity(ctx context.Context, id *domain.SSOIdentity) error {
	if id == nil {
		return errNil("sso identity")
	}
	return b.update(ctx, func(tx *bolt.Tx) error {
		if err := checkKeys("sso issuer", id.Issuer, "sso subject", id.Subject, "account id", id.AccountID); err != nil {
			return err
		}
		ib, xb := tx.Bucket(bktSSOIdent), tx.Bucket(bktSSOIdentByAcct)
		pk := []byte(key(id.Issuer, id.Subject))
		stored, err := getRec[domain.SSOIdentity](ib, pk)
		if err != nil {
			return err
		}
		if err := prepareSSOIdentity(id, stored); err != nil {
			return err
		}
		if stored != nil && stored.AccountID != id.AccountID {
			if err := xb.Delete([]byte(key(stored.AccountID, id.Issuer, id.Subject))); err != nil {
				return err
			}
		}
		if err := xb.Put([]byte(key(id.AccountID, id.Issuer, id.Subject)), pk); err != nil {
			return err
		}
		return putJSON(ib, pk, normSSOIdentity(*id))
	})
}

// GetSSOIdentity implements domain.IAMStore.
func (b *Bolt) GetSSOIdentity(ctx context.Context, issuer, subject string) (*domain.SSOIdentity, error) {
	var out *domain.SSOIdentity
	err := b.view(ctx, func(tx *bolt.Tx) error {
		id, err := getRec[domain.SSOIdentity](tx.Bucket(bktSSOIdent), []byte(key(issuer, subject)))
		if err != nil {
			return err
		}
		if id == nil {
			return notFound("sso identity", issuer, subject)
		}
		out = id
		return nil
	})
	return out, err
}

// ListAccountSSOIdentities implements domain.IAMStore (ordered by issuer,
// then subject).
func (b *Bolt) ListAccountSSOIdentities(ctx context.Context, accountID string) ([]domain.SSOIdentity, error) {
	out := []domain.SSOIdentity{}
	err := b.view(ctx, func(tx *bolt.Tx) error {
		ib := tx.Bucket(bktSSOIdent)
		return scanPrefix(tx.Bucket(bktSSOIdentByAcct), []byte(prefix(accountID)), func(_, pk []byte) error {
			id, err := getRec[domain.SSOIdentity](ib, pk)
			if err != nil {
				return err
			}
			if id != nil {
				out = append(out, *id)
			}
			return nil
		})
	})
	if err != nil {
		return nil, err
	}
	return out, nil
}

var _ domain.IAMStore = (*Bolt)(nil)
