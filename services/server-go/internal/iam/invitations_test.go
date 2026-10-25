package iam

import (
	"context"
	"errors"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type inviteEnv struct {
	*testEnv
	acme  *domain.Tenant
	owner *domain.Account
	admin *domain.Account // tenant admin
	team  *domain.Team
}

func newInviteEnv(t *testing.T) *inviteEnv {
	t.Helper()
	ctx := context.Background()
	e := newTestEnv(t, nil)
	ie := &inviteEnv{testEnv: e, acme: e.newOrg(t, "acme")}
	ie.owner = e.register(t, "olivia")
	ie.admin = e.register(t, "adam")
	_ = e.store.PutMembership(ctx, &domain.Membership{TenantID: ie.acme.ID, AccountID: ie.owner.ID, Role: domain.RoleOwner})
	_ = e.store.PutMembership(ctx, &domain.Membership{TenantID: ie.acme.ID, AccountID: ie.admin.ID, Role: domain.RoleAdmin})
	ie.team = &domain.Team{TenantID: ie.acme.ID, Slug: "core", Name: "Core"}
	if err := e.store.CreateTeam(ctx, ie.team); err != nil {
		t.Fatal(err)
	}
	return ie
}

func (ie *inviteEnv) invite(t *testing.T, inv domain.Invitation) (string, *domain.Invitation) {
	t.Helper()
	if inv.TenantID == "" {
		inv.TenantID = ie.acme.ID
	}
	if inv.InvitedBy == "" {
		inv.InvitedBy = ie.admin.Username
	}
	code, err := ie.svc.CreateInvitation(context.Background(), &inv, 0)
	if err != nil {
		t.Fatalf("CreateInvitation(%+v): %v", inv, err)
	}
	return code, &inv
}

func TestCreateInvitation(t *testing.T) {
	ctx := context.Background()
	ie := newInviteEnv(t)
	code, inv := ie.invite(t, domain.Invitation{Email: " bob@example.com ", TeamID: "core"})
	if !wellFormedSecret(InvitationCodePrefix, code) {
		t.Fatalf("code %q", code)
	}
	if inv.ID == "" || inv.CodeHash != HashToken(code) || inv.Email != "bob@example.com" || inv.Role != domain.RoleViewer ||
		inv.TeamID != ie.team.ID || !inv.ExpiresAt.Equal(ie.clock.Now().Add(DefaultInvitationTTL)) || inv.Revoked || inv.AcceptedAt != nil {
		t.Fatalf("invitation %+v", inv)
	}
	stored, err := ie.store.GetInvitation(ctx, ie.acme.ID, inv.ID)
	if err != nil || stored.CodeHash != HashToken(code) || strings.Contains(stored.CodeHash, code) {
		t.Fatalf("stored %+v, %v", stored, err)
	}

	// Custom roles of the tenant are accepted.
	_ = ie.store.PutRole(ctx, &domain.CustomRole{TenantID: ie.acme.ID, Name: "release"})
	if _, inv := ie.invite(t, domain.Invitation{Role: "Release"}); inv.Role != "release" {
		t.Fatalf("custom role = %q", inv.Role)
	}
	// Owners and server admins may invite owners.
	if _, inv := ie.invite(t, domain.Invitation{Role: "owner", InvitedBy: "olivia"}); inv.Role != domain.RoleOwner {
		t.Fatal("owner invite by owner")
	}
	srv := ie.register(t, "root")
	srv.IsAdmin = true
	_ = ie.store.UpdateAccount(ctx, srv)
	ie.invite(t, domain.Invitation{Role: "owner", InvitedBy: "root"})

	other := ie.newOrg(t, "other")
	otherTeam := &domain.Team{TenantID: other.ID, Slug: "ops"}
	_ = ie.store.CreateTeam(ctx, otherTeam)
	_ = ie.store.PutRole(ctx, &domain.CustomRole{TenantID: other.ID, Name: "foreign"})
	ie.register(t, "bob")

	tests := []struct {
		name string
		inv  domain.Invitation
		ttl  time.Duration
		want error
	}{
		{"no tenant", domain.Invitation{Role: "viewer"}, 0, domain.ErrInvalid},
		{"unknown tenant", domain.Invitation{TenantID: "nope"}, 0, domain.ErrNotFound},
		{"bad email", domain.Invitation{TenantID: ie.acme.ID, Email: "not-an-email"}, 0, domain.ErrInvalid},
		{"bad username", domain.Invitation{TenantID: ie.acme.ID, Username: "x"}, 0, domain.ErrInvalid},
		{"unknown username", domain.Invitation{TenantID: ie.acme.ID, Username: "ghost"}, 0, domain.ErrInvalid},
		{"unknown role", domain.Invitation{TenantID: ie.acme.ID, Role: "wizard"}, 0, domain.ErrInvalid},
		{"other tenant's custom role", domain.Invitation{TenantID: ie.acme.ID, Role: "foreign"}, 0, domain.ErrInvalid},
		{"unknown team", domain.Invitation{TenantID: ie.acme.ID, TeamID: "nope"}, 0, domain.ErrInvalid},
		{"other tenant's team id", domain.Invitation{TenantID: ie.acme.ID, TeamID: otherTeam.ID}, 0, domain.ErrInvalid},
		{"other tenant's team slug", domain.Invitation{TenantID: ie.acme.ID, TeamID: "ops"}, 0, domain.ErrInvalid},
		{"owner by admin", domain.Invitation{TenantID: ie.acme.ID, Role: "owner", InvitedBy: "adam"}, 0, domain.ErrForbidden},
		{"owner by nobody", domain.Invitation{TenantID: ie.acme.ID, Role: "owner"}, 0, domain.ErrForbidden},
		{"negative ttl", domain.Invitation{TenantID: ie.acme.ID}, -time.Hour, domain.ErrInvalid},
		{"ttl too long", domain.Invitation{TenantID: ie.acme.ID}, MaxInvitationTTL + time.Hour, domain.ErrInvalid},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			inv := tt.inv
			if _, err := ie.svc.CreateInvitation(ctx, &inv, tt.ttl); !errors.Is(err, tt.want) {
				t.Fatalf("CreateInvitation = %v, want %v", err, tt.want)
			}
		})
	}
	if _, err := ie.svc.CreateInvitation(ctx, nil, 0); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("nil invitation = %v", err)
	}
	// Custom ttl and a username invitation.
	inv2 := domain.Invitation{TenantID: ie.acme.ID, Username: "BOB", Role: "developer"}
	if _, err := ie.svc.CreateInvitation(ctx, &inv2, time.Hour); err != nil || inv2.Username != "bob" ||
		!inv2.ExpiresAt.Equal(ie.clock.Now().Add(time.Hour)) {
		t.Fatalf("username invite = %+v, %v", inv2, err)
	}
}

