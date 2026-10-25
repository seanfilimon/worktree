package meta

import (
	"context"
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// iamConformanceTests is the IAMStore part of the shared contract suite
// (run by runConformance against both stores).
var iamConformanceTests = []struct {
	name string
	fn   func(t *testing.T, s domain.MetaStore)
}{
	{"IAM/Teams", testTeams},
	{"IAM/TeamMembers", testTeamMembers},
	{"IAM/DeleteTeamCascades", testDeleteTeamCascades},
	{"IAM/Roles", testRoles},
	{"IAM/Policies", testPolicies},
	{"IAM/Grants", testGrants},
	{"IAM/DeleteTreeRemovesGrants", testDeleteTreeRemovesGrants},
	{"IAM/ServiceAccounts", testServiceAccounts},
	{"IAM/ServiceAccountKeys", testServiceAccountKeys},
	{"IAM/DeleteServiceAccountCascades", testDeleteServiceAccountCascades},
	{"IAM/TokenFields", testTokenIAMFields},
	{"IAM/MFA", testMFA},
	{"IAM/Invitations", testInvitations},
	{"IAM/PasswordResets", testPasswordResets},
	{"IAM/SSOConfig", testSSOConfig},
	{"IAM/SSOIdentities", testSSOIdentities},
	{"IAM/InvalidArguments", testIAMInvalidArguments},
	{"IAM/EmptyListsNonNil", testIAMEmptyListsNonNil},
	{"IAM/NoAliasing", testIAMNoAliasing},
	{"IAM/CanceledAndClosed", testIAMCanceledAndClosed},
}

func mkTree(t *testing.T, s domain.MetaStore, tenantID, name string) *domain.Tree {
	t.Helper()
	tr := &domain.Tree{TenantID: tenantID, Name: name}
	must(t, s.CreateTree(context.Background(), tr))
	return tr
}

func mkTeam(t *testing.T, s domain.MetaStore, tenantID, slug string) *domain.Team {
	t.Helper()
	tm := &domain.Team{TenantID: tenantID, Slug: slug, Name: slug}
	must(t, s.CreateTeam(context.Background(), tm))
	return tm
}

func mkSA(t *testing.T, s domain.MetaStore, tenantID, name string) *domain.ServiceAccount {
	t.Helper()
	sa := &domain.ServiceAccount{TenantID: tenantID, Name: name, Role: domain.RoleDeveloper}
	must(t, s.CreateServiceAccount(context.Background(), sa))
	return sa
}

func testTeams(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	core := &domain.Team{TenantID: "t1", Slug: "core", Name: "Core", Description: "core devs"}
	must(t, s.CreateTeam(ctx, core))
	if core.ID == "" || core.CreatedAt.IsZero() || !core.UpdatedAt.Equal(core.CreatedAt) {
		t.Fatalf("CreateTeam defaults: %+v", core)
	}
	ops := &domain.Team{ID: "team-ops", TenantID: "t1", Slug: "ops", CreatedAt: at(1), UpdatedAt: at(2)}
	must(t, s.CreateTeam(ctx, ops))
	if ops.ID != "team-ops" || !ops.CreatedAt.Equal(at(1)) || !ops.UpdatedAt.Equal(at(2)) {
		t.Fatalf("CreateTeam overwrote caller values: %+v", ops)
	}
	wantErr(t, s.CreateTeam(ctx, &domain.Team{TenantID: "t1", Slug: "core"}), domain.ErrAlreadyExists)
	wantErr(t, s.CreateTeam(ctx, &domain.Team{ID: "team-ops", TenantID: "t9", Slug: "x"}), domain.ErrAlreadyExists)
	// Slugs are unique per tenant only.
	must(t, s.CreateTeam(ctx, &domain.Team{TenantID: "t2", Slug: "core"}))
	must(t, s.CreateTeam(ctx, &domain.Team{TenantID: "t1", Slug: "alpha"}))

	got, err := s.GetTeam(ctx, core.ID)
	must(t, err)
	wantEqual(t, "GetTeam", *got, *core)
	got, err = s.GetTeamBySlug(ctx, "t1", "ops")
	must(t, err)
	wantEqual(t, "GetTeamBySlug", *got, *ops)
	_, err = s.GetTeam(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetTeamBySlug(ctx, "t3", "core")
	wantErr(t, err, domain.ErrNotFound)

	slugs := func(ts []domain.Team) []string {
		out := []string{}
		for _, x := range ts {
			out = append(out, x.Slug)
		}
		return out
	}
	list, err := s.ListTeams(ctx, "t1")
	must(t, err)
	wantEqual(t, "ListTeams(t1)", slugs(list), []string{"alpha", "core", "ops"})
	list, err = s.ListTeams(ctx, "t2")
	must(t, err)
	wantEqual(t, "ListTeams(t2)", slugs(list), []string{"core"})

	// Update: rename slug, UpdatedAt advances, CreatedAt preserved.
	upd := *core
	upd.Slug = "platform"
	upd.Name = "Platform"
	upd.CreatedAt = time.Time{}
	time.Sleep(2 * time.Millisecond)
	must(t, s.UpdateTeam(ctx, &upd))
	got, err = s.GetTeamBySlug(ctx, "t1", "platform")
	must(t, err)
	if got.Name != "Platform" || !got.CreatedAt.Equal(core.CreatedAt) || !got.UpdatedAt.After(core.UpdatedAt) {
		t.Fatalf("UpdateTeam result: %+v (orig %+v)", got, core)
	}
	wantEqual(t, "UpdateTeam writes back", *got, upd)
	_, err = s.GetTeamBySlug(ctx, "t1", "core")
	wantErr(t, err, domain.ErrNotFound)
	must(t, s.CreateTeam(ctx, &domain.Team{TenantID: "t1", Slug: "core"})) // old slug is free
	upd.Slug = "ops"
	wantErr(t, s.UpdateTeam(ctx, &upd), domain.ErrAlreadyExists)
	upd.Slug = "platform"
	upd.TenantID = "t2"
	wantErr(t, s.UpdateTeam(ctx, &upd), domain.ErrInvalid)
	wantErr(t, s.UpdateTeam(ctx, &domain.Team{ID: "missing", TenantID: "t1", Slug: "m"}), domain.ErrNotFound)
}

func testTeamMembers(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	a := mkTeam(t, s, "t1", "a")
	b := mkTeam(t, s, "t1", "b")
	wantErr(t, s.PutTeamMember(ctx, &domain.TeamMember{TeamID: "missing", AccountID: "u1"}), domain.ErrNotFound)
	wantErr(t, s.PutTeamMember(ctx, &domain.TeamMember{TeamID: a.ID, AccountID: "u1", Role: "owner"}), domain.ErrInvalid)

	m1 := &domain.TeamMember{TeamID: a.ID, AccountID: "u2"}
	must(t, s.PutTeamMember(ctx, m1))
	if m1.Role != domain.TeamRoleMember || m1.CreatedAt.IsZero() {
		t.Fatalf("PutTeamMember defaults: %+v", m1)
	}
	must(t, s.PutTeamMember(ctx, &domain.TeamMember{TeamID: a.ID, AccountID: "u1", Role: domain.TeamRoleMaintainer, CreatedAt: at(3)}))
	must(t, s.PutTeamMember(ctx, &domain.TeamMember{TeamID: b.ID, AccountID: "u2", Role: domain.TeamRoleMaintainer, CreatedAt: at(4)}))

	// Upsert keeps the original CreatedAt.
	must(t, s.PutTeamMember(ctx, &domain.TeamMember{TeamID: a.ID, AccountID: "u2", Role: domain.TeamRoleMaintainer}))
	members, err := s.ListTeamMembers(ctx, a.ID)
	must(t, err)
	if len(members) != 2 || members[0].AccountID != "u1" || members[1].AccountID != "u2" {
		t.Fatalf("ListTeamMembers order: %+v", members)
	}
	if members[1].Role != domain.TeamRoleMaintainer || !members[1].CreatedAt.Equal(m1.CreatedAt) {
		t.Fatalf("upsert: %+v (want maintainer, created %v)", members[1], m1.CreatedAt)
	}
	wantEqual(t, "member u1", members[0], domain.TeamMember{TeamID: a.ID, AccountID: "u1", Role: domain.TeamRoleMaintainer, CreatedAt: at(3)})

	teams, err := s.ListAccountTeams(ctx, "u2")
	must(t, err)
	wantIDs := []string{a.ID, b.ID}
	if wantIDs[0] > wantIDs[1] {
		wantIDs[0], wantIDs[1] = wantIDs[1], wantIDs[0]
	}
	if len(teams) != 2 || teams[0].TeamID != wantIDs[0] || teams[1].TeamID != wantIDs[1] {
		t.Fatalf("ListAccountTeams (by team id): %+v, want %v", teams, wantIDs)
	}

	must(t, s.DeleteTeamMember(ctx, a.ID, "u2"))
	wantErr(t, s.DeleteTeamMember(ctx, a.ID, "u2"), domain.ErrNotFound)
	wantErr(t, s.DeleteTeamMember(ctx, "missing", "u2"), domain.ErrNotFound)
	teams, err = s.ListAccountTeams(ctx, "u2")
	must(t, err)
	if len(teams) != 1 || teams[0].TeamID != b.ID {
		t.Fatalf("ListAccountTeams after delete: %+v", teams)
	}
	members, err = s.ListTeamMembers(ctx, a.ID)
	must(t, err)
	if len(members) != 1 || members[0].AccountID != "u1" {
		t.Fatalf("ListTeamMembers after delete: %+v", members)
	}
}

func testDeleteTeamCascades(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	tr1, tr2 := mkTree(t, s, "t1", "web"), mkTree(t, s, "t1", "api")
	doomed, keep := mkTeam(t, s, "t1", "doomed"), mkTeam(t, s, "t1", "keep")
	for _, tm := range []*domain.Team{doomed, keep} {
		must(t, s.PutTeamMember(ctx, &domain.TeamMember{TeamID: tm.ID, AccountID: "u1"}))
		must(t, s.PutTeamMember(ctx, &domain.TeamMember{TeamID: tm.ID, AccountID: "u2"}))
		for _, tr := range []*domain.Tree{tr1, tr2} {
			must(t, s.PutGrant(ctx, &domain.Grant{TreeID: tr.ID, SubjectType: domain.SubjectTeam, SubjectID: tm.ID, Subject: tm.Slug, Role: domain.RoleDeveloper}))
		}
	}
	// A user grant whose subject id equals the team id is a different subject.
	must(t, s.PutGrant(ctx, &domain.Grant{TreeID: tr1.ID, SubjectType: domain.SubjectUser, SubjectID: doomed.ID, Role: domain.RoleViewer}))

	must(t, s.DeleteTeam(ctx, doomed.ID))
	wantErr(t, s.DeleteTeam(ctx, doomed.ID), domain.ErrNotFound)
	_, err := s.GetTeam(ctx, doomed.ID)
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetTeamBySlug(ctx, "t1", "doomed")
	wantErr(t, err, domain.ErrNotFound)

	members, err := s.ListTeamMembers(ctx, doomed.ID)
	must(t, err)
	if len(members) != 0 {
		t.Fatalf("members of deleted team survived: %+v", members)
	}
	for _, acct := range []string{"u1", "u2"} {
		teams, err := s.ListAccountTeams(ctx, acct)
		must(t, err)
		if len(teams) != 1 || teams[0].TeamID != keep.ID {
			t.Fatalf("ListAccountTeams(%s) after team delete: %+v", acct, teams)
		}
	}
	gs, err := s.ListSubjectGrants(ctx, domain.SubjectTeam, doomed.ID)
	must(t, err)
	if len(gs) != 0 {
		t.Fatalf("grants of deleted team survived: %+v", gs)
	}
	gs, err = s.ListSubjectGrants(ctx, domain.SubjectTeam, keep.ID)
	must(t, err)
	if len(gs) != 2 {
		t.Fatalf("surviving team grants: %+v", gs)
	}
	gs, err = s.ListGrants(ctx, tr1.ID)
	must(t, err)
	if len(gs) != 2 || gs[0].SubjectID != keep.ID || gs[1].SubjectType != domain.SubjectUser {
		t.Fatalf("tree grants after team delete: %+v", gs)
	}
	members, err = s.ListTeamMembers(ctx, keep.ID)
	must(t, err)
	if len(members) != 2 {
		t.Fatalf("surviving team members: %+v", members)
	}
	// The slug is free again and the new team starts empty.
	again := mkTeam(t, s, "t1", "doomed")
	members, err = s.ListTeamMembers(ctx, again.ID)
	must(t, err)
	if len(members) != 0 {
		t.Fatalf("recreated team has members: %+v", members)
	}
}

func testRoles(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	r := &domain.CustomRole{TenantID: "t1", Name: "releaser", Description: "cuts releases",
		Permissions: []domain.Action{domain.ActTreeRead, domain.ActTreeWrite, "mr:*"}}
	must(t, s.PutRole(ctx, r))
	if r.CreatedAt.IsZero() || r.UpdatedAt.IsZero() {
		t.Fatalf("PutRole timestamps: %+v", r)
	}
	got, err := s.GetRole(ctx, "t1", "releaser")
	must(t, err)
	wantEqual(t, "GetRole", *got, *r)
	_, err = s.GetRole(ctx, "t1", "nope")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetRole(ctx, "t2", "releaser")
	wantErr(t, err, domain.ErrNotFound)

	for _, builtin := range []string{domain.RoleOwner, domain.RoleAdmin, domain.RoleMaintainer, domain.RoleDeveloper, domain.RoleViewer} {
		wantErr(t, s.PutRole(ctx, &domain.CustomRole{TenantID: "t1", Name: builtin}), domain.ErrInvalid)
	}

	// Replace: CreatedAt kept, UpdatedAt advances, nil permissions stored as [].
	time.Sleep(2 * time.Millisecond)
	must(t, s.PutRole(ctx, &domain.CustomRole{TenantID: "t1", Name: "releaser", Description: "v2"}))
	got, err = s.GetRole(ctx, "t1", "releaser")
	must(t, err)
	if got.Description != "v2" || !got.CreatedAt.Equal(r.CreatedAt) || !got.UpdatedAt.After(r.UpdatedAt) {
		t.Fatalf("PutRole replace: %+v (orig %+v)", got, r)
	}
	if got.Permissions == nil || len(got.Permissions) != 0 {
		t.Fatalf("Permissions = %#v, want empty non-nil", got.Permissions)
	}

	must(t, s.PutRole(ctx, &domain.CustomRole{TenantID: "t1", Name: "auditor", CreatedAt: at(1)}))
	must(t, s.PutRole(ctx, &domain.CustomRole{TenantID: "t2", Name: "zzz"}))
	list, err := s.ListRoles(ctx, "t1")
	must(t, err)
	if len(list) != 2 || list[0].Name != "auditor" || list[1].Name != "releaser" || !list[0].CreatedAt.Equal(at(1)) {
		t.Fatalf("ListRoles: %+v", list)
	}

	must(t, s.DeleteRole(ctx, "t1", "releaser"))
	wantErr(t, s.DeleteRole(ctx, "t1", "releaser"), domain.ErrNotFound)
	list, err = s.ListRoles(ctx, "t1")
	must(t, err)
	if len(list) != 1 || list[0].Name != "auditor" {
		t.Fatalf("ListRoles after delete: %+v", list)
	}
}

func samplePolicy() *domain.Policy {
	nb, na := at(100), at(200)
	return &domain.Policy{
		TenantID: "t1", Name: "freeze", Description: "release freeze", Enabled: true, Version: 7, CreatedBy: "alice",
		Statements: []domain.PolicyStatement{
			{
				Sid: "deny-main", Effect: domain.EffectDeny,
				Actions:    []string{string(domain.ActBranchPush)},
				Resources:  []string{"wt:branch/acme/*/main"},
				Principals: []string{"*"},
				Conditions: domain.PolicyConditions{
					IPRanges: []string{"10.0.0.0/8"}, RequireMFA: true, NotBefore: &nb, NotAfter: &na,
					TimeWindow: &domain.TimeWindow{Days: []string{"sat", "sun"}, Start: "00:00", End: "23:59", Timezone: "Europe/Berlin"},
				},
			},
			{
				Effect: domain.EffectAllow, Actions: []string{"tree:*"}, Resources: []string{"wt:tree/acme/**"},
				Principals: []string{"team:core", "role:releaser"},
			},
		},
	}
}

func testPolicies(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	p := samplePolicy()
	must(t, s.CreatePolicy(ctx, p))
	if p.ID == "" || p.Version != 1 || p.CreatedAt.IsZero() || !p.UpdatedAt.Equal(p.CreatedAt) {
		t.Fatalf("CreatePolicy defaults: %+v", p)
	}
	got, err := s.GetPolicy(ctx, "t1", p.ID)
	must(t, err)
	wantEqual(t, "GetPolicy", *got, *p)
	_, err = s.GetPolicy(ctx, "t2", p.ID)
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetPolicy(ctx, "t1", "nope")
	wantErr(t, err, domain.ErrNotFound)
	wantErr(t, s.CreatePolicy(ctx, &domain.Policy{ID: p.ID, TenantID: "t1", Name: "dup"}), domain.ErrAlreadyExists)

	// Nil slices are stored as empty ones; times are normalized to UTC.
	local := at(5).In(time.FixedZone("z", 5*3600))
	bare := &domain.Policy{TenantID: "t1", Name: "alpha", Statements: []domain.PolicyStatement{
		{Effect: domain.EffectAllow, Conditions: domain.PolicyConditions{NotAfter: &local, TimeWindow: &domain.TimeWindow{Start: "09:00", End: "17:00"}}},
	}}
	must(t, s.CreatePolicy(ctx, bare))
	got, err = s.GetPolicy(ctx, "t1", bare.ID)
	must(t, err)
	st := got.Statements[0]
	if st.Actions == nil || st.Resources == nil || st.Principals == nil || st.Conditions.TimeWindow.Days == nil {
		t.Fatalf("nil slices not normalized: %+v", st)
	}
	if st.Conditions.IPRanges != nil {
		t.Fatalf("empty IPRanges = %#v, want nil", st.Conditions.IPRanges)
	}
	if !st.Conditions.NotAfter.Equal(local) || st.Conditions.NotAfter.Location() != time.UTC {
		t.Fatalf("NotAfter = %v, want %v in UTC", st.Conditions.NotAfter, local)
	}
	empty := &domain.Policy{TenantID: "t1", Name: "zulu"}
	must(t, s.CreatePolicy(ctx, empty))
	got, err = s.GetPolicy(ctx, "t1", empty.ID)
	must(t, err)
	if got.Statements == nil || len(got.Statements) != 0 {
		t.Fatalf("Statements = %#v, want empty non-nil", got.Statements)
	}
	must(t, s.CreatePolicy(ctx, &domain.Policy{TenantID: "t2", Name: "other"}))

	list, err := s.ListPolicies(ctx, "t1")
	must(t, err)
	names := []string{}
	for _, x := range list {
		names = append(names, x.Name)
	}
	wantEqual(t, "ListPolicies order", names, []string{"alpha", "freeze", "zulu"})
	wantEqual(t, "listed policy", list[1], *p)

	// Update: version increments from the stored value, CreatedAt and
	// CreatedBy are preserved.
	upd := samplePolicy()
	upd.ID = p.ID
	upd.Enabled = false
	upd.Version = 42
	upd.CreatedBy = ""
	upd.Statements = upd.Statements[:1]
	time.Sleep(2 * time.Millisecond)
	must(t, s.UpdatePolicy(ctx, upd))
	if upd.Version != 2 || upd.CreatedBy != "alice" || !upd.CreatedAt.Equal(p.CreatedAt) || !upd.UpdatedAt.After(p.UpdatedAt) {
		t.Fatalf("UpdatePolicy write-back: %+v", upd)
	}
	got, err = s.GetPolicy(ctx, "t1", p.ID)
	must(t, err)
	wantEqual(t, "updated policy", *got, *upd)
	must(t, s.UpdatePolicy(ctx, got))
	if got.Version != 3 {
		t.Fatalf("second update version = %d, want 3", got.Version)
	}
	wantErr(t, s.UpdatePolicy(ctx, &domain.Policy{TenantID: "t1", ID: "missing", Name: "x"}), domain.ErrNotFound)
	wantErr(t, s.UpdatePolicy(ctx, &domain.Policy{TenantID: "t2", ID: p.ID, Name: "x"}), domain.ErrNotFound)
	wantErr(t, s.UpdatePolicy(ctx, &domain.Policy{TenantID: "t1", ID: p.ID}), domain.ErrInvalid)

	must(t, s.DeletePolicy(ctx, "t1", p.ID))
	wantErr(t, s.DeletePolicy(ctx, "t1", p.ID), domain.ErrNotFound)
	list, err = s.ListPolicies(ctx, "t1")
	must(t, err)
	if len(list) != 2 {
		t.Fatalf("ListPolicies after delete: %+v", list)
	}
}

func testGrants(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	trA, trB := mkTree(t, s, "t1", "a"), mkTree(t, s, "t1", "b")
	team := mkTeam(t, s, "t1", "core")
	sa := mkSA(t, s, "t1", "ci")

	wantErr(t, s.PutGrant(ctx, &domain.Grant{TreeID: "missing", SubjectType: domain.SubjectUser, SubjectID: "u1", Role: domain.RoleViewer}), domain.ErrNotFound)
	wantErr(t, s.PutGrant(ctx, &domain.Grant{TreeID: trA.ID, SubjectType: domain.SubjectTeam, SubjectID: "missing", Role: domain.RoleViewer}), domain.ErrNotFound)
	wantErr(t, s.PutGrant(ctx, &domain.Grant{TreeID: trA.ID, SubjectType: domain.SubjectServiceAccount, SubjectID: "missing", Role: domain.RoleViewer}), domain.ErrNotFound)
	wantErr(t, s.PutGrant(ctx, &domain.Grant{TreeID: trA.ID, SubjectType: "group", SubjectID: "u1", Role: domain.RoleViewer}), domain.ErrInvalid)

	g1 := &domain.Grant{TreeID: trA.ID, SubjectType: domain.SubjectUser, SubjectID: "u-zed", Subject: "zed", Role: domain.RoleDeveloper, CreatedBy: "alice"}
	must(t, s.PutGrant(ctx, g1))
	if g1.CreatedAt.IsZero() {
		t.Fatal("PutGrant did not fill CreatedAt")
	}
	must(t, s.PutGrant(ctx, &domain.Grant{TreeID: trA.ID, SubjectType: domain.SubjectUser, SubjectID: "u-amy", Subject: "amy", Role: domain.RoleViewer, CreatedAt: at(1)}))
	must(t, s.PutGrant(ctx, &domain.Grant{TreeID: trA.ID, SubjectType: domain.SubjectTeam, SubjectID: team.ID, Subject: "core", Role: domain.RoleMaintainer}))
	must(t, s.PutGrant(ctx, &domain.Grant{TreeID: trA.ID, SubjectType: domain.SubjectServiceAccount, SubjectID: sa.ID, Subject: "ci", Role: domain.RoleDeveloper}))
	must(t, s.PutGrant(ctx, &domain.Grant{TreeID: trB.ID, SubjectType: domain.SubjectUser, SubjectID: "u-zed", Subject: "zed", Role: "releaser"}))

	subjects := func(gs []domain.Grant) []string {
		out := []string{}
		for _, g := range gs {
			out = append(out, g.SubjectType+":"+g.Subject)
		}
		return out
	}
	list, err := s.ListGrants(ctx, trA.ID)
	must(t, err)
	wantEqual(t, "ListGrants order", subjects(list), []string{"service_account:ci", "team:core", "user:amy", "user:zed"})
	wantEqual(t, "grant", list[3], *g1)

	// Replace keeps CreatedAt.
	must(t, s.PutGrant(ctx, &domain.Grant{TreeID: trA.ID, SubjectType: domain.SubjectUser, SubjectID: "u-zed", Subject: "zed", Role: domain.RoleAdmin}))
	list, err = s.ListGrants(ctx, trA.ID)
	must(t, err)
	if len(list) != 4 || list[3].Role != domain.RoleAdmin || !list[3].CreatedAt.Equal(g1.CreatedAt) || list[3].CreatedBy != "" {
		t.Fatalf("PutGrant replace: %+v", list[3])
	}

	sg, err := s.ListSubjectGrants(ctx, domain.SubjectUser, "u-zed")
	must(t, err)
	wantTrees := []string{trA.ID, trB.ID}
	if wantTrees[0] > wantTrees[1] {
		wantTrees[0], wantTrees[1] = wantTrees[1], wantTrees[0]
	}
	if len(sg) != 2 || sg[0].TreeID != wantTrees[0] || sg[1].TreeID != wantTrees[1] {
		t.Fatalf("ListSubjectGrants (by tree id): %+v, want %v", sg, wantTrees)
	}
	sg, err = s.ListSubjectGrants(ctx, domain.SubjectTeam, "u-zed")
	must(t, err)
	if len(sg) != 0 {
		t.Fatalf("ListSubjectGrants matched the wrong subject type: %+v", sg)
	}

	must(t, s.DeleteGrant(ctx, trA.ID, domain.SubjectUser, "u-zed"))
	wantErr(t, s.DeleteGrant(ctx, trA.ID, domain.SubjectUser, "u-zed"), domain.ErrNotFound)
	wantErr(t, s.DeleteGrant(ctx, trA.ID, domain.SubjectTeam, "u-amy"), domain.ErrNotFound)
	sg, err = s.ListSubjectGrants(ctx, domain.SubjectUser, "u-zed")
	must(t, err)
	if len(sg) != 1 || sg[0].TreeID != trB.ID {
		t.Fatalf("ListSubjectGrants after delete: %+v", sg)
	}
	list, err = s.ListGrants(ctx, trA.ID)
	must(t, err)
	if len(list) != 3 {
		t.Fatalf("ListGrants after delete: %+v", list)
	}
}

func testDeleteTreeRemovesGrants(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	doomed, keep := mkTree(t, s, "t1", "doomed"), mkTree(t, s, "t1", "keep")
	team := mkTeam(t, s, "t1", "core")
	for _, tr := range []*domain.Tree{doomed, keep} {
		must(t, s.PutGrant(ctx, &domain.Grant{TreeID: tr.ID, SubjectType: domain.SubjectUser, SubjectID: "u1", Role: domain.RoleViewer}))
		must(t, s.PutGrant(ctx, &domain.Grant{TreeID: tr.ID, SubjectType: domain.SubjectTeam, SubjectID: team.ID, Role: domain.RoleViewer}))
	}
	must(t, s.DeleteTree(ctx, doomed.ID))
	gs, err := s.ListGrants(ctx, doomed.ID)
	must(t, err)
	if len(gs) != 0 {
		t.Fatalf("grants of deleted tree survived: %+v", gs)
	}
	for _, sub := range [][2]string{{domain.SubjectUser, "u1"}, {domain.SubjectTeam, team.ID}} {
		sg, err := s.ListSubjectGrants(ctx, sub[0], sub[1])
		must(t, err)
		if len(sg) != 1 || sg[0].TreeID != keep.ID {
			t.Fatalf("ListSubjectGrants(%v) after tree delete: %+v", sub, sg)
		}
	}
	gs, err = s.ListGrants(ctx, keep.ID)
	must(t, err)
	if len(gs) != 2 {
		t.Fatalf("surviving tree grants: %+v", gs)
	}
}

func testServiceAccounts(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	ci := &domain.ServiceAccount{TenantID: "t1", Name: "ci", Description: "CI bot", Role: domain.RoleDeveloper, CreatedBy: "alice"}
	must(t, s.CreateServiceAccount(ctx, ci))
	if ci.ID == "" || ci.CreatedAt.IsZero() || !ci.UpdatedAt.Equal(ci.CreatedAt) {
		t.Fatalf("CreateServiceAccount defaults: %+v", ci)
	}
	wantErr(t, s.CreateServiceAccount(ctx, &domain.ServiceAccount{TenantID: "t1", Name: "ci"}), domain.ErrAlreadyExists)
	wantErr(t, s.CreateServiceAccount(ctx, &domain.ServiceAccount{ID: ci.ID, TenantID: "t2", Name: "x"}), domain.ErrAlreadyExists)
	must(t, s.CreateServiceAccount(ctx, &domain.ServiceAccount{TenantID: "t2", Name: "ci"}))
	must(t, s.CreateServiceAccount(ctx, &domain.ServiceAccount{TenantID: "t1", Name: "backup", Disabled: true}))

	got, err := s.GetServiceAccount(ctx, ci.ID)
	must(t, err)
	wantEqual(t, "GetServiceAccount", *got, *ci)
	got, err = s.GetServiceAccountByName(ctx, "t1", "ci")
	must(t, err)
	wantEqual(t, "GetServiceAccountByName", *got, *ci)
	_, err = s.GetServiceAccount(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetServiceAccountByName(ctx, "t3", "ci")
	wantErr(t, err, domain.ErrNotFound)

	list, err := s.ListServiceAccounts(ctx, "t1")
	must(t, err)
	if len(list) != 2 || list[0].Name != "backup" || !list[0].Disabled || list[1].Name != "ci" {
		t.Fatalf("ListServiceAccounts: %+v", list)
	}

	upd := *ci
	upd.Name = "deployer"
	upd.Role = domain.RoleMaintainer
	upd.Disabled = true
	upd.CreatedAt = time.Time{}
	upd.CreatedBy = ""
	time.Sleep(2 * time.Millisecond)
	must(t, s.UpdateServiceAccount(ctx, &upd))
	got, err = s.GetServiceAccountByName(ctx, "t1", "deployer")
	must(t, err)
	if !got.Disabled || got.Role != domain.RoleMaintainer || got.CreatedBy != "alice" ||
		!got.CreatedAt.Equal(ci.CreatedAt) || !got.UpdatedAt.After(ci.UpdatedAt) {
		t.Fatalf("UpdateServiceAccount result: %+v", got)
	}
	wantEqual(t, "UpdateServiceAccount writes back", *got, upd)
	_, err = s.GetServiceAccountByName(ctx, "t1", "ci")
	wantErr(t, err, domain.ErrNotFound)
	upd.Name = "backup"
	wantErr(t, s.UpdateServiceAccount(ctx, &upd), domain.ErrAlreadyExists)
	upd.Name = "deployer"
	upd.TenantID = "t2"
	wantErr(t, s.UpdateServiceAccount(ctx, &upd), domain.ErrInvalid)
	wantErr(t, s.UpdateServiceAccount(ctx, &domain.ServiceAccount{ID: "missing", TenantID: "t1", Name: "m"}), domain.ErrNotFound)
	must(t, s.CreateServiceAccount(ctx, &domain.ServiceAccount{TenantID: "t1", Name: "ci"})) // old name free
}

func testServiceAccountKeys(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	sa := mkSA(t, s, "t1", "ci")
	other := mkSA(t, s, "t1", "other")
	exp := at(9000)
	k1 := &domain.APIToken{ServiceAccountID: sa.ID, Name: "deploy", TokenHash: "sa-h1", Kind: domain.TokenService,
		Scopes: []string{"tree:read", "tree:write"}, CreatedAt: at(5), ExpiresAt: &exp}
	must(t, s.CreateToken(ctx, k1))
	must(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: sa.ID, Name: "old", TokenHash: "sa-h2", Kind: domain.TokenService, CreatedAt: at(1)}))
	must(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: other.ID, TokenHash: "sa-h3", Kind: domain.TokenService}))
	must(t, s.CreateToken(ctx, &domain.APIToken{AccountID: "u1", Name: "personal", TokenHash: "u-h1"}))

	wantErr(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: "missing", TokenHash: "x"}), domain.ErrNotFound)
	wantErr(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: sa.ID, AccountID: "u1", TokenHash: "y"}), domain.ErrInvalid)
	wantErr(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: sa.ID, TokenHash: "u-h1"}), domain.ErrAlreadyExists)

	got, err := s.GetTokenByHash(ctx, "sa-h1")
	must(t, err)
	wantEqual(t, "service key by hash", *got, *k1)
	if got.AccountID != "" || got.ServiceAccountID != sa.ID {
		t.Fatalf("service key owner: %+v", got)
	}

	keys, err := s.ListServiceAccountTokens(ctx, sa.ID)
	must(t, err)
	if len(keys) != 2 || keys[0].Name != "old" || keys[1].Name != "deploy" {
		t.Fatalf("ListServiceAccountTokens (by created_at): %+v", keys)
	}
	// Service-account keys never show up as account tokens.
	for _, acct := range []string{"", sa.ID, "u1"} {
		toks, err := s.ListTokens(ctx, acct)
		must(t, err)
		for _, tk := range toks {
			if tk.ServiceAccountID != "" {
				t.Fatalf("ListTokens(%q) returned a service key: %+v", acct, tk)
			}
		}
		if acct == "u1" && len(toks) != 1 {
			t.Fatalf("ListTokens(u1) = %+v", toks)
		}
		if acct != "u1" && len(toks) != 0 {
			t.Fatalf("ListTokens(%q) = %+v, want none", acct, toks)
		}
	}
	keys, err = s.ListServiceAccountTokens(ctx, "u1")
	must(t, err)
	if len(keys) != 0 {
		t.Fatalf("ListServiceAccountTokens(u1) returned account tokens: %+v", keys)
	}

	// Touch and delete work on service keys like on any token.
	must(t, s.TouchToken(ctx, k1.ID, at(50)))
	got, err = s.GetTokenByHash(ctx, "sa-h1")
	must(t, err)
	if got.LastUsedAt == nil || !got.LastUsedAt.Equal(at(50)) || got.ServiceAccountID != sa.ID {
		t.Fatalf("TouchToken on service key: %+v", got)
	}
	must(t, s.DeleteToken(ctx, k1.ID))
	_, err = s.GetTokenByHash(ctx, "sa-h1")
	wantErr(t, err, domain.ErrNotFound)
	keys, err = s.ListServiceAccountTokens(ctx, sa.ID)
	must(t, err)
	if len(keys) != 1 || keys[0].Name != "old" {
		t.Fatalf("ListServiceAccountTokens after delete: %+v", keys)
	}
}

