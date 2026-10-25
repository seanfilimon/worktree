package httpapi

import (
	"context"
	"errors"
	"net/http"
	"net/netip"
	"slices"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// maxPolicyCandidates bounds POST /iam/simulate candidate_policies.
const maxPolicyCandidates = 200

type simulatePrincipal struct {
	Type   string `json:"type"` // "user" | "service_account" ("sa" accepted)
	Name   string `json:"name"`
	Tenant string `json:"tenant"` // service accounts: owning tenant (default: the request tenant)
}

type simulateContext struct {
	IP  string `json:"ip"`
	MFA bool   `json:"mfa"`
}

type simulateRequest struct {
	Principal *simulatePrincipal `json:"principal"` // nil = the caller
	Action    string             `json:"action"`
	Tenant    string             `json:"tenant"`
	Tree      string             `json:"tree"`
	Branch    string             `json:"branch"`
	Path      string             `json:"path"`
	Context   simulateContext    `json:"context"`
	// CandidatePolicies, when present, replace the tenant's stored policies
	// for this evaluation: a dry run of a policy change (§13.7). Only
	// enabled candidates take part; [] simulates having no policies.
	CandidatePolicies *[]domain.Policy `json:"candidate_policies"`
}

type teamRoleView struct {
	Team string `json:"team"` // slug
	Role string `json:"role"` // team role: maintainer | member
}

type effectiveResponse struct {
	Principal   string          `json:"principal"` // "user:<name>" | "sa:<tenant>/<name>"
	Role        string          `json:"role"`      // tenant role ("" = none)
	TeamRoles   []teamRoleView  `json:"team_roles"`
	Grants      []domain.Grant  `json:"grants"`
	Permissions []domain.Action `json:"permissions"`
}

// access returns Deps.Access or a 501 error.
func (a *api) access() (domain.AccessEvaluator, error) {
	if a.d.Access == nil {
		return nil, errIAMNotImplemented("the access evaluator")
	}
	return a.d.Access, nil
}

// principalRef is a resolved subject of /iam/simulate or /iam/effective.
type principalRef struct {
	p    *domain.Principal
	acct *domain.Account        // users
	sa   *domain.ServiceAccount // service accounts
}

func (pr principalRef) String() string {
	if pr.sa != nil {
		return pr.p.Username
	}
	return "user:" + pr.p.Username
}

// isCaller reports whether pr is the request's own principal.
func (pr principalRef) isCaller(r *http.Request) bool {
	c := principal(r)
	if c == nil {
		return false
	}
	if pr.sa != nil {
		return c.ServiceAccountID != "" && c.ServiceAccountID == pr.sa.ID
	}
	return c.AccountID != "" && c.AccountID == pr.p.AccountID
}

// resolveUserPrincipal builds the principal of a user.
func (a *api) resolveUserPrincipal(ctx context.Context, username string) (principalRef, error) {
	acct, err := a.accountByUsername(ctx, strings.TrimSpace(username))
	if err != nil {
		return principalRef{}, err
	}
	return principalRef{acct: acct, p: &domain.Principal{
		AccountID: acct.ID, Username: acct.Username, TenantID: acct.TenantID, IsAdmin: acct.IsAdmin, Kind: domain.PrincipalUser,
	}}, nil
}

// resolveSAPrincipal builds the principal of service account name in the
// tenant tenantRef (slug or id).
func (a *api) resolveSAPrincipal(ctx context.Context, tenantRef, name string) (principalRef, error) {
	if tenantRef == "" {
		return principalRef{}, badRequestf("service account principals need a tenant")
	}
	t, err := a.tenantByRef(ctx, tenantRef)
	if err != nil {
		return principalRef{}, err
	}
	sa, err := a.d.Store.GetServiceAccountByName(ctx, t.ID, name)
	if errors.Is(err, domain.ErrNotFound) {
		return principalRef{}, notFoundf("service account %s/%s not found", t.Slug, name)
	}
	if err != nil {
		return principalRef{}, err
	}
	return principalRef{sa: sa, p: &domain.Principal{
		Username: "sa:" + t.Slug + "/" + sa.Name, TenantID: sa.TenantID,
		Kind: domain.PrincipalServiceAccount, ServiceAccountID: sa.ID,
	}}, nil
}

// parsePrincipalRef parses "user:<name>" or "sa:<tenant>/<name>" into a
// resolver (run after authorization) and, for service accounts, the
// owning tenant reference.
func (a *api) parsePrincipalRef(ctx context.Context, s string) (resolve func() (principalRef, error), saTenant string, err error) {
	kind, rest, _ := strings.Cut(strings.TrimSpace(s), ":")
	switch kind {
	case "user":
		if rest == "" {
			break
		}
		return func() (principalRef, error) { return a.resolveUserPrincipal(ctx, rest) }, "", nil
	case "sa":
		tenant, name, ok := strings.Cut(rest, "/")
		if !ok || tenant == "" || name == "" {
			break
		}
		return func() (principalRef, error) { return a.resolveSAPrincipal(ctx, tenant, name) }, tenant, nil
	}
	return nil, "", badRequestf("principal must be user:<name> or sa:<tenant>/<name>")
}

// callerRef describes the request's own principal.
func (a *api) callerRef(r *http.Request) (principalRef, error) {
	p, err := requireLogin(r)
	if err != nil {
		return principalRef{}, err
	}
	ctx := r.Context()
	if p.IsServiceAccount() {
		sa, err := a.d.Store.GetServiceAccount(ctx, p.ServiceAccountID)
		if err != nil {
			return principalRef{}, err
		}
		return principalRef{p: p, sa: sa}, nil
	}
	acct, err := a.d.Store.GetAccount(ctx, p.AccountID)
	if err != nil {
		return principalRef{}, err
	}
	return principalRef{p: p, acct: acct}, nil
}

// inspectTarget authorizes inspecting a principal in tenantID and then
// resolves it. Callers may always inspect themselves; inspecting others
// needs iam:read on the tenant. Callers without iam:read get the
// authorization error, never a lookup error, so that they cannot probe
// which accounts exist.
func (a *api) inspectTarget(r *http.Request, tenantID string, resolve func() (principalRef, error)) (principalRef, error) {
	authErr := a.d.Authz.Authorize(r.Context(), principal(r), domain.ActIAMRead, tenantID, nil)
	if authErr != nil && !errors.Is(authErr, domain.ErrForbidden) {
		return principalRef{}, authErr
	}
	pr, err := resolve()
	if authErr != nil {
		if err != nil || !pr.isCaller(r) {
			return principalRef{}, authErr
		}
		return pr, nil
	}
	return pr, err
}

// treeInTenant resolves a tree name (or "tenant/tree") within t.
func (a *api) treeInTenant(ctx context.Context, t *domain.Tenant, ref string) (*domain.Tree, error) {
	name := ref
	if prefix, rest, ok := strings.Cut(ref, "/"); ok {
		if prefix != t.Slug && prefix != t.ID {
			return nil, badRequestf("tree %q is not in tenant %s", ref, t.Slug)
		}
		name = rest
	}
	tree, err := a.d.Store.GetTreeByName(ctx, t.ID, name)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("tree %s/%s not found", t.Slug, name)
	}
	return tree, err
}