func TestPreviewInvitation(t *testing.T) {
	ctx := context.Background()
	ie := newInviteEnv(t)
	code, inv := ie.invite(t, domain.Invitation{Email: "bob@example.com"})
	got, ten, err := ie.svc.PreviewInvitation(ctx, code)
	if err != nil || got.ID != inv.ID || ten.ID != ie.acme.ID {
		t.Fatalf("Preview = %+v, %+v, %v", got, ten, err)
	}
	unknown, _, _ := newSecret(InvitationCodePrefix)
	for _, c := range []string{unknown, "", "wti_short", TokenPrefix + code[len(InvitationCodePrefix):]} {
		if _, _, err := ie.svc.PreviewInvitation(ctx, c); !errors.Is(err, domain.ErrNotFound) {
			t.Fatalf("Preview(%q) = %v, want ErrNotFound", c, err)
		}
	}
}

func TestAcceptInvitation(t *testing.T) {
	ctx := context.Background()
	ie := newInviteEnv(t)
	bob := ie.register(t, "bob")
	code, inv := ie.invite(t, domain.Invitation{Email: "Bob@Example.com", Role: "developer", TeamID: ie.team.ID})

	got, err := ie.svc.AcceptInvitation(ctx, code, principalOf(bob))
	if err != nil {
		t.Fatal(err)
	}
	if got.ID != inv.ID || got.AcceptedAt == nil || got.AcceptedBy != "bob" {
		t.Fatalf("accepted %+v", got)
	}
	if m, err := ie.store.GetMembership(ctx, ie.acme.ID, bob.ID); err != nil || m.Role != domain.RoleDeveloper {
		t.Fatalf("membership %+v, %v", m, err)
	}
	if ms, _ := ie.store.ListTeamMembers(ctx, ie.team.ID); len(ms) != 1 || ms[0].AccountID != bob.ID || ms[0].Role != domain.TeamRoleMember {
		t.Fatalf("team members %+v", ms)
	}
	// Single use.
	if _, err := ie.svc.AcceptInvitation(ctx, code, principalOf(bob)); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("second accept = %v", err)
	}
}