func testDeleteServiceAccountCascades(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	tr := mkTree(t, s, "t1", "web")
	doomed, keep := mkSA(t, s, "t1", "doomed"), mkSA(t, s, "t1", "keep")
	for _, sa := range []*domain.ServiceAccount{doomed, keep} {
		must(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: sa.ID, TokenHash: "k1-" + sa.Name, Kind: domain.TokenService}))
		must(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: sa.ID, TokenHash: "k2-" + sa.Name, Kind: domain.TokenService}))
		must(t, s.PutGrant(ctx, &domain.Grant{TreeID: tr.ID, SubjectType: domain.SubjectServiceAccount, SubjectID: sa.ID, Subject: sa.Name, Role: domain.RoleViewer}))
	}
	must(t, s.CreateToken(ctx, &domain.APIToken{AccountID: "u1", TokenHash: "user-key"}))

	must(t, s.DeleteServiceAccount(ctx, doomed.ID))
	wantErr(t, s.DeleteServiceAccount(ctx, doomed.ID), domain.ErrNotFound)
	_, err := s.GetServiceAccountByName(ctx, "t1", "doomed")
	wantErr(t, err, domain.ErrNotFound)
	for _, h := range []string{"k1-doomed", "k2-doomed"} {
		_, err := s.GetTokenByHash(ctx, h)
		wantErr(t, err, domain.ErrNotFound)
	}
	keys, err := s.ListServiceAccountTokens(ctx, doomed.ID)
	must(t, err)
	if len(keys) != 0 {
		t.Fatalf("keys of deleted service account survived: %+v", keys)
	}
	gs, err := s.ListSubjectGrants(ctx, domain.SubjectServiceAccount, doomed.ID)
	must(t, err)
	if len(gs) != 0 {
		t.Fatalf("grants of deleted service account survived: %+v", gs)
	}
	gs, err = s.ListGrants(ctx, tr.ID)
	must(t, err)
	if len(gs) != 1 || gs[0].SubjectID != keep.ID {
		t.Fatalf("tree grants after service account delete: %+v", gs)
	}
	keys, err = s.ListServiceAccountTokens(ctx, keep.ID)
	must(t, err)
	if len(keys) != 2 {
		t.Fatalf("surviving service account keys: %+v", keys)
	}
	if _, err := s.GetTokenByHash(ctx, "user-key"); err != nil {
		t.Fatalf("user token removed by service account delete: %v", err)
	}
	// Hashes and the name are free again.
	re := mkSA(t, s, "t1", "doomed")
	must(t, s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: re.ID, TokenHash: "k1-doomed"}))
}

