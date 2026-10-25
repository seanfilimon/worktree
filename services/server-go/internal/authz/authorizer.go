package authz

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"time"

	"google.golang.org/grpc/peer"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// Decision effects.
const (
	effectAllow        = "allow"
	effectDeny         = "deny"
	effectImplicitDeny = "implicit_deny"
	effectInfo         = "info"
)

// Decision reason sources (domain.DecisionReason.Source).
const (
	SourceServerAdmin    = "server_admin"
	SourceDisabled       = "disabled"
	SourceTenantRole     = "tenant_role"
	SourceTeamRole       = "team_role"
	SourceTreeGrant      = "tree_grant"
	SourceCustomRole     = "custom_role"
	SourceServiceAccount = "service_account"
	SourceVisibility     = "visibility"
	SourceTokenScope     = "token_scope"
	SourcePolicy         = "policy"
	SourceDefault        = "default"

	// v0.5 (§13.7).
	SourceOwnerProtected    = "owner_protected"    // a deny statement skipped for a tenant owner
	SourceTenantSuspended   = "tenant_suspended"   // writes refused: the tenant is suspended
	SourceTenantDeactivated = "tenant_deactivated" // everything refused: the tenant is deactivated
)

// Options configures an Authorizer.
type Options struct {
	// Now returns the current time, used for policy time conditions when
	// RequestEnv.Now is zero. nil = time.Now.
	Now func() time.Time
}

// Authorizer is the v0.4 authorization engine (docs/ENGINEERING_PLAN.md
// §12.1). It implements domain.Authorizer and domain.AccessEvaluator and is
// safe for concurrent use. See the package documentation for the model.
type Authorizer struct {
	store domain.MetaStore
	now   func() time.Time
}

var (
	_ domain.Authorizer      = (*Authorizer)(nil)
	_ domain.AccessEvaluator = (*Authorizer)(nil)
)

// New returns an Authorizer reading identities, roles, grants and policies
// from store.
func New(store domain.MetaStore, opts Options) *Authorizer {
	now := opts.Now
	if now == nil {
		now = time.Now
	}
	return &Authorizer{store: store, now: now}
}

// RequestEnvFrom returns the request environment of ctx: the client
// address recorded by iam.WithRemoteAddr (HTTP middleware), else the gRPC
// peer address, without the port. Now is left zero (= current time).
func RequestEnvFrom(ctx context.Context) domain.RequestEnv {
	addr := iam.RemoteAddrFrom(ctx)
	if addr == "" {
		if p, ok := peer.FromContext(ctx); ok && p.Addr != nil {
			addr = p.Addr.String()
		}
	}
	if ip, ok := parseRemoteIP(addr); ok {
		return domain.RequestEnv{RemoteIP: ip.String()}
	}
	return domain.RequestEnv{RemoteIP: addr}
}

// Authorize implements domain.Authorizer on top of Evaluate, with the
// request environment taken from ctx (RequestEnvFrom). A refusal wraps
// domain.ErrUnauthenticated for anonymous callers and domain.ErrForbidden
// otherwise; an unknown account or service account wraps
// ErrUnauthenticated; store failures are returned as is (wrapped).
func (a *Authorizer) Authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	d, msg, err := a.evaluate(ctx, p, act, domain.Resource{TenantID: tenantID, Tree: tree}, a.envFor(p, RequestEnvFrom(ctx), false))
	if err != nil {
		return err
	}
	if d.Allowed {
		return nil
	}
	if p == nil {
		return fmt.Errorf("authz: %s: %s: %w", act, msg, domain.ErrUnauthenticated)
	}
	return fmt.Errorf("authz: %s: %s: %w", act, msg, domain.ErrForbidden)
}

// RoleOf returns the account's (lower-cased) membership role in the
// tenant, or "" if it is not a member.
func (a *Authorizer) RoleOf(ctx context.Context, accountID, tenantID string) (string, error) {
	m, err := a.store.GetMembership(ctx, tenantID, accountID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return "", nil
		}
		return "", fmt.Errorf("authz: looking up membership: %w", err)
	}
	return strings.ToLower(m.Role), nil
}