func TestAcceptInvitationFailures(t *testing.T) {
	ctx := context.Background()
	ie := newInviteEnv(t)
	bob := ie.register(t, "bob")
	carol := ie.register(t, "carol")
	dora := ie.register(t, "dora")
	dora.Disabled = true
	_ = ie.store.UpdateAccount(ctx, dora)

	expCode, _ := ie.invite(t, domain.Invitation{})
	ie.clock.Advance(DefaultInvitationTTL)
	emailCode, _ := ie.invite(t, domain.Invitation{Email: "bob@example.com"})
	userCode, _ := ie.invite(t, domain.Invitation{Username: "bob"})
	revCode, revInv := ie.invite(t, domain.Invitation{})
	revInv.Revoked = true
	_ = ie.store.UpdateInvitation(ctx, revInv)
	liveCode, _ := ie.invite(t, domain.Invitation{})
	unknown, _, _ := newSecret(InvitationCodePrefix)
	acme := ie.acme
	sa := ie.newSA(t, acme, "ci", domain.RoleViewer)

	tests := []struct {
		name string
		code string
		p    *domain.Principal
		want error
	}{
		{"anonymous", liveCode, nil, domain.ErrUnauthenticated},
		{"service account", liveCode, &domain.Principal{Kind: domain.PrincipalServiceAccount, ServiceAccountID: sa.ID, TenantID: acme.ID}, domain.ErrForbidden},
		{"deleted account", liveCode, &domain.Principal{AccountID: "ghost"}, domain.ErrUnauthenticated},
		{"disabled account", liveCode, principalOf(dora), domain.ErrForbidden},
		{"unknown code", unknown, principalOf(bob), domain.ErrNotFound},
		{"malformed code", "wti_nope", principalOf(bob), domain.ErrNotFound},
		{"expired", expCode, principalOf(bob), domain.ErrForbidden},
		{"revoked", revCode, principalOf(bob), domain.ErrForbidden},
		{"email mismatch", emailCode, principalOf(carol), domain.ErrForbidden},
		{"username mismatch", userCode, principalOf(carol), domain.ErrForbidden},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			if _, err := ie.svc.AcceptInvitation(ctx, tt.code, tt.p); !errors.Is(err, tt.want) {
				t.Fatalf("AcceptInvitation = %v, want %v", err, tt.want)
			}
		})
	}
	// Nothing was granted to carol by the failed attempts.
	if _, err := ie.store.GetMembership(ctx, acme.ID, carol.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("carol membership after failures: %v", err)
	}
	// The matching accounts can still accept them.
	if _, err := ie.svc.AcceptInvitation(ctx, liveCode, principalOf(carol)); err != nil {
		t.Fatalf("open invitation: %v", err)
	}
	if _, err := ie.svc.AcceptInvitation(ctx, emailCode, principalOf(bob)); err != nil {
		t.Fatalf("email invitation: %v", err)
	}
	if _, err := ie.svc.AcceptInvitation(ctx, userCode, principalOf(bob)); err != nil {
		t.Fatalf("username invitation: %v", err)
	}
}