func testTokenIAMFields(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	tok := &domain.APIToken{AccountID: "u1", Name: "session", TokenHash: "sess", Kind: domain.TokenSession,
		Scopes: []string{"tree:read"}, MFA: true, LastIP: "203.0.113.7", UserAgent: "wt/0.4 (windows)", CreatedAt: at(1)}
	must(t, s.CreateToken(ctx, tok))
	got, err := s.GetTokenByHash(ctx, "sess")
	must(t, err)
	wantEqual(t, "token with IAM fields", *got, *tok)
	list, err := s.ListTokens(ctx, "u1")
	must(t, err)
	if len(list) != 1 {
		t.Fatalf("ListTokens: %+v", list)
	}
	wantEqual(t, "listed token", list[0], *tok)
	// Empty scopes read back as nil (unrestricted) in both stores.
	must(t, s.CreateToken(ctx, &domain.APIToken{AccountID: "u1", TokenHash: "api", Kind: domain.TokenAPI, Scopes: []string{}}))
	got, err = s.GetTokenByHash(ctx, "api")
	must(t, err)
	if got.Scopes != nil || got.Kind != domain.TokenAPI {
		t.Fatalf("empty scopes token: %+v", got)
	}
}

func testMFA(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	_, err := s.GetMFA(ctx, "u1")
	wantErr(t, err, domain.ErrNotFound)
	enabled := at(10)
	st := &domain.MFAState{AccountID: "u1", TOTPEnabled: true, TOTPSecret: "JBSWY3DPEHPK3PXP", PendingSecret: "PENDING",
		RecoveryCodeHashes: []string{"h1", "h2", "h3"}, LastUsedStep: 58000000, EnabledAt: &enabled, RecoveryCodesLeft: 99}
	must(t, s.PutMFA(ctx, st))
	if st.RecoveryCodesLeft != 3 {
		t.Fatalf("RecoveryCodesLeft = %d, want 3 (derived)", st.RecoveryCodesLeft)
	}
	got, err := s.GetMFA(ctx, "u1")
	must(t, err)
	wantEqual(t, "GetMFA", *got, *st)

	// Replace: use a recovery code.
	got.RecoveryCodeHashes = got.RecoveryCodeHashes[1:]
	got.LastUsedStep++
	must(t, s.PutMFA(ctx, got))
	got2, err := s.GetMFA(ctx, "u1")
	must(t, err)
	if got2.RecoveryCodesLeft != 2 || len(got2.RecoveryCodeHashes) != 2 || got2.LastUsedStep != 58000001 || got2.TOTPSecret != "JBSWY3DPEHPK3PXP" {
		t.Fatalf("PutMFA replace: %+v", got2)
	}

	// Nil hashes are stored as empty.
	must(t, s.PutMFA(ctx, &domain.MFAState{AccountID: "u2", PendingSecret: "P2"}))
	got, err = s.GetMFA(ctx, "u2")
	must(t, err)
	if got.RecoveryCodeHashes == nil || got.RecoveryCodesLeft != 0 || got.PendingSecret != "P2" || got.EnabledAt != nil {
		t.Fatalf("minimal MFA state: %+v", got)
	}

	must(t, s.DeleteMFA(ctx, "u1"))
	wantErr(t, s.DeleteMFA(ctx, "u1"), domain.ErrNotFound)
	_, err = s.GetMFA(ctx, "u1")
	wantErr(t, err, domain.ErrNotFound)
	if _, err := s.GetMFA(ctx, "u2"); err != nil {
		t.Fatalf("DeleteMFA removed another account's state: %v", err)
	}
}