// Evaluate decides whether p may perform act on res and explains why.
// env.Now defaults to the current time; env.RemoteIP may be empty (IP
// conditions then do not grant, and IP-conditioned denies apply). A branch
// or path resource needs a tree (ErrInvalid otherwise).
func (a *Authorizer) Evaluate(ctx context.Context, p *domain.Principal, act domain.Action, res domain.Resource, env domain.RequestEnv) (domain.Decision, error) {
	d, _, err := a.evaluate(ctx, p, act, res, a.envFor(p, env, false))
	return d, err
}

// EffectivePermissions lists, in catalog order, every action p may perform
// on tree (or on the tenant when tree is nil). Time and MFA conditions are
// evaluated against the current time and the principal; statements with IP
// conditions (which need a concrete request) are ignored.
func (a *Authorizer) EffectivePermissions(ctx context.Context, p *domain.Principal, tenantID string, tree *domain.Tree) ([]domain.Action, error) {
	ctx = WithRequestCache(ctx)
	env := a.envFor(p, domain.RequestEnv{}, true)
	out := make([]domain.Action, 0, len(catalog))
	for _, info := range catalog {
		d, _, err := a.evaluate(ctx, p, info.Action, domain.Resource{TenantID: tenantID, Tree: tree}, env)
		if err != nil {
			return nil, err
		}
		if d.Allowed {
			out = append(out, info.Action)
		}
	}
	return out, nil
}

// evalEnv is the normalized request environment.
type evalEnv struct {
	remoteIP  string
	now       time.Time
	mfa       bool
	effective bool // EffectivePermissions: skip statements that need a request
}

func (a *Authorizer) envFor(p *domain.Principal, env domain.RequestEnv, effective bool) evalEnv {
	now := env.Now
	if now.IsZero() {
		now = a.now()
	}
	return evalEnv{remoteIP: env.RemoteIP, now: now, mfa: p != nil && p.MFA, effective: effective}
}

// ---- evaluation bookkeeping ----

type evaluation struct {
	reasons     []domain.DecisionReason
	allowed     bool
	denied      bool
	denyMsg     string // first explicit deny, for error messages
	implicitMsg string
}

func (e *evaluation) add(source, effect, format string, args ...any) {
	e.reasons = append(e.reasons, domain.DecisionReason{Source: source, Detail: fmt.Sprintf(format, args...), Effect: effect})
}

func (e *evaluation) allow(source, format string, args ...any) {
	e.allowed = true
	e.add(source, effectAllow, format, args...)
}

func (e *evaluation) info(source, format string, args ...any) {
	e.add(source, effectInfo, format, args...)
}

// deny records an explicit deny; msg is the short form used in errors.
func (e *evaluation) deny(source, msg, format string, args ...any) {
	if !e.denied {
		e.denyMsg = msg
	}
	e.denied = true
	e.add(source, effectDeny, format, args...)
}

// refuse records an implicit deny reason (nothing allows the action).
func (e *evaluation) refuse(source, msg, format string, args ...any) {
	if e.implicitMsg == "" {
		e.implicitMsg = msg
	}
	e.add(source, effectDeny, format, args...)
}

func (e *evaluation) decision() (domain.Decision, string) {
	reasons := e.reasons
	if reasons == nil {
		reasons = []domain.DecisionReason{}
	}
	switch {
	case e.denied:
		return domain.Decision{Allowed: false, Effect: effectDeny, Reasons: reasons}, e.denyMsg
	case e.allowed:
		return domain.Decision{Allowed: true, Effect: effectAllow, Reasons: reasons}, ""
	}
	msg := e.implicitMsg
	if msg == "" {
		msg = "not permitted"
	}
	return domain.Decision{Allowed: false, Effect: effectImplicitDeny, Reasons: reasons}, msg
}

// subject is the resolved caller.
type subject struct {
	anonymous bool
	acct      *domain.Account        // users
	sa        *domain.ServiceAccount // service accounts
	name      string                 // for reasons
}

func (s *subject) admin() bool { return s.acct != nil && s.acct.IsAdmin }