// handleIAMSimulate answers POST /api/v1/iam/simulate with the Decision of
// the access engine for a hypothetical request.
func (a *api) handleIAMSimulate(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	var req simulateRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Action = strings.TrimSpace(req.Action)
	req.Tenant = strings.TrimSpace(req.Tenant)
	if req.Action == "" {
		return badRequestf("action is required")
	}
	if !knownAction(req.Action) {
		return badRequestf("unknown action %q", req.Action)
	}
	if req.Tenant == "" {
		return badRequestf("tenant is required")
	}
	if req.Branch != "" {
		if err := validateBranchName(req.Branch); err != nil {
			return err
		}
	}
	path := strings.Trim(strings.TrimSpace(req.Path), "/")
	if err := validateText("path", path, 4096, false); err != nil {
		return err
	}
	env := domain.RequestEnv{Now: time.Now().UTC()}
	if ip := strings.TrimSpace(req.Context.IP); ip != "" {
		addr, err := netip.ParseAddr(ip)
		if err != nil {
			return badRequestf("context.ip must be an IP address")
		}
		env.RemoteIP = addr.String()
	}
	ctx := r.Context()
	t, err := a.tenantByRef(ctx, req.Tenant)
	if err != nil {
		return err
	}
	resolve := func() (principalRef, error) { return a.callerRef(r) }
	if sp := req.Principal; sp != nil && (sp.Type != "" || sp.Name != "") {
		name := strings.TrimSpace(sp.Name)
		if name == "" {
			return badRequestf("principal.name is required")
		}
		switch sp.Type {
		case domain.PrincipalUser:
			resolve = func() (principalRef, error) { return a.resolveUserPrincipal(ctx, name) }
		case domain.PrincipalServiceAccount, "sa":
			saTenant := strings.TrimSpace(sp.Tenant)
			if tn, n, ok := strings.Cut(name, "/"); ok && saTenant == "" {
				saTenant, name = tn, n
			}
			if saTenant == "" {
				saTenant = t.Slug
			}
			resolve = func() (principalRef, error) { return a.resolveSAPrincipal(ctx, saTenant, name) }
		default:
			return badRequestf("principal.type must be %q or %q", domain.PrincipalUser, domain.PrincipalServiceAccount)
		}
	}
	pr, err := a.inspectTarget(r, t.ID, resolve)
	if err != nil {
		return err
	}
	ev, err := a.access()
	if err != nil {
		return err
	}
	res := domain.Resource{TenantID: t.ID, Branch: req.Branch, Path: path}
	if tr := strings.TrimSpace(req.Tree); tr != "" {
		if res.Tree, err = a.treeInTenant(ctx, t, tr); err != nil {
			return err
		}
	} else if req.Branch != "" || path != "" {
		return badRequestf("branch and path need a tree")
	}
	if req.CandidatePolicies != nil {
		cands := *req.CandidatePolicies
		if len(cands) > maxPolicyCandidates {
			return badRequestf("at most %d candidate policies are allowed", maxPolicyCandidates)
		}
		for i := range cands {
			c := &cands[i]
			c.TenantID = t.ID
			if c.Name == "" {
				c.Name = "candidate-" + strconv.Itoa(i+1)
			}
			if err := statementsError(a.validateStatements(c.Statements)); err != nil {
				return badRequestf("candidate_policies[%d] (%s): %v", i, c.Name, err)
			}
		}
		ctx = authz.WithCandidatePolicies(ctx, t.ID, cands)
	}
	sim := *pr.p
	sim.MFA = req.Context.MFA
	dec, err := ev.Evaluate(ctx, &sim, domain.Action(req.Action), res, env)
	if err != nil {
		return err
	}
	dec.Reasons = nonNil(dec.Reasons)
	writeJSON(w, http.StatusOK, dec)
	return nil
}