func testInvitations(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	inv := &domain.Invitation{TenantID: "t1", Email: "new@example.com", Role: domain.RoleDeveloper, TeamID: "team-1",
		CodeHash: "code-1", InvitedBy: "alice", CreatedAt: at(10), ExpiresAt: at(10 + 7*24*3600)}
	must(t, s.CreateInvitation(ctx, inv))
	if inv.ID == "" {
		t.Fatal("CreateInvitation did not fill ID")
	}
	wantErr(t, s.CreateInvitation(ctx, &domain.Invitation{TenantID: "t2", CodeHash: "code-1"}), domain.ErrAlreadyExists)
	wantErr(t, s.CreateInvitation(ctx, &domain.Invitation{ID: inv.ID, TenantID: "t1", CodeHash: "code-x"}), domain.ErrAlreadyExists)
	older := &domain.Invitation{TenantID: "t1", Username: "bob", Role: domain.RoleViewer, CodeHash: "code-2", CreatedAt: at(5)}
	must(t, s.CreateInvitation(ctx, older))
	newest := &domain.Invitation{TenantID: "t1", Role: domain.RoleViewer, CodeHash: "code-3"}
	must(t, s.CreateInvitation(ctx, newest))
	if newest.CreatedAt.IsZero() {
		t.Fatal("CreateInvitation did not fill CreatedAt")
	}
	must(t, s.CreateInvitation(ctx, &domain.Invitation{TenantID: "t2", CodeHash: "code-4"}))

	got, err := s.GetInvitation(ctx, "t1", inv.ID)
	must(t, err)
	wantEqual(t, "GetInvitation", *got, *inv)
	if got.CodeHash != "code-1" {
		t.Fatalf("CodeHash not persisted: %q", got.CodeHash)
	}
	got, err = s.GetInvitationByCodeHash(ctx, "code-2")
	must(t, err)
	wantEqual(t, "GetInvitationByCodeHash", *got, *older)
	_, err = s.GetInvitation(ctx, "t2", inv.ID)
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetInvitationByCodeHash(ctx, "nope")
	wantErr(t, err, domain.ErrNotFound)

	list, err := s.ListInvitations(ctx, "t1")
	must(t, err)
	if len(list) != 3 || list[0].ID != newest.ID || list[1].ID != inv.ID || list[2].ID != older.ID {
		t.Fatalf("ListInvitations (newest first): %+v", list)
	}

	// Accept: update without CodeHash keeps it.
	accepted := at(20)
	upd := *inv
	upd.CodeHash = ""
	upd.CreatedAt = time.Time{}
	upd.AcceptedAt = &accepted
	upd.AcceptedBy = "carol"
	must(t, s.UpdateInvitation(ctx, &upd))
	if upd.CodeHash != "code-1" || !upd.CreatedAt.Equal(at(10)) {
		t.Fatalf("UpdateInvitation write-back: %+v", upd)
	}
	got, err = s.GetInvitationByCodeHash(ctx, "code-1")
	must(t, err)
	wantEqual(t, "accepted invitation", *got, upd)

	// Re-keying the code hash moves the index; taken hashes conflict.
	upd.CodeHash = "code-2"
	wantErr(t, s.UpdateInvitation(ctx, &upd), domain.ErrAlreadyExists)
	upd.CodeHash = "code-1b"
	upd.Revoked = true
	must(t, s.UpdateInvitation(ctx, &upd))
	_, err = s.GetInvitationByCodeHash(ctx, "code-1")
	wantErr(t, err, domain.ErrNotFound)
	got, err = s.GetInvitationByCodeHash(ctx, "code-1b")
	must(t, err)
	if got.ID != inv.ID || !got.Revoked {
		t.Fatalf("re-keyed invitation: %+v", got)
	}
	wantErr(t, s.UpdateInvitation(ctx, &domain.Invitation{TenantID: "t1", ID: "missing"}), domain.ErrNotFound)
	wantErr(t, s.UpdateInvitation(ctx, &domain.Invitation{TenantID: "t2", ID: inv.ID}), domain.ErrNotFound)

	must(t, s.DeleteInvitation(ctx, "t1", inv.ID))
	wantErr(t, s.DeleteInvitation(ctx, "t1", inv.ID), domain.ErrNotFound)
	_, err = s.GetInvitationByCodeHash(ctx, "code-1b")
	wantErr(t, err, domain.ErrNotFound)
	must(t, s.CreateInvitation(ctx, &domain.Invitation{TenantID: "t1", CodeHash: "code-1b"})) // hash reusable
	list, err = s.ListInvitations(ctx, "t1")
	must(t, err)
	if len(list) != 3 {
		t.Fatalf("ListInvitations after delete: %+v", list)
	}
}