// heldRole is one role the caller holds for the request.
type heldRole struct {
	source string // SourceTenantRole | SourceTeamRole | SourceTreeGrant | SourceServiceAccount
	role   string // lower-cased
	via    string // human description of where the role comes from
	tree   bool   // granted on the tree only (tree-scoped actions)
}

// evaluate runs the full §12.1 algorithm. It returns the decision, a short
// message for refusals, and an error only for invalid requests, unknown
// identities and store failures.
func (a *Authorizer) evaluate(ctx context.Context, p *domain.Principal, act domain.Action, res domain.Resource, env evalEnv) (domain.Decision, string, error) {
	c := cacheFrom(ctx)
	if c == nil {
		c = newRequestCache()
	}
	e := &evaluation{}

	// Normalize the resource.
	if res.Tree != nil {
		switch {
		case res.TenantID == "":
			res.TenantID = res.Tree.TenantID
		case res.TenantID != res.Tree.TenantID:
			e.deny(SourceDefault, "tree does not belong to tenant", "tree %q does not belong to the requested tenant", res.Tree.Name)
			d, msg := e.decision()
			return d, msg, nil
		}
	} else if res.Branch != "" || res.Path != "" {
		return domain.Decision{}, "", fmt.Errorf("authz: a branch or path resource needs a tree: %w", domain.ErrInvalid)
	}
	if !IsAction(act) {
		e.refuse(SourceDefault, "unknown action", "%q is not a known action", act)
		d, msg := e.decision()
		return d, msg, nil
	}

	// branch:push is tree:write plus per-branch denies.
	target := act
	if act == domain.ActBranchPush {
		target = domain.ActTreeWrite
		e.info(SourceDefault, "branch:push is allowed when tree:write is allowed and no policy denies the push")
	}

	// 1. Identity; disabled identities may do nothing.
	s, err := a.loadSubject(ctx, c, p)
	if err != nil {
		return domain.Decision{}, "", err
	}
	switch {
	case s.acct != nil && s.acct.Disabled:
		e.deny(SourceDisabled, "account is disabled", "account %s is disabled", s.name)
	case s.sa != nil && s.sa.Disabled:
		e.deny(SourceDisabled, "service account is disabled", "service account %s is disabled", s.name)
	}
	if e.denied {
		d, msg := e.decision()
		return d, msg, nil
	}

	// 3. Token scopes (checked before the admin bypass: they bind admins too).
	if !s.anonymous && len(p.Scopes) > 0 {
		scope := ""
		for _, sc := range p.Scopes {
			if actionPatternMatches(sc, target) {
				scope = sc
				break
			}
		}
		if scope == "" {
			e.deny(SourceTokenScope, "token scope does not permit it", "token scopes [%s] do not include %s", strings.Join(p.Scopes, ", "), target)
			d, msg := e.decision()
			return d, msg, nil
		}
		e.info(SourceTokenScope, "token scope %q permits %s", scope, target)
	}

	// 2. Server admins bypass roles and policies.
	if s.admin() {
		e.allow(SourceServerAdmin, "%s is a server admin; policies do not apply", s.name)
		d, msg := e.decision()
		return d, msg, nil
	}
	if act == domain.ActServerAdmin {
		msg := "server admin required"
		if s.anonymous {
			msg = "authentication required"
		}
		e.refuse(SourceServerAdmin, msg, "server:admin is reserved for server admins")
		d, m := e.decision()
		return d, m, nil
	}

	var tenant *domain.Tenant
	if res.TenantID != "" {
		if tenant, err = a.tenant(ctx, c, res.TenantID); err != nil {
			return domain.Decision{}, "", err
		}
	}

	// Tenant status (v0.5): suspended tenants are read-only, deactivated
	// ones refuse everything. Server admins were allowed above.
	if tenant != nil && checkTenantStatus(tenant, target, e) {
		d, msg := e.decision()
		return d, msg, nil
	}

	// 4. Roles.
	held, err := a.heldRoles(ctx, c, s, res, tenant, e)
	if err != nil {
		return domain.Decision{}, "", err
	}
	for _, h := range held {
		if err := a.applyRole(ctx, c, res.TenantID, h, target, e); err != nil {
			return domain.Decision{}, "", err
		}
	}

	// Visibility widens tree:read.
	if target == domain.ActTreeRead && res.Tree != nil {
		name := res.Tree.Name
		switch res.Tree.Visibility {
		case domain.VisibilityPublic:
			e.allow(SourceVisibility, "tree %q is public: anyone may read it", name)
		case domain.VisibilityInternal:
			if s.anonymous {
				e.info(SourceVisibility, "tree %q is internal: sign in to read it", name)
			} else {
				e.allow(SourceVisibility, "tree %q is internal: any signed-in principal may read it", name)
			}
		default:
			e.info(SourceVisibility, "tree %q is private: reading needs a role, grant or policy", name)
		}
	}

	// 5. Policies of the resource tenant.
	if tenant != nil {
		if err := a.applyPolicies(ctx, c, s, held, tenant, act, target, res, env, e); err != nil {
			return domain.Decision{}, "", err
		}
	}

	// 6. Default.
	if !e.denied && !e.allowed {
		switch {
		case s.anonymous:
			e.refuse(SourceDefault, "authentication required", "anonymous callers may only read public trees (or what a policy allows)")
		case res.TenantID == "":
			e.refuse(SourceDefault, "no tenant given", "no tenant given and no tenant-independent permission applies")
		case len(held) == 0:
			e.refuse(SourceDefault, "not a member of the tenant", "%s has no role in the tenant or on the tree and no policy allows %s", s.name, act)
		default:
			names := make([]string, 0, len(held))
			for _, h := range held {
				names = append(names, h.role)
			}
			e.refuse(SourceDefault, "role "+strings.Join(dedupe(names), ", ")+" does not permit it", "no role, grant, visibility rule or policy allows %s", act)
		}
	}
	d, msg := e.decision()
	return d, msg, nil
}

