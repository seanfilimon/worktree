package httpapi

import (
	"errors"
	"net/http"
	"slices"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type createTeamRequest struct {
	Slug        string `json:"slug"`
	Name        string `json:"name"`
	Description string `json:"description"`
}

type patchTeamRequest struct {
	Name        *string `json:"name"`
	Description *string `json:"description"`
}

type putTeamMemberRequest struct {
	Role string `json:"role"`
}

type teamMemberView struct {
	Account domain.Account `json:"account"`
	Role    string         `json:"role"`
}

type teamDetail struct {
	Team    *domain.Team     `json:"team"`
	Members []teamMemberView `json:"members"`
	Grants  []domain.Grant   `json:"grants"`
}

func validateTeamText(name, description string) error {
	if err := validateText("name", name, 200, true); err != nil {
		return err
	}
	return validateText("description", description, 2000, false)
}

// teamByPath resolves the {team} path value inside tenant t.
func (a *api) teamByPath(r *http.Request, t *domain.Tenant) (*domain.Team, error) {
	slug := r.PathValue("team")
	team, err := a.d.Store.GetTeamBySlug(r.Context(), t.ID, slug)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("team %s/%s not found", t.Slug, slug)
	}
	return team, err
}

// teamRoleOf returns the caller's role in team ("" when not a member).
func (a *api) teamRoleOf(r *http.Request, teamID string) (string, error) {
	p := principal(r)
	if p == nil || p.AccountID == "" {
		return "", nil
	}
	ms, err := a.d.Store.ListTeamMembers(r.Context(), teamID)
	if err != nil {
		return "", err
	}
	for _, m := range ms {
		if m.AccountID == p.AccountID {
			return m.Role, nil
		}
	}
	return "", nil
}

// teamAccess resolves {tenant} and {team} and authorizes act on the
// tenant. When the caller lacks it, members of the team still pass if
// their team role is one of teamRoles ("" = any team role). Callers that
// pass neither check get the authorization error, even for unknown teams.
func (a *api) teamAccess(r *http.Request, act domain.Action, teamRoles ...string) (*domain.Tenant, *domain.Team, error) {
	if _, err := requireLogin(r); err != nil {
		return nil, nil, err
	}
	ctx := r.Context()
	t, err := a.tenantByRef(ctx, r.PathValue("tenant"))
	if err != nil {
		return nil, nil, err
	}
	authErr := a.d.Authz.Authorize(ctx, principal(r), act, t.ID, nil)
	if authErr != nil && (len(teamRoles) == 0 || !errors.Is(authErr, domain.ErrForbidden)) {
		return nil, nil, authErr
	}
	team, err := a.teamByPath(r, t)
	if err != nil {
		if authErr != nil {
			return nil, nil, authErr // do not reveal which teams exist
		}
		return nil, nil, err
	}
	if authErr != nil {
		role, err := a.teamRoleOf(r, team.ID)
		if err != nil {
			return nil, nil, err
		}
		allowed := false
		for _, tr := range teamRoles {
			if role != "" && (tr == "" || tr == role) {
				allowed = true
			}
		}
		if !allowed {
			return nil, nil, authErr
		}
	}
	return t, team, nil
}

// handleListTeams answers GET /api/v1/tenants/{tenant}/teams.
func (a *api) handleListTeams(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	teams, err := a.d.Store.ListTeams(r.Context(), t.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(teams))
	return nil
}

// handleCreateTeam answers POST /api/v1/tenants/{tenant}/teams.
func (a *api) handleCreateTeam(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req createTeamRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Slug = strings.TrimSpace(req.Slug)
	req.Name = strings.TrimSpace(req.Name)
	req.Description = strings.TrimSpace(req.Description)
	if err := validateIAMName("slug", req.Slug); err != nil {
		return err
	}
	if req.Name == "" {
		req.Name = req.Slug
	}
	if err := validateTeamText(req.Name, req.Description); err != nil {
		return err
	}
	now := time.Now().UTC()
	team := &domain.Team{TenantID: t.ID, Slug: req.Slug, Name: req.Name, Description: req.Description, CreatedAt: now, UpdatedAt: now}
	if err := a.d.Store.CreateTeam(r.Context(), team); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("team %s/%s already exists", t.Slug, req.Slug)
		}
		return err
	}
	a.audit(r, "team.create", t.ID, "team", team.ID, map[string]string{"slug": team.Slug})
	writeJSON(w, http.StatusCreated, team)
	return nil
}

// handleGetTeam answers GET /api/v1/tenants/{tenant}/teams/{team}. Callers
// need iam:read, or must be members of the team.
func (a *api) handleGetTeam(w http.ResponseWriter, r *http.Request) error {
	_, team, err := a.teamAccess(r, domain.ActIAMRead, "")
	if err != nil {
		return err
	}
	ctx := r.Context()
	ms, err := a.d.Store.ListTeamMembers(ctx, team.ID)
	if err != nil {
		return err
	}
	members := make([]teamMemberView, 0, len(ms))
	for _, m := range ms {
		acct, err := a.d.Store.GetAccount(ctx, m.AccountID)
		if errors.Is(err, domain.ErrNotFound) {
			continue // dangling member
		}
		if err != nil {
			return err
		}
		members = append(members, teamMemberView{Account: memberAccount(*acct), Role: m.Role}) // abac: no attributes
	}
	sort.Slice(members, func(i, j int) bool { return members[i].Account.Username < members[j].Account.Username })
	grants, err := a.d.Store.ListSubjectGrants(ctx, domain.SubjectTeam, team.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, teamDetail{Team: team, Members: members, Grants: nonNil(grants)})
	return nil
}