func testPasswordResets(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	_, err := s.GetPasswordReset(ctx, "r1")
	wantErr(t, err, domain.ErrNotFound)
	r := &domain.PasswordReset{CodeHash: "r1", AccountID: "u1", CreatedBy: "admin", ExpiresAt: at(3600)}
	must(t, s.PutPasswordReset(ctx, r))
	if r.CreatedAt.IsZero() {
		t.Fatal("PutPasswordReset did not fill CreatedAt")
	}
	got, err := s.GetPasswordReset(ctx, "r1")
	must(t, err)
	wantEqual(t, "GetPasswordReset", *got, *r)
	if got.CodeHash != "r1" {
		t.Fatalf("CodeHash not returned: %q", got.CodeHash)
	}
	used := at(60)
	got.UsedAt = &used
	must(t, s.PutPasswordReset(ctx, got))
	got2, err := s.GetPasswordReset(ctx, "r1")
	must(t, err)
	wantEqual(t, "replaced reset", *got2, *got)
	must(t, s.PutPasswordReset(ctx, &domain.PasswordReset{CodeHash: "r2", AccountID: "u1"}))
	must(t, s.DeletePasswordReset(ctx, "r1"))
	wantErr(t, s.DeletePasswordReset(ctx, "r1"), domain.ErrNotFound)
	_, err = s.GetPasswordReset(ctx, "r1")
	wantErr(t, err, domain.ErrNotFound)
	if _, err := s.GetPasswordReset(ctx, "r2"); err != nil {
		t.Fatalf("other reset removed: %v", err)
	}
}