// loadSubject resolves the principal against the store, so that disabling
// and admin changes take effect immediately (stale principals are not
// trusted).
func (a *Authorizer) loadSubject(ctx context.Context, c *requestCache, p *domain.Principal) (*subject, error) {
	if p == nil {
		return &subject{anonymous: true, name: "anonymous"}, nil
	}
	if p.IsServiceAccount() {
		if p.ServiceAccountID == "" {
			return nil, fmt.Errorf("authz: service-account principal without id: %w", domain.ErrUnauthenticated)
		}
		sa, err := cached(c, "sa:"+p.ServiceAccountID, func() (*domain.ServiceAccount, error) {
			return a.store.GetServiceAccount(ctx, p.ServiceAccountID)
		})
		if err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				return nil, fmt.Errorf("authz: unknown service account: %w", domain.ErrUnauthenticated)
			}
			return nil, fmt.Errorf("authz: looking up service account: %w", err)
		}
		name := p.Username
		if name == "" {
			name = "sa:" + sa.Name
		}
		return &subject{sa: sa, name: name}, nil
	}
	acct, err := cached(c, "acct:"+p.AccountID, func() (*domain.Account, error) {
		return a.store.GetAccount(ctx, p.AccountID)
	})
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("authz: unknown account: %w", domain.ErrUnauthenticated)
		}
		return nil, fmt.Errorf("authz: looking up account: %w", err)
	}
	return &subject{acct: acct, name: acct.Username}, nil
}

// tenant returns the tenant, or nil when it does not exist.
func (a *Authorizer) tenant(ctx context.Context, c *requestCache, id string) (*domain.Tenant, error) {
	t, err := cached(c, "tenant:"+id, func() (*domain.Tenant, error) { return a.store.GetTenant(ctx, id) })
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("authz: looking up tenant: %w", err)
	}
	return t, nil
}

func (a *Authorizer) tenantSlug(ctx context.Context, c *requestCache, id string) (string, error) {
	t, err := a.tenant(ctx, c, id)
	if err != nil || t == nil {
		return "", err
	}
	return t.Slug, nil
}

