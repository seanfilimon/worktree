package httpapi

import (
	"context"
	"errors"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type putGrantRequest struct {
	SubjectType string `json:"subject_type"`
	Subject     string `json:"subject"`
	Role        string `json:"role"`
}

// normalizeSubjectType accepts the grant subject types (and "sa" as an
// alias of service_account).
func normalizeSubjectType(s string) (string, error) {
	switch strings.TrimSpace(s) {
	case domain.SubjectUser:
		return domain.SubjectUser, nil
	case domain.SubjectTeam:
		return domain.SubjectTeam, nil
	case domain.SubjectServiceAccount, "sa":
		return domain.SubjectServiceAccount, nil
	}
	return "", badRequestf("subject_type must be %q, %q or %q", domain.SubjectUser, domain.SubjectTeam, domain.SubjectServiceAccount)
}

// resolveSubject maps a grant subject name (username, team slug or
// service-account name within tenantID) to its id and display name.
func (a *api) resolveSubject(ctx context.Context, tenantID, subjectType, subject string) (id, display string, err error) {
	switch subjectType {
	case domain.SubjectUser:
		acct, err := a.accountByUsername(ctx, subject)
		if err != nil {
			return "", "", err
		}
		return acct.ID, acct.Username, nil
	case domain.SubjectTeam:
		team, err := a.d.Store.GetTeamBySlug(ctx, tenantID, subject)
		if errors.Is(err, domain.ErrNotFound) {
			return "", "", notFoundf("team %q not found", subject)
		}
		if err != nil {
			return "", "", err
		}
		return team.ID, team.Slug, nil
	default:
		sa, err := a.d.Store.GetServiceAccountByName(ctx, tenantID, subject)
		if errors.Is(err, domain.ErrNotFound) {
			return "", "", notFoundf("service account %q not found", subject)
		}
		if err != nil {
			return "", "", err
		}
		return sa.ID, sa.Name, nil
	}
}

// handleListGrants answers GET /api/v1/trees/{tenant}/{tree}/grants.
func (a *api) handleListGrants(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	gs, err := a.d.Store.ListGrants(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	gs = nonNil(gs)
	sort.SliceStable(gs, func(i, j int) bool {
		if gs[i].SubjectType != gs[j].SubjectType {
			return gs[i].SubjectType < gs[j].SubjectType
		}
		return gs[i].Subject < gs[j].Subject
	})
	writeJSON(w, http.StatusOK, gs)
	return nil
}

// handlePutGrant answers PUT /api/v1/trees/{tenant}/{tree}/grants.
func (a *api) handlePutGrant(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req putGrantRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	st, err := normalizeSubjectType(req.SubjectType)
	if err != nil {
		return err
	}
	req.Subject = strings.TrimSpace(req.Subject)
	req.Role = strings.TrimSpace(req.Role)
	if req.Subject == "" {
		return badRequestf("subject is required")
	}
	ctx := r.Context()
	if err := a.checkTenantRole(ctx, t.ID, req.Role); err != nil {
		return err
	}
	if err := a.checkOwnerAssign(r, t.ID, req.Role); err != nil {
		return err
	}
	id, display, err := a.resolveSubject(ctx, t.ID, st, req.Subject)
	if err != nil {
		return err
	}
	existing, err := a.d.Store.ListGrants(ctx, tree.ID)
	if err != nil {
		return err
	}
	g := &domain.Grant{TreeID: tree.ID, SubjectType: st, SubjectID: id, Subject: display, Role: req.Role,
		CreatedBy: principal(r).Username, CreatedAt: time.Now().UTC()}
	prev := ""
	for _, e := range existing {
		if e.SubjectType == st && e.SubjectID == id {
			prev = e.Role
			if !e.CreatedAt.IsZero() {
				g.CreatedAt = e.CreatedAt
			}
		}
	}
	if err := a.d.Store.PutGrant(ctx, g); err != nil {
		return err
	}
	a.audit(r, "grant.set", t.ID, "tree", tree.ID, map[string]string{
		"tree": tree.Name, "subject_type": st, "subject": display, "role": g.Role, "previous_role": prev})
	writeJSON(w, http.StatusOK, g)
	return nil
}

// handleDeleteGrant answers DELETE
// /api/v1/trees/{tenant}/{tree}/grants?subject_type=&subject=. The subject
// is matched by display name or id, so grants of deleted subjects can be
// removed too.
func (a *api) handleDeleteGrant(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	st, err := normalizeSubjectType(q.Get("subject_type"))
	if err != nil {
		return err
	}
	subject := strings.TrimSpace(q.Get("subject"))
	if subject == "" {
		return badRequestf("subject is required")
	}
	ctx := r.Context()
	gs, err := a.d.Store.ListGrants(ctx, tree.ID)
	if err != nil {
		return err
	}
	var found *domain.Grant
	for i := range gs {
		g := &gs[i]
		if g.SubjectType == st && (g.Subject == subject || g.SubjectID == subject) {
			found = g
			break
		}
	}
	if found == nil {
		return notFoundf("no %s grant for %q on %s/%s", st, subject, t.Slug, tree.Name)
	}
	if err := a.d.Store.DeleteGrant(ctx, tree.ID, found.SubjectType, found.SubjectID); err != nil {
		return err
	}
	a.audit(r, "grant.remove", t.ID, "tree", tree.ID, map[string]string{
		"tree": tree.Name, "subject_type": st, "subject": found.Subject, "previous_role": found.Role})
	w.WriteHeader(http.StatusNoContent)
	return nil
}