// handleIAMEffective answers GET
// /api/v1/iam/effective?tenant=&tree=&principal=: the principal's tenant
// role, team memberships, applicable grants and effective permissions.
func (a *api) handleIAMEffective(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	ctx := r.Context()
	q := r.URL.Query()
	resolve := func() (principalRef, error) { return a.callerRef(r) }
	saTenant := ""
	if c := principal(r); c.IsServiceAccount() {
		saTenant = c.TenantID
	}
	if s := strings.TrimSpace(q.Get("principal")); s != "" {
		var err error
		if resolve, saTenant, err = a.parsePrincipalRef(ctx, s); err != nil {
			return err
		}
	}
	tenantRef := strings.TrimSpace(q.Get("tenant"))
	treeRef := strings.TrimSpace(q.Get("tree"))
	if tenantRef == "" {
		if prefix, _, ok := strings.Cut(treeRef, "/"); ok {
			tenantRef = prefix
		} else if saTenant != "" {
			tenantRef = saTenant
		} else {
			return badRequestf("tenant is required")
		}
	}
	t, err := a.tenantByRef(ctx, tenantRef)
	if err != nil {
		return err
	}
	pr, err := a.inspectTarget(r, t.ID, resolve)
	if err != nil {
		return err
	}
	ev, err := a.access()
	if err != nil {
		return err
	}
	var tree *domain.Tree
	if treeRef != "" {
		if tree, err = a.treeInTenant(ctx, t, treeRef); err != nil {
			return err
		}
	}

	out := effectiveResponse{Principal: pr.String(), TeamRoles: []teamRoleView{}, Grants: []domain.Grant{}}
	subjects := map[[2]string]bool{} // grant subject (type, id) of the principal
	if pr.sa != nil {
		if pr.sa.TenantID == t.ID {
			out.Role = pr.sa.Role
		}
		subjects[[2]string{domain.SubjectServiceAccount, pr.sa.ID}] = true
		subjects[[2]string{domain.SubjectTenant, pr.sa.TenantID}] = true
	} else {
		mems, err := a.d.Store.ListAccountMemberships(ctx, pr.p.AccountID)
		if err != nil {
			return err
		}
		for _, m := range mems {
			subjects[[2]string{domain.SubjectTenant, m.TenantID}] = true
		}
		if m, err := a.d.Store.GetMembership(ctx, t.ID, pr.p.AccountID); err == nil {
			out.Role = m.Role
		} else if !errors.Is(err, domain.ErrNotFound) {
			return err
		}
		subjects[[2]string{domain.SubjectUser, pr.p.AccountID}] = true
		tms, err := a.d.Store.ListAccountTeams(ctx, pr.p.AccountID)
		if err != nil {
			return err
		}
		for _, tm := range tms {
			team, err := a.d.Store.GetTeam(ctx, tm.TeamID)
			if errors.Is(err, domain.ErrNotFound) {
				continue
			}
			if err != nil {
				return err
			}
			if team.TenantID != t.ID {
				continue
			}
			out.TeamRoles = append(out.TeamRoles, teamRoleView{Team: team.Slug, Role: tm.Role})
			subjects[[2]string{domain.SubjectTeam, team.ID}] = true
		}
		sort.Slice(out.TeamRoles, func(i, j int) bool { return out.TeamRoles[i].Team < out.TeamRoles[j].Team })
	}

	if tree != nil {
		gs, err := a.d.Store.ListGrants(ctx, tree.ID)
		if err != nil {
			return err
		}
		for _, g := range gs {
			if subjects[[2]string{g.SubjectType, g.SubjectID}] {
				out.Grants = append(out.Grants, g)
			}
		}
	} else {
		trees, err := a.d.Store.ListTrees(ctx, t.ID)
		if err != nil {
			return err
		}
		inTenant := map[string]bool{}
		for _, tr := range trees {
			inTenant[tr.ID] = true
		}
		keys := make([][2]string, 0, len(subjects))
		for k := range subjects {
			keys = append(keys, k)
		}
		sort.Slice(keys, func(i, j int) bool { return keys[i][0]+keys[i][1] < keys[j][0]+keys[j][1] })
		for _, k := range keys {
			gs, err := a.d.Store.ListSubjectGrants(ctx, k[0], k[1])
			if err != nil {
				return err
			}
			for _, g := range gs {
				if inTenant[g.TreeID] {
					out.Grants = append(out.Grants, g)
				}
			}
		}
	}

	perms, err := ev.EffectivePermissions(ctx, pr.p, t.ID, tree)
	if err != nil {
		return err
	}
	perms = slices.Clone(perms)
	slices.Sort(perms)
	out.Permissions = nonNil(slices.Compact(perms))
	writeJSON(w, http.StatusOK, out)
	return nil
}