// accountTeams returns the ids of the teams the account belongs to.
func (a *Authorizer) accountTeams(ctx context.Context, c *requestCache, accountID string) ([]domain.TeamMember, error) {
	ms, err := cached(c, "acctteams:"+accountID, func() ([]domain.TeamMember, error) {
		return a.store.ListAccountTeams(ctx, accountID)
	})
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("authz: listing team memberships: %w", err)
	}
	return ms, nil
}

// teamSlugs returns the slugs of the account's teams in tenantID.
func (a *Authorizer) teamSlugs(ctx context.Context, c *requestCache, accountID, tenantID string) ([]string, error) {
	ms, err := a.accountTeams(ctx, c, accountID)
	if err != nil {
		return nil, err
	}
	var out []string
	for _, m := range ms {
		t, err := cached(c, "team:"+m.TeamID, func() (*domain.Team, error) { return a.store.GetTeam(ctx, m.TeamID) })
		if errors.Is(err, domain.ErrNotFound) {
			continue
		}
		if err != nil {
			return nil, fmt.Errorf("authz: looking up team: %w", err)
		}
		if t.TenantID == tenantID {
			out = append(out, t.Slug)
		}
	}
	return out, nil
}

// heldRoles collects every role the subject holds for res: its tenant
// membership (users), its own role (service accounts in their tenant),
// direct tree grants and team grants on the tree.
func (a *Authorizer) heldRoles(ctx context.Context, c *requestCache, s *subject, res domain.Resource, tenant *domain.Tenant, e *evaluation) ([]heldRole, error) {
	if s.anonymous || res.TenantID == "" {
		return nil, nil
	}
	slug := res.TenantID
	if tenant != nil {
		slug = tenant.Slug
	}
	var held []heldRole
	switch {
	case s.acct != nil:
		m, err := cached(c, "mem:"+res.TenantID+"/"+s.acct.ID, func() (*domain.Membership, error) {
			return a.store.GetMembership(ctx, res.TenantID, s.acct.ID)
		})
		switch {
		case errors.Is(err, domain.ErrNotFound):
			e.info(SourceTenantRole, "%s is not a member of tenant %s", s.name, slug)
		case err != nil:
			return nil, fmt.Errorf("authz: looking up membership: %w", err)
		default:
			held = append(held, heldRole{source: SourceTenantRole, role: strings.ToLower(m.Role), via: "member of tenant " + slug})
		}
	case s.sa != nil:
		if s.sa.TenantID == res.TenantID {
			held = append(held, heldRole{source: SourceServiceAccount, role: strings.ToLower(s.sa.Role), via: "service account " + s.sa.Name})
		} else {
			e.info(SourceServiceAccount, "service account %s belongs to another tenant; its role does not apply in %s", s.name, slug)
		}
	}

	if res.Tree == nil {
		return held, nil
	}
	grants, err := cached(c, "grants:"+res.Tree.ID, func() ([]domain.Grant, error) {
		return a.store.ListGrants(ctx, res.Tree.ID)
	})
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return nil, fmt.Errorf("authz: listing tree grants: %w", err)
	}
	var teamIDs map[string]bool // loaded on the first team grant
	for _, g := range grants {
		switch g.SubjectType {
		case domain.SubjectUser:
			if s.acct != nil && g.SubjectID == s.acct.ID {
				held = append(held, heldRole{source: SourceTreeGrant, role: strings.ToLower(g.Role), via: fmt.Sprintf("grant on tree %q", res.Tree.Name), tree: true})
			}
		case domain.SubjectServiceAccount:
			if s.sa != nil && g.SubjectID == s.sa.ID {
				held = append(held, heldRole{source: SourceTreeGrant, role: strings.ToLower(g.Role), via: fmt.Sprintf("grant on tree %q", res.Tree.Name), tree: true})
			}
		case domain.SubjectTeam:
			if s.acct == nil {
				continue
			}
			if teamIDs == nil {
				ms, err := a.accountTeams(ctx, c, s.acct.ID)
				if err != nil {
					return nil, err
				}
				teamIDs = make(map[string]bool, len(ms))
				for _, m := range ms {
					teamIDs[m.TeamID] = true
				}
			}
			if teamIDs[g.SubjectID] {
				team := g.Subject
				if team == "" {
					team = g.SubjectID
				}
				held = append(held, heldRole{source: SourceTeamRole, role: strings.ToLower(g.Role), via: fmt.Sprintf("team %s grant on tree %q", team, res.Tree.Name), tree: true})
			}
		case domain.SubjectTenant:
			in, err := a.inTenant(ctx, c, s, g.SubjectID)
			if err != nil {
				return nil, err
			}
			if in {
				name := g.Subject
				if name == "" {
					name = g.SubjectID
				}
				held = append(held, heldRole{source: SourceTreeGrant, role: strings.ToLower(g.Role), via: fmt.Sprintf("tenant %s grant on tree %q", name, res.Tree.Name), tree: true})
			}
		}
	}
	return held, nil
}