func testSSOConfig(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	_, err := s.GetSSOConfig(ctx)
	wantErr(t, err, domain.ErrNotFound)
	c := &domain.SSOConfig{Enabled: true, Name: "Okta", Issuer: "https://dev-1.okta.com", ClientID: "cid",
		ClientSecret: "s3cret", RedirectURL: "https://wt.example.com/api/v1/auth/sso/callback",
		Scopes: []string{"openid", "email"}, AllowedDomains: []string{"example.com"}, AutoCreate: true, UsernameClaim: "preferred_username"}
	must(t, s.PutSSOConfig(ctx, c))
	if !c.HasSecret || c.UpdatedAt.IsZero() {
		t.Fatalf("PutSSOConfig derived fields: %+v", c)
	}
	got, err := s.GetSSOConfig(ctx)
	must(t, err)
	wantEqual(t, "GetSSOConfig", *got, *c)
	if got.ClientSecret != "s3cret" {
		t.Fatalf("ClientSecret not persisted: %q", got.ClientSecret)
	}
	// Replace without a secret; HasSecret follows; nil slices become [].
	time.Sleep(2 * time.Millisecond)
	must(t, s.PutSSOConfig(ctx, &domain.SSOConfig{Name: "Off", HasSecret: true}))
	got2, err := s.GetSSOConfig(ctx)
	must(t, err)
	if got2.HasSecret || got2.ClientSecret != "" || got2.Name != "Off" || got2.Scopes == nil || got2.AllowedDomains == nil || !got2.UpdatedAt.After(got.UpdatedAt) {
		t.Fatalf("PutSSOConfig replace: %+v", got2)
	}
}