func TestAcceptInvitationRoleMerge(t *testing.T) {
	ctx := context.Background()
	ie := newInviteEnv(t)
	bob := ie.register(t, "bob")
	_ = ie.store.PutRole(ctx, &domain.CustomRole{TenantID: ie.acme.ID, Name: "release"})

	accept := func(role string) string {
		t.Helper()
		code, _ := ie.invite(t, domain.Invitation{Role: role})
		if _, err := ie.svc.AcceptInvitation(ctx, code, principalOf(bob)); err != nil {
			t.Fatal(err)
		}
		m, _ := ie.store.GetMembership(ctx, ie.acme.ID, bob.ID)
		return m.Role
	}
	if r := accept("developer"); r != domain.RoleDeveloper {
		t.Fatalf("new member role %q", r)
	}
	if r := accept("viewer"); r != domain.RoleDeveloper {
		t.Fatalf("downgraded to %q", r)
	}
	if r := accept("maintainer"); r != domain.RoleMaintainer {
		t.Fatalf("not upgraded: %q", r)
	}
	if r := accept("release"); r != domain.RoleMaintainer {
		t.Fatalf("built-in replaced by custom role: %q", r)
	}

	// The admin is never downgraded; an existing team membership is kept.
	_ = ie.store.PutTeamMember(ctx, &domain.TeamMember{TeamID: ie.team.ID, AccountID: ie.admin.ID, Role: domain.TeamRoleMaintainer})
	code, _ := ie.invite(t, domain.Invitation{Role: "viewer", TeamID: "core"})
	if _, err := ie.svc.AcceptInvitation(ctx, code, principalOf(ie.admin)); err != nil {
		t.Fatal(err)
	}
	if m, _ := ie.store.GetMembership(ctx, ie.acme.ID, ie.admin.ID); m.Role != domain.RoleAdmin {
		t.Fatalf("admin downgraded to %q", m.Role)
	}
	if ms, _ := ie.store.ListTeamMembers(ctx, ie.team.ID); len(ms) != 1 || ms[0].Role != domain.TeamRoleMaintainer {
		t.Fatalf("team membership changed: %+v", ms)
	}

	// A team deleted after the invitation was issued is skipped.
	carol := ie.register(t, "carol")
	code, _ = ie.invite(t, domain.Invitation{TeamID: "core"})
	_ = ie.store.DeleteTeam(ctx, ie.team.ID)
	if _, err := ie.svc.AcceptInvitation(ctx, code, principalOf(carol)); err != nil {
		t.Fatalf("accept with deleted team: %v", err)
	}
	if m, err := ie.store.GetMembership(ctx, ie.acme.ID, carol.ID); err != nil || m.Role != domain.RoleViewer {
		t.Fatalf("carol membership %+v, %v", m, err)
	}
}

func TestAcceptInvitationConcurrent(t *testing.T) {
	ctx := context.Background()
	ie := newInviteEnv(t)
	code, _ := ie.invite(t, domain.Invitation{})
	const n = 8
	accts := make([]*domain.Account, n)
	for i := range accts {
		accts[i] = ie.register(t, "user"+string(rune('a'+i)))
	}
	var wg sync.WaitGroup
	var mu sync.Mutex
	ok := 0
	for _, a := range accts {
		wg.Add(1)
		go func(a *domain.Account) {
			defer wg.Done()
			if _, err := ie.svc.AcceptInvitation(ctx, code, principalOf(a)); err == nil {
				mu.Lock()
				ok++
				mu.Unlock()
			}
		}(a)
	}
	wg.Wait()
	if ok != 1 {
		t.Fatalf("invitation accepted %d times", ok)
	}
}