// inTenant reports whether the subject belongs to tenantID: a user with a
// membership there, or a service account owned by it (tenant grants and
// "tenant:<slug>" policy principals).
func (a *Authorizer) inTenant(ctx context.Context, c *requestCache, s *subject, tenantID string) (bool, error) {
	switch {
	case s.sa != nil:
		return s.sa.TenantID == tenantID, nil
	case s.acct != nil:
		_, err := cached(c, "mem:"+tenantID+"/"+s.acct.ID, func() (*domain.Membership, error) {
			return a.store.GetMembership(ctx, tenantID, s.acct.ID)
		})
		switch {
		case errors.Is(err, domain.ErrNotFound):
			return false, nil
		case err != nil:
			return false, fmt.Errorf("authz: looking up membership: %w", err)
		}
		return true, nil
	}
	return false, nil
}

// memberTenantSlugs lists the slugs of every tenant the subject belongs to
// (for "tenant:<slug>" principals).
func (a *Authorizer) memberTenantSlugs(ctx context.Context, c *requestCache, s *subject) ([]string, error) {
	switch {
	case s.sa != nil:
		slug, err := a.tenantSlug(ctx, c, s.sa.TenantID)
		if err != nil || slug == "" {
			return nil, err
		}
		return []string{slug}, nil
	case s.acct != nil:
		ms, err := cached(c, "acctmems:"+s.acct.ID, func() ([]domain.Membership, error) {
			return a.store.ListAccountMemberships(ctx, s.acct.ID)
		})
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil
		}
		if err != nil {
			return nil, fmt.Errorf("authz: listing memberships: %w", err)
		}
		var out []string
		for _, m := range ms {
			slug, err := a.tenantSlug(ctx, c, m.TenantID)
			if err != nil {
				return nil, err
			}
			if slug != "" {
				out = append(out, slug)
			}
		}
		return out, nil
	}
	return nil, nil
}

// readOnlyActions are the actions a suspended tenant still allows.
var readOnlyActions = map[domain.Action]bool{
	domain.ActTenantRead: true,
	domain.ActTreeRead:   true,
	domain.ActIAMRead:    true,
}

// checkTenantStatus records the effect of the tenant's status on act and
// reports whether the request is refused because of it.
func checkTenantStatus(t *domain.Tenant, act domain.Action, e *evaluation) bool {
	why := ""
	if t.StatusReason != "" {
		why = " (" + t.StatusReason + ")"
	}
	switch t.Status {
	case domain.TenantDeactivated:
		e.deny(SourceTenantDeactivated, "tenant is deactivated", "tenant %s is deactivated%s: all access is refused", t.Slug, why)
		return true
	case domain.TenantSuspended:
		if !readOnlyActions[act] {
			e.deny(SourceTenantSuspended, "tenant is suspended", "tenant %s is suspended%s: %s is a write and is refused", t.Slug, why, act)
			return true
		}
		e.info(SourceTenantSuspended, "tenant %s is suspended%s: reads are still allowed", t.Slug, why)
	}
	return false
}