func testSSOIdentities(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	iss := "https://idp.example.com"
	id := &domain.SSOIdentity{Issuer: iss, Subject: "sub-1", AccountID: "u1", Email: "a@example.com"}
	must(t, s.PutSSOIdentity(ctx, id))
	if id.CreatedAt.IsZero() || !id.LastLoginAt.Equal(id.CreatedAt) {
		t.Fatalf("PutSSOIdentity defaults: %+v", id)
	}
	got, err := s.GetSSOIdentity(ctx, iss, "sub-1")
	must(t, err)
	wantEqual(t, "GetSSOIdentity", *got, *id)
	_, err = s.GetSSOIdentity(ctx, iss, "sub-2")
	wantErr(t, err, domain.ErrNotFound)
	_, err = s.GetSSOIdentity(ctx, "https://other", "sub-1")
	wantErr(t, err, domain.ErrNotFound)

	must(t, s.PutSSOIdentity(ctx, &domain.SSOIdentity{Issuer: "https://a.example.com", Subject: "z", AccountID: "u1", CreatedAt: at(1), LastLoginAt: at(2)}))
	must(t, s.PutSSOIdentity(ctx, &domain.SSOIdentity{Issuer: iss, Subject: "sub-0", AccountID: "u1", CreatedAt: at(3)}))
	must(t, s.PutSSOIdentity(ctx, &domain.SSOIdentity{Issuer: iss, Subject: "sub-9", AccountID: "u2"}))
	keys := func(ids []domain.SSOIdentity) []string {
		out := []string{}
		for _, x := range ids {
			out = append(out, x.Issuer+"|"+x.Subject)
		}
		return out
	}
	list, err := s.ListAccountSSOIdentities(ctx, "u1")
	must(t, err)
	wantEqual(t, "ListAccountSSOIdentities order", keys(list),
		[]string{"https://a.example.com|z", iss + "|sub-0", iss + "|sub-1"})

	// Upsert keeps CreatedAt, records the login, and may relink the account.
	must(t, s.PutSSOIdentity(ctx, &domain.SSOIdentity{Issuer: iss, Subject: "sub-1", AccountID: "u2", Email: "b@example.com", LastLoginAt: at(500)}))
	got, err = s.GetSSOIdentity(ctx, iss, "sub-1")
	must(t, err)
	if got.AccountID != "u2" || !got.CreatedAt.Equal(id.CreatedAt) || !got.LastLoginAt.Equal(at(500)) || got.Email != "b@example.com" {
		t.Fatalf("PutSSOIdentity upsert: %+v", got)
	}
	list, err = s.ListAccountSSOIdentities(ctx, "u1")
	must(t, err)
	wantEqual(t, "u1 after relink", keys(list), []string{"https://a.example.com|z", iss + "|sub-0"})
	list, err = s.ListAccountSSOIdentities(ctx, "u2")
	must(t, err)
	wantEqual(t, "u2 after relink", keys(list), []string{iss + "|sub-1", iss + "|sub-9"})
}

func testIAMInvalidArguments(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	nul := "bad\x00name"
	tr := mkTree(t, s, "t1", "web")
	team := mkTeam(t, s, "t1", "core")
	calls := map[string]error{
		"nil team":               s.CreateTeam(ctx, nil),
		"team no tenant":         s.CreateTeam(ctx, &domain.Team{Slug: "x"}),
		"team no slug":           s.CreateTeam(ctx, &domain.Team{TenantID: "t1"}),
		"team nul slug":          s.CreateTeam(ctx, &domain.Team{TenantID: "t1", Slug: nul}),
		"team nul id":            s.CreateTeam(ctx, &domain.Team{ID: nul, TenantID: "t1", Slug: "ok"}),
		"update nil team":        s.UpdateTeam(ctx, nil),
		"update team no id":      s.UpdateTeam(ctx, &domain.Team{TenantID: "t1", Slug: "x"}),
		"nil team member":        s.PutTeamMember(ctx, nil),
		"member no account":      s.PutTeamMember(ctx, &domain.TeamMember{TeamID: team.ID}),
		"member no team":         s.PutTeamMember(ctx, &domain.TeamMember{AccountID: "u1"}),
		"nil role":               s.PutRole(ctx, nil),
		"role no name":           s.PutRole(ctx, &domain.CustomRole{TenantID: "t1"}),
		"role no tenant":         s.PutRole(ctx, &domain.CustomRole{Name: "x"}),
		"role builtin":           s.PutRole(ctx, &domain.CustomRole{TenantID: "t1", Name: domain.RoleOwner}),
		"nil policy":             s.CreatePolicy(ctx, nil),
		"policy no tenant":       s.CreatePolicy(ctx, &domain.Policy{Name: "x"}),
		"policy no name":         s.CreatePolicy(ctx, &domain.Policy{TenantID: "t1"}),
		"update nil policy":      s.UpdatePolicy(ctx, nil),
		"update policy no id":    s.UpdatePolicy(ctx, &domain.Policy{TenantID: "t1", Name: "x"}),
		"nil grant":              s.PutGrant(ctx, nil),
		"grant no tree":          s.PutGrant(ctx, &domain.Grant{SubjectType: domain.SubjectUser, SubjectID: "u", Role: "viewer"}),
		"grant no subject":       s.PutGrant(ctx, &domain.Grant{TreeID: tr.ID, SubjectType: domain.SubjectUser, Role: "viewer"}),
		"grant no role":          s.PutGrant(ctx, &domain.Grant{TreeID: tr.ID, SubjectType: domain.SubjectUser, SubjectID: "u"}),
		"grant bad type":         s.PutGrant(ctx, &domain.Grant{TreeID: tr.ID, SubjectType: "", SubjectID: "u", Role: "viewer"}),
		"nil service account":    s.CreateServiceAccount(ctx, nil),
		"sa no name":             s.CreateServiceAccount(ctx, &domain.ServiceAccount{TenantID: "t1"}),
		"sa nul name":            s.CreateServiceAccount(ctx, &domain.ServiceAccount{TenantID: "t1", Name: nul}),
		"update nil sa":          s.UpdateServiceAccount(ctx, nil),
		"update sa no id":        s.UpdateServiceAccount(ctx, &domain.ServiceAccount{TenantID: "t1", Name: "x"}),
		"token both owners":      s.CreateToken(ctx, &domain.APIToken{AccountID: "a", ServiceAccountID: "s", TokenHash: "h"}),
		"token nul sa":           s.CreateToken(ctx, &domain.APIToken{ServiceAccountID: nul, TokenHash: "h"}),
		"nil mfa":                s.PutMFA(ctx, nil),
		"mfa no account":         s.PutMFA(ctx, &domain.MFAState{}),
		"nil invitation":         s.CreateInvitation(ctx, nil),
		"invitation no tenant":   s.CreateInvitation(ctx, &domain.Invitation{CodeHash: "c"}),
		"invitation no code":     s.CreateInvitation(ctx, &domain.Invitation{TenantID: "t1"}),
		"update nil invitation":  s.UpdateInvitation(ctx, nil),
		"update invitation noid": s.UpdateInvitation(ctx, &domain.Invitation{TenantID: "t1"}),
		"nil reset":              s.PutPasswordReset(ctx, nil),
		"reset no code":          s.PutPasswordReset(ctx, &domain.PasswordReset{AccountID: "u1"}),
		"reset no account":       s.PutPasswordReset(ctx, &domain.PasswordReset{CodeHash: "c"}),
		"nil sso config":         s.PutSSOConfig(ctx, nil),
		"nil sso identity":       s.PutSSOIdentity(ctx, nil),
		"sso identity no issuer": s.PutSSOIdentity(ctx, &domain.SSOIdentity{Subject: "s", AccountID: "u1"}),
		"sso identity no acct":   s.PutSSOIdentity(ctx, &domain.SSOIdentity{Issuer: "i", Subject: "s"}),
	}
	for name, err := range calls {
		if !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("%s: err = %v, want ErrInvalid", name, err)
		}
	}
	// Rejected arguments are not mutated and nothing was written.
	tm := &domain.Team{TenantID: "t1"}
	_ = s.CreateTeam(ctx, tm)
	if tm.ID != "" || !tm.CreatedAt.IsZero() {
		t.Fatalf("rejected team was mutated: %+v", tm)
	}
	m := &domain.TeamMember{TeamID: "missing", AccountID: "u1"}
	_ = s.PutTeamMember(ctx, m)
	if m.Role != "" || !m.CreatedAt.IsZero() {
		t.Fatalf("rejected team member was mutated: %+v", m)
	}
	g := &domain.Grant{TreeID: "missing", SubjectType: domain.SubjectUser, SubjectID: "u", Role: "viewer"}
	_ = s.PutGrant(ctx, g)
	if !g.CreatedAt.IsZero() {
		t.Fatalf("rejected grant was mutated: %+v", g)
	}
	if list, _ := s.ListTeams(ctx, "t1"); len(list) != 1 {
		t.Fatalf("invalid calls wrote teams: %+v", list)
	}
	if list, _ := s.ListRoles(ctx, "t1"); len(list) != 0 {
		t.Fatalf("invalid calls wrote roles: %+v", list)
	}
	if list, _ := s.ListGrants(ctx, tr.ID); len(list) != 0 {
		t.Fatalf("invalid calls wrote grants: %+v", list)
	}
	if _, err := s.GetSSOConfig(ctx); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("nil PutSSOConfig wrote a config: %v", err)
	}
}