// handlePatchTeam answers PATCH /api/v1/tenants/{tenant}/teams/{team}.
func (a *api) handlePatchTeam(w http.ResponseWriter, r *http.Request) error {
	t, team, err := a.teamAccess(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req patchTeamRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	trimPtr(req.Name)
	trimPtr(req.Description)
	details := map[string]string{"slug": team.Slug}
	if req.Name != nil {
		team.Name = *req.Name
		details["name"] = team.Name
	}
	if req.Description != nil {
		team.Description = *req.Description
		details["description"] = "updated"
	}
	if err := validateTeamText(team.Name, team.Description); err != nil {
		return err
	}
	team.UpdatedAt = time.Now().UTC()
	if err := a.d.Store.UpdateTeam(r.Context(), team); err != nil {
		return err
	}
	a.audit(r, "team.update", t.ID, "team", team.ID, details)
	writeJSON(w, http.StatusOK, team)
	return nil
}

// handleDeleteTeam answers DELETE /api/v1/tenants/{tenant}/teams/{team}.
func (a *api) handleDeleteTeam(w http.ResponseWriter, r *http.Request) error {
	t, team, err := a.teamAccess(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	if err := a.d.Store.DeleteTeam(r.Context(), team.ID); err != nil {
		return err
	}
	a.audit(r, "team.delete", t.ID, "team", team.ID, map[string]string{"slug": team.Slug})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handlePutTeamMember answers PUT
// /api/v1/tenants/{tenant}/teams/{team}/members/{username}. The account
// must be a member of the tenant.
func (a *api) handlePutTeamMember(w http.ResponseWriter, r *http.Request) error {
	t, team, err := a.teamAccess(r, domain.ActIAMAdmin, domain.TeamRoleMaintainer)
	if err != nil {
		return err
	}
	var req putTeamMemberRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Role = strings.TrimSpace(req.Role)
	if req.Role == "" {
		req.Role = domain.TeamRoleMember
	}
	if req.Role != domain.TeamRoleMember && req.Role != domain.TeamRoleMaintainer {
		return badRequestf("role must be %q or %q", domain.TeamRoleMember, domain.TeamRoleMaintainer)
	}
	ctx := r.Context()
	acct, err := a.accountByUsername(ctx, r.PathValue("username"))
	if err != nil {
		return err
	}
	if _, err := a.d.Store.GetMembership(ctx, t.ID, acct.ID); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return badRequestf("%s is not a member of tenant %s", acct.Username, t.Slug)
		}
		return err
	}
	ms, err := a.d.Store.ListTeamMembers(ctx, team.ID)
	if err != nil {
		return err
	}
	m := &domain.TeamMember{TeamID: team.ID, AccountID: acct.ID, Role: req.Role, CreatedAt: time.Now().UTC()}
	prev := ""
	for _, cur := range ms {
		if cur.AccountID == acct.ID {
			m.CreatedAt, prev = cur.CreatedAt, cur.Role
		}
	}
	if err := a.d.Store.PutTeamMember(ctx, m); err != nil {
		return err
	}
	a.audit(r, "team.member.set", t.ID, "team", team.ID,
		map[string]string{"team": team.Slug, "username": acct.Username, "role": m.Role, "previous_role": prev})
	writeJSON(w, http.StatusOK, teamMemberView{Account: memberAccount(*acct), Role: m.Role}) // abac: no attributes
	return nil
}

// handleDeleteTeamMember answers DELETE
// /api/v1/tenants/{tenant}/teams/{team}/members/{username}. Members may
// always leave a team.
func (a *api) handleDeleteTeamMember(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	username := r.PathValue("username")
	var (
		t    *domain.Tenant
		team *domain.Team
	)
	if p.Username == username && !p.IsServiceAccount() {
		if t, err = a.tenantByRef(ctx, r.PathValue("tenant")); err != nil {
			return err
		}
		if team, err = a.teamByPath(r, t); err != nil {
			return err
		}
	} else if t, team, err = a.teamAccess(r, domain.ActIAMAdmin, domain.TeamRoleMaintainer); err != nil {
		return err
	}
	acct, err := a.accountByUsername(ctx, username)
	if err != nil {
		return err
	}
	ms, err := a.d.Store.ListTeamMembers(ctx, team.ID)
	if err != nil {
		return err
	}
	notMember := notFoundf("%s is not a member of team %s", acct.Username, team.Slug)
	if !slices.ContainsFunc(ms, func(m domain.TeamMember) bool { return m.AccountID == acct.ID }) {
		return notMember
	}
	if err := a.d.Store.DeleteTeamMember(ctx, team.ID, acct.ID); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notMember
		}
		return err
	}
	a.audit(r, "team.member.remove", t.ID, "team", team.ID,
		map[string]string{"team": team.Slug, "username": acct.Username})
	w.WriteHeader(http.StatusNoContent)
	return nil
}