// applyRole records whether one held role grants act.
func (a *Authorizer) applyRole(ctx context.Context, c *requestCache, tenantID string, h heldRole, act domain.Action, e *evaluation) error {
	if h.tree && !treeScoped(act) {
		e.info(h.source, "%s: role %s (tree grants only cover tree actions, not %s)", h.via, h.role, act)
		return nil
	}
	if IsBuiltinRole(h.role) {
		if RoleAllows(h.role, act) {
			e.allow(h.source, "%s: role %s grants %s", h.via, h.role, act)
		} else {
			e.info(h.source, "%s: role %s does not grant %s", h.via, h.role, act)
		}
		return nil
	}
	if h.role == "" {
		e.info(h.source, "%s: no role", h.via)
		return nil
	}
	r, err := cached(c, "role:"+tenantID+"/"+h.role, func() (*domain.CustomRole, error) {
		return a.store.GetRole(ctx, tenantID, h.role)
	})
	if errors.Is(err, domain.ErrNotFound) {
		e.info(h.source, "%s: role %s", h.via, h.role)
		e.info(SourceCustomRole, "role %s is not defined in the tenant and grants nothing", h.role)
		return nil
	}
	if err != nil {
		return fmt.Errorf("authz: looking up role %q: %w", h.role, err)
	}
	e.info(h.source, "%s: custom role %s", h.via, h.role)
	if perm := customRoleGrant(r.Permissions, act); perm != "" {
		e.allow(SourceCustomRole, "custom role %s grants %s (permission %s)", h.role, act, perm)
	} else {
		e.info(SourceCustomRole, "custom role %s does not grant %s", h.role, act)
	}
	return nil
}

// policies returns the tenant's enabled policies, compiled.
func (a *Authorizer) policies(ctx context.Context, c *requestCache, tenantID string) ([]compiledPolicy, error) {
	if cand, ok := candidatesFor(ctx, tenantID); ok {
		return compilePolicies(cand), nil
	}
	pols, err := cached(c, "policies:"+tenantID, func() ([]compiledPolicy, error) {
		ps, err := a.store.ListPolicies(ctx, tenantID)
		if err != nil {
			return nil, err
		}
		return compilePolicies(ps), nil
	})
	if errors.Is(err, domain.ErrNotFound) {
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("authz: listing policies: %w", err)
	}
	return pols, nil
}

// requestResources builds the concrete resources of a request: the most
// specific resource and its ancestors (a statement on a tenant covers its
// trees, one on a tree covers its branches and paths).
func requestResources(slug string, res domain.Resource) (base []parsedResource, branch []parsedResource) {
	base = append(base, parsedResource{kind: kindTenant, segs: []string{slug}})
	if res.Tree == nil {
		return base, nil
	}
	base = append(base, parsedResource{kind: kindTree, segs: []string{slug, res.Tree.Name}})
	if p := splitPath(res.Path); len(p) > 0 {
		base = append(base, parsedResource{kind: kindPath, segs: append([]string{slug, res.Tree.Name}, p...)})
	}
	if b := splitPath(res.Branch); len(b) > 0 {
		branch = append(branch, parsedResource{kind: kindBranch, segs: append([]string{slug, res.Tree.Name}, b...)})
	}
	return base, branch
}

func splitPath(p string) []string {
	var out []string
	for _, s := range strings.Split(p, "/") {
		if s != "" {
			out = append(out, s)
		}
	}
	return out
}

func anyResourceMatches(patterns []parsedResource, reqs []parsedResource) bool {
	for _, p := range patterns {
		for _, r := range reqs {
			if resourcePatternMatches(p, r) {
				return true
			}
		}
	}
	return false
}

func anyActionMatches(patterns []string, act domain.Action) bool {
	for _, p := range patterns {
		if actionPatternMatches(p, act) {
			return true
		}
	}
	return false
}

func anyPrincipalMatches(patterns []string, f *principalFacts) bool {
	for _, p := range patterns {
		if principalMatches(p, f) {
			return true
		}
	}
	return false
}

