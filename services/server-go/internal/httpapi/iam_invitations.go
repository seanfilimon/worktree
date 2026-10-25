package httpapi

import (
	"errors"
	"net/http"
	"net/mail"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

const (
	defaultInvitationTTL   = 7 * 24 * time.Hour
	maxInvitationTTLHours  = 30 * 24
	defaultInvitationRole  = domain.RoleDeveloper
	maxInvitationCodeBytes = 256
)

type createInvitationRequest struct {
	Email    string `json:"email"`
	Username string `json:"username"`
	Role     string `json:"role"`
	Team     string `json:"team"`      // team slug (optional)
	TTLHours int    `json:"ttl_hours"` // optional, default 7 days
}

type createInvitationResponse struct {
	Invitation *domain.Invitation `json:"invitation"`
	Code       string             `json:"code"`
}

type invitationPreview struct {
	Invitation *domain.Invitation `json:"invitation"`
	Tenant     *domain.Tenant     `json:"tenant"`
}

// handleListInvitations answers GET /api/v1/tenants/{tenant}/invitations.
func (a *api) handleListInvitations(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	invs, err := a.d.Store.ListInvitations(r.Context(), t.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(invs))
	return nil
}

// handleCreateInvitation answers POST /api/v1/tenants/{tenant}/invitations.
// The one-time code is returned once.
func (a *api) handleCreateInvitation(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	var req createInvitationRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Email = strings.TrimSpace(req.Email)
	req.Username = strings.TrimSpace(req.Username)
	req.Role = strings.TrimSpace(req.Role)
	req.Team = strings.TrimSpace(req.Team)
	if req.Email != "" && req.Username != "" {
		return badRequestf("give either email or username, not both")
	}
	if req.Email != "" {
		if err := validateText("email", req.Email, 254, true); err != nil {
			return err
		}
		if addr, err := mail.ParseAddress(req.Email); err != nil || addr.Address != req.Email {
			return badRequestf("email must be a valid address")
		}
	}
	if req.Role == "" {
		req.Role = defaultInvitationRole
	}
	if req.TTLHours < 0 || req.TTLHours > maxInvitationTTLHours {
		return badRequestf("ttl_hours must be between 1 and %d (0 = 7 days)", maxInvitationTTLHours)
	}
	ttl := defaultInvitationTTL
	if req.TTLHours > 0 {
		ttl = time.Duration(req.TTLHours) * time.Hour
	}
	ctx := r.Context()
	if err := a.checkTenantRole(ctx, t.ID, req.Role); err != nil {
		return err
	}
	if err := a.checkOwnerAssign(r, t.ID, req.Role); err != nil {
		return err
	}
	inv := &domain.Invitation{TenantID: t.ID, Email: req.Email, Role: req.Role, InvitedBy: principal(r).Username}
	if req.Username != "" {
		acct, err := a.accountByUsername(ctx, req.Username)
		if err != nil {
			return err
		}
		if _, err := a.d.Store.GetMembership(ctx, t.ID, acct.ID); err == nil {
			return conflictf("%s is already a member of %s", acct.Username, t.Slug)
		} else if !errors.Is(err, domain.ErrNotFound) {
			return err
		}
		inv.Username = acct.Username
	}
	if req.Team != "" {
		team, err := a.d.Store.GetTeamBySlug(ctx, t.ID, req.Team)
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("team %s/%s not found", t.Slug, req.Team)
		}
		if err != nil {
			return err
		}
		inv.TeamID = team.ID
	}
	code, err := sec.CreateInvitation(ctx, inv, ttl)
	if err != nil {
		return err
	}
	details := map[string]string{"role": inv.Role}
	for k, v := range map[string]string{"email": inv.Email, "username": inv.Username, "team": req.Team} {
		if v != "" {
			details[k] = v
		}
	}
	a.audit(r, "invitation.create", t.ID, "invitation", inv.ID, details)
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusCreated, createInvitationResponse{Invitation: inv, Code: code})
	return nil
}

// handleRevokeInvitation answers DELETE /api/v1/tenants/{tenant}/invitations/{id}.
func (a *api) handleRevokeInvitation(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	ctx := r.Context()
	id := r.PathValue("id")
	inv, err := a.d.Store.GetInvitation(ctx, t.ID, id)
	if errors.Is(err, domain.ErrNotFound) {
		return notFoundf("invitation %q not found", id)
	}
	if err != nil {
		return err
	}
	if inv.AcceptedAt != nil {
		return conflictf("invitation was already accepted")
	}
	if !inv.Revoked {
		inv.Revoked = true
		if err := a.d.Store.UpdateInvitation(ctx, inv); err != nil {
			return err
		}
		a.audit(r, "invitation.revoke", t.ID, "invitation", inv.ID, nil)
	}
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// invitationCode reads and bounds the {code} path value.
func invitationCode(r *http.Request) (string, error) {
	code := strings.TrimSpace(r.PathValue("code"))
	if code == "" || len(code) > maxInvitationCodeBytes {
		return "", notFoundf("invitation not found")
	}
	return code, nil
}

// handlePreviewInvitation answers GET /api/v1/invitations/{code} (public).
func (a *api) handlePreviewInvitation(w http.ResponseWriter, r *http.Request) error {
	sec, err := a.security()
	if err != nil {
		return err
	}
	code, err := invitationCode(r)
	if err != nil {
		return err
	}
	inv, t, err := sec.PreviewInvitation(r.Context(), code)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) || errors.Is(err, domain.ErrInvalid) {
			return notFoundf("invitation not found or no longer valid")
		}
		return err
	}
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, invitationPreview{Invitation: inv, Tenant: t})
	return nil
}

// handleAcceptInvitation answers POST /api/v1/invitations/{code}/accept
// for the signed-in account.
func (a *api) handleAcceptInvitation(w http.ResponseWriter, r *http.Request) error {
	p, err := requireFullUser(r)
	if err != nil {
		return err
	}
	sec, err := a.security()
	if err != nil {
		return err
	}
	code, err := invitationCode(r)
	if err != nil {
		return err
	}
	inv, err := sec.AcceptInvitation(r.Context(), code, p)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("invitation not found or no longer valid")
		}
		return err
	}
	a.audit(r, "invitation.accept", inv.TenantID, "invitation", inv.ID, map[string]string{"role": inv.Role})
	writeJSON(w, http.StatusOK, inv)
	return nil
}