func testIAMEmptyListsNonNil(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	lists := map[string]func() (any, error){
		"ListTeams":                func() (any, error) { return s.ListTeams(ctx, "t") },
		"ListTeamMembers":          func() (any, error) { return s.ListTeamMembers(ctx, "x") },
		"ListAccountTeams":         func() (any, error) { return s.ListAccountTeams(ctx, "a") },
		"ListRoles":                func() (any, error) { return s.ListRoles(ctx, "t") },
		"ListPolicies":             func() (any, error) { return s.ListPolicies(ctx, "t") },
		"ListGrants":               func() (any, error) { return s.ListGrants(ctx, "tr") },
		"ListSubjectGrants":        func() (any, error) { return s.ListSubjectGrants(ctx, domain.SubjectUser, "a") },
		"ListServiceAccounts":      func() (any, error) { return s.ListServiceAccounts(ctx, "t") },
		"ListServiceAccountTokens": func() (any, error) { return s.ListServiceAccountTokens(ctx, "sa") },
		"ListServiceAccountTok('')": func() (any, error) {
			return s.ListServiceAccountTokens(ctx, "")
		},
		"ListInvitations":          func() (any, error) { return s.ListInvitations(ctx, "t") },
		"ListAccountSSOIdentities": func() (any, error) { return s.ListAccountSSOIdentities(ctx, "a") },
	}
	for name, f := range lists {
		v, err := f()
		must(t, err)
		rv := reflect.ValueOf(v)
		if rv.IsNil() || rv.Len() != 0 {
			t.Errorf("%s = %#v, want empty non-nil slice", name, v)
		}
	}
}

func testIAMNoAliasing(t *testing.T, s domain.MetaStore) {
	ctx := context.Background()
	r := &domain.CustomRole{TenantID: "t1", Name: "r", Permissions: []domain.Action{domain.ActTreeRead}}
	must(t, s.PutRole(ctx, r))
	r.Permissions[0] = "MUTATED"
	gotR, _ := s.GetRole(ctx, "t1", "r")
	if gotR.Permissions[0] != domain.ActTreeRead {
		t.Fatal("PutRole aliases Permissions")
	}
	gotR.Permissions[0] = "MUTATED"
	roles, _ := s.ListRoles(ctx, "t1")
	if roles[0].Permissions[0] != domain.ActTreeRead {
		t.Fatal("GetRole returns aliased Permissions")
	}

	p := samplePolicy()
	must(t, s.CreatePolicy(ctx, p))
	p.Statements[0].Actions[0] = "MUTATED"
	*p.Statements[0].Conditions.NotBefore = at(9999)
	p.Statements[0].Conditions.TimeWindow.Days[0] = "MUTATED"
	p.Statements[0].Conditions.IPRanges[0] = "MUTATED"
	gotP, _ := s.GetPolicy(ctx, "t1", p.ID)
	st := gotP.Statements[0]
	if st.Actions[0] != string(domain.ActBranchPush) || !st.Conditions.NotBefore.Equal(at(100)) ||
		st.Conditions.TimeWindow.Days[0] != "sat" || st.Conditions.IPRanges[0] != "10.0.0.0/8" {
		t.Fatalf("CreatePolicy aliases statements: %+v", st)
	}
	gotP.Statements[0].Principals[0] = "MUTATED"
	gotP.Statements[0].Conditions.TimeWindow.Start = "MUTATED"
	pols, _ := s.ListPolicies(ctx, "t1")
	if pols[0].Statements[0].Principals[0] != "*" || pols[0].Statements[0].Conditions.TimeWindow.Start != "00:00" {
		t.Fatal("GetPolicy returns aliased statements")
	}

	enabled := at(1)
	m := &domain.MFAState{AccountID: "u1", RecoveryCodeHashes: []string{"h1"}, EnabledAt: &enabled}
	must(t, s.PutMFA(ctx, m))
	m.RecoveryCodeHashes[0] = "MUTATED"
	*m.EnabledAt = at(2)
	gotM, _ := s.GetMFA(ctx, "u1")
	if gotM.RecoveryCodeHashes[0] != "h1" || !gotM.EnabledAt.Equal(at(1)) {
		t.Fatal("PutMFA aliases its argument")
	}
	gotM.RecoveryCodeHashes[0] = "MUTATED"
	gotM, _ = s.GetMFA(ctx, "u1")
	if gotM.RecoveryCodeHashes[0] != "h1" {
		t.Fatal("GetMFA returns aliased hashes")
	}

	c := &domain.SSOConfig{Scopes: []string{"openid"}, AllowedDomains: []string{"a.com"}}
	must(t, s.PutSSOConfig(ctx, c))
	c.Scopes[0] = "MUTATED"
	gotC, _ := s.GetSSOConfig(ctx)
	if gotC.Scopes[0] != "openid" {
		t.Fatal("PutSSOConfig aliases Scopes")
	}
	gotC.AllowedDomains[0] = "MUTATED"
	gotC, _ = s.GetSSOConfig(ctx)
	if gotC.AllowedDomains[0] != "a.com" {
		t.Fatal("GetSSOConfig returns aliased AllowedDomains")
	}

	tok := &domain.APIToken{AccountID: "u1", TokenHash: "h", Scopes: []string{"tree:read"}}
	must(t, s.CreateToken(ctx, tok))
	tok.Scopes[0] = "MUTATED"
	gotT, _ := s.GetTokenByHash(ctx, "h")
	if gotT.Scopes[0] != "tree:read" {
		t.Fatal("CreateToken aliases Scopes")
	}
	gotT.Scopes[0] = "MUTATED"
	toks, _ := s.ListTokens(ctx, "u1")
	if toks[0].Scopes[0] != "tree:read" {
		t.Fatal("GetTokenByHash returns aliased Scopes")
	}

	accepted := at(3)
	inv := &domain.Invitation{TenantID: "t1", CodeHash: "c", AcceptedAt: &accepted}
	must(t, s.CreateInvitation(ctx, inv))
	*inv.AcceptedAt = at(4)
	gotI, _ := s.GetInvitation(ctx, "t1", inv.ID)
	if !gotI.AcceptedAt.Equal(at(3)) {
		t.Fatal("CreateInvitation aliases AcceptedAt")
	}
	*gotI.AcceptedAt = at(4)
	gotI, _ = s.GetInvitationByCodeHash(ctx, "c")
	if !gotI.AcceptedAt.Equal(at(3)) {
		t.Fatal("GetInvitation returns aliased AcceptedAt")
	}
}

func testIAMCanceledAndClosed(t *testing.T, s domain.MetaStore) {
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	wantErr(t, s.CreateTeam(ctx, &domain.Team{TenantID: "t1", Slug: "x"}), context.Canceled)
	_, err := s.ListPolicies(ctx, "t1")
	wantErr(t, err, context.Canceled)
	_, err = s.GetSSOConfig(ctx)
	wantErr(t, err, context.Canceled)
	if list, err := s.ListTeams(context.Background(), "t1"); err != nil || len(list) != 0 {
		t.Fatalf("canceled create wrote data: %v %v", list, err)
	}

	bg := context.Background()
	must(t, s.Close())
	wantErr(t, s.CreateServiceAccount(bg, &domain.ServiceAccount{TenantID: "t1", Name: "x"}), ErrClosed)
	_, err = s.GetMFA(bg, "u1")
	wantErr(t, err, ErrClosed)
	_, err = s.ListGrants(bg, "tr")
	wantErr(t, err, ErrClosed)
	wantErr(t, s.PutSSOConfig(bg, &domain.SSOConfig{}), ErrClosed)
}