// applyPolicies evaluates the tenant's enabled policies (step 5).
//
// For most actions a statement applies when one of its actions covers the
// action, one of its resources covers the request's resource or an
// ancestor of it, one of its principals covers the caller and its
// conditions hold. For branch:push the tree:write evaluation above is
// followed by a deny-only pass over the branch: a deny statement for
// branch:push on the branch, tree or tenant, or for tree:write on the
// branch itself, refuses the push.
func (a *Authorizer) applyPolicies(ctx context.Context, c *requestCache, s *subject, held []heldRole,
	tenant *domain.Tenant, act, target domain.Action, res domain.Resource, env evalEnv, e *evaluation) error {
	pols, err := a.policies(ctx, c, tenant.ID)
	if err != nil || len(pols) == 0 {
		return err
	}

	facts := &principalFacts{anonymous: s.anonymous}
	owner := false
	for _, h := range held {
		facts.roles = append(facts.roles, h.role)
		owner = owner || (h.source == SourceTenantRole && h.role == domain.RoleOwner)
	}
	// Lockout guard: tenant owners can always manage IAM and the tenant,
	// whatever the policies say.
	protected := owner && (areaOf(string(target)) == AreaIAM || target == domain.ActTenantAdmin)
	needTenants := false
	for _, p := range pols {
		needTenants = needTenants || p.usesTenants
	}
	if needTenants && !s.anonymous {
		if facts.tenants, err = a.memberTenantSlugs(ctx, c, s); err != nil {
			return err
		}
	}
	switch {
	case s.acct != nil:
		facts.username = s.acct.Username
		needTeams := false
		for _, p := range pols {
			needTeams = needTeams || p.usesTeams
		}
		if needTeams {
			if facts.teams, err = a.teamSlugs(ctx, c, s.acct.ID, tenant.ID); err != nil {
				return err
			}
		}
	case s.sa != nil:
		facts.saName = s.sa.Name
		facts.saLocal = s.sa.TenantID == tenant.ID
		if facts.saTenant, err = a.tenantSlug(ctx, c, s.sa.TenantID); err != nil {
			return err
		}
	}

	base, branch := requestResources(tenant.Slug, res)
	if act != domain.ActBranchPush {
		// Branch-specific actions (branch:create / branch:delete with a
		// branch) match statements on that branch too.
		base = append(base, branch...)
		branch = nil
	}
	full := append(append([]parsedResource(nil), base...), branch...)

	for pi := range pols {
		pol := &pols[pi]
		for si := range pol.statements {
			st := &pol.statements[si]
			if st.invalid != nil || (env.effective && st.needsRequest()) {
				continue
			}
			if !anyPrincipalMatches(st.principals, facts) {
				continue
			}
			matched := anyActionMatches(st.actions, target) && anyResourceMatches(st.resources, base)
			if !matched && act == domain.ActBranchPush && st.effect == domain.EffectDeny {
				matched = (anyActionMatches(st.actions, domain.ActBranchPush) && anyResourceMatches(st.resources, full)) ||
					(anyActionMatches(st.actions, domain.ActTreeWrite) && anyResourceMatches(st.resources, branch))
			}
			if !matched {
				continue
			}
			label := st.label(pol)
			cr := st.cond.check(env, st.effect == domain.EffectDeny)
			if !cr.holds {
				e.info(SourcePolicy, "%s (%s) does not apply: %s", label, st.effect, cr.detail)
				continue
			}
			suffix := ""
			if cr.detail != "" {
				suffix = " (" + cr.detail + ")"
			}
			if st.effect == domain.EffectDeny && protected {
				e.info(SourceOwnerProtected, "%s would deny %s, but policies never deny tenant owners iam:* or tenant:admin", label, act)
				continue
			}
			if st.effect == domain.EffectDeny {
				e.deny(SourcePolicy, fmt.Sprintf("denied by policy %q", pol.name), "%s denies %s%s", label, act, suffix)
			} else {
				e.allow(SourcePolicy, "%s allows %s%s", label, target, suffix)
			}
		}
	}
	return nil
}

func dedupe(xs []string) []string {
	seen := make(map[string]bool, len(xs))
	out := xs[:0:0]
	for _, x := range xs {
		if !seen[x] {
			seen[x] = true
			out = append(out, x)
		}
	}
	return out
}
