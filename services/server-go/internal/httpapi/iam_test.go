package httpapi

import (
	"context"
	"encoding/json"
	"net/http/httptest"
	"slices"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// newIAMFixture extends the shared fixture with the IAM fakes and seeds:
//
//	maria: maintainer of acme
//	team acme/backend: bob maintainer, carol member; grant developer on web
//	custom role acme/auditor [tree:read]
//	service account acme/ci (developer)
//	policy "baseline" (id pol-1)
func newIAMFixture(t testing.TB, mutate ...func(*Deps)) *fixture {
	t.Helper()
	f := newFixture(t, append([]func(*Deps){func(d *Deps) {
		st := newIAMStore(d.Store.(*fakeStore))
		d.Store = st
		d.Authz = &iamAuthz{fakeAuthz: d.Authz.(*fakeAuthz), store: st}
		d.Security = newIAMSecurity(st, d.Auth.(*fakeAuth))
		d.Access = &iamAccess{}
	}}, mutate...)...)
	ctx := context.Background()
	st := f.iamStore()

	maria := domain.Account{ID: "acct-maria", Username: "maria", Email: "maria@example.com", TenantID: "tenant-acme", CreatedAt: t0}
	must(t, st.CreateAccount(ctx, &maria))
	must(t, st.PutMembership(ctx, &domain.Membership{TenantID: f.acme.ID, AccountID: maria.ID, Role: domain.RoleMaintainer, CreatedAt: t0}))
	f.users["maria"] = &domain.Principal{AccountID: maria.ID, Username: "maria", TenantID: "tenant-acme"}
	f.auth.passwords["maria"] = "maria-password"

	team := domain.Team{ID: "team-backend", TenantID: f.acme.ID, Slug: "backend", Name: "Backend", CreatedAt: t0, UpdatedAt: t0}
	must(t, st.CreateTeam(ctx, &team))
	must(t, st.PutTeamMember(ctx, &domain.TeamMember{TeamID: team.ID, AccountID: "acct-bob", Role: domain.TeamRoleMaintainer, CreatedAt: t0}))
	must(t, st.PutTeamMember(ctx, &domain.TeamMember{TeamID: team.ID, AccountID: "acct-carol", Role: domain.TeamRoleMember, CreatedAt: t0}))
	must(t, st.PutGrant(ctx, &domain.Grant{TreeID: f.web.ID, SubjectType: domain.SubjectTeam, SubjectID: team.ID, Subject: "backend", Role: domain.RoleDeveloper, CreatedAt: t0}))
	must(t, st.PutRole(ctx, &domain.CustomRole{TenantID: f.acme.ID, Name: "auditor", Permissions: []domain.Action{domain.ActTreeRead}, CreatedAt: t0, UpdatedAt: t0}))
	must(t, st.CreateServiceAccount(ctx, &domain.ServiceAccount{ID: "sa-ci", TenantID: f.acme.ID, Name: "ci", Role: domain.RoleDeveloper, CreatedAt: t0, UpdatedAt: t0}))
	must(t, st.CreatePolicy(ctx, &domain.Policy{ID: "pol-1", TenantID: f.acme.ID, Name: "baseline", Enabled: true, Version: 3,
		Statements: []domain.PolicyStatement{validStatement()}, CreatedBy: "alice", CreatedAt: t0, UpdatedAt: t0}))
	return f
}

func (f *fixture) iamStore() *iamStore  { return f.deps.Store.(*iamStore) }
func (f *fixture) iamSec() *iamSecurity { return f.deps.Security.(*iamSecurity) }
func (f *fixture) iamAcc() *iamAccess   { return f.deps.Access.(*iamAccess) }
func noAccess(d *Deps)                  { d.Access = nil }
func noSecurity(d *Deps)                { d.Security = nil }
func validStatement() domain.PolicyStatement {
	return domain.PolicyStatement{Effect: "deny", Actions: []string{"branch:push"}, Resources: []string{"wt:branch/acme/web/main"}, Principals: []string{"role:developer"}}
}

// addServiceAccountToken makes raw authenticate as service account acme/ci.
func addServiceAccountToken(f *fixture, raw string) {
	f.tokens[raw] = &domain.Principal{Username: "sa:acme/ci", TenantID: f.acme.ID, Kind: domain.PrincipalServiceAccount, ServiceAccountID: "sa-ci"}
}

// addScopedToken makes raw authenticate as username restricted to scopes.
func addScopedToken(f *fixture, username, raw string, scopes ...string) {
	p := *f.users[username]
	p.Scopes = scopes
	f.tokens[raw] = &p
}

func runIAMCases(t *testing.T, cases []endpointCase, mutate ...func(*Deps)) {
	t.Helper()
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			t.Parallel()
			f := newIAMFixture(t, mutate...)
			if tc.setup != nil {
				tc.setup(t, f)
			}
			rec := f.do(t, tc.method, tc.path, tc.as, tc.body)
			if rec.Code != tc.status {
				t.Fatalf("%s %s: status = %d, want %d; body: %s", tc.method, tc.path, rec.Code, tc.status, rec.Body.String())
			}
			if tc.code != "" {
				if got := errCode(rec); got != tc.code {
					t.Fatalf("error code = %q, want %q; body: %s", got, tc.code, rec.Body.String())
				}
			}
			if rec.Code >= 400 {
				if ct := rec.Header().Get("Content-Type"); !strings.HasPrefix(ct, "application/json") || errCode(rec) == "" {
					t.Fatalf("error response without JSON envelope (%s): %s", ct, rec.Body.String())
				}
				// ops (§13.3): 403s are audited as access.denied by design.
				if n := len(nonDenialActions(f)); n > 0 && !strings.Contains(strings.Join(nonDenialActions(f), ","), "failed") {
					t.Fatalf("failed request was audited: %v", f.audit.actions())
				}
			}
			if tc.check != nil {
				tc.check(t, f, rec)
			}
		})
	}
}

// auditDetail returns the details of the last audit event with action.
func auditDetail(t *testing.T, f *fixture, action string) domain.AuditEvent {
	t.Helper()
	f.audit.mu.Lock()
	defer f.audit.mu.Unlock()
	for i := len(f.audit.events) - 1; i >= 0; i-- {
		if f.audit.events[i].Action == action {
			return f.audit.events[i]
		}
	}
	t.Fatalf("audit %q not recorded", action)
	return domain.AuditEvent{}
}

func audited(action string) func(*testing.T, *fixture, *httptest.ResponseRecorder) {
	return func(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) { t.Helper(); hasAudit(t, f, action) }
}

func TestIAMRoutesRegistered(t *testing.T) {
	f := newIAMFixture(t)
	// A method the mux does not know for a registered path answers 405
	// with the JSON envelope, proving the path is routed.
	for _, path := range []string{
		"/api/v1/tenants/acme/teams", "/api/v1/tenants/acme/roles", "/api/v1/tenants/acme/policies",
		"/api/v1/tenants/acme/service-accounts", "/api/v1/tenants/acme/invitations",
		"/api/v1/trees/acme/web/grants", "/api/v1/auth/sessions",
	} {
		if rec := f.do(t, "PATCH", path, "alice", map[string]string{}); rec.Code != 405 {
			t.Errorf("PATCH %s = %d, want 405", path, rec.Code)
		}
	}
}

func TestIAMActions(t *testing.T) {
	runIAMCases(t, []endpointCase{{
		name: "public catalog", method: "GET", path: "/api/v1/iam/actions", status: 200,
		check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
			acts := decode[[]ActionInfo](t, rec)
			idx := slices.IndexFunc(acts, func(a ActionInfo) bool { return a.Action == domain.ActIAMAdmin })
			if idx < 0 || acts[idx].Area != "iam" || acts[idx].Description == "" {
				t.Fatalf("iam:admin missing or incomplete: %+v", acts)
			}
			if !slices.ContainsFunc(acts, func(a ActionInfo) bool { return a.Action == domain.ActBranchPush }) {
				t.Fatalf("branch:push missing")
			}
		},
	}})
}

func TestIAMTeams(t *testing.T) {
	const teams = "/api/v1/tenants/acme/teams"
	runIAMCases(t, []endpointCase{
		{name: "list anonymous", method: "GET", path: teams, status: 401, code: "unauthenticated"},
		{name: "list non-member", method: "GET", path: teams, as: "dave", status: 403, code: "forbidden"},
		{name: "list viewer", method: "GET", path: teams, as: "carol", status: 403, code: "forbidden"},
		{name: "list unknown tenant", method: "GET", path: "/api/v1/tenants/nope/teams", as: "alice", status: 404},
		{name: "list maintainer", method: "GET", path: teams, as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				ts := decode[[]domain.Team](t, rec)
				if len(ts) != 1 || ts[0].Slug != "backend" {
					t.Fatalf("teams = %+v", ts)
				}
			}},
		{name: "create", method: "POST", path: teams, as: "alice", body: map[string]string{"slug": "frontend", "description": "UI"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				tm := decode[domain.Team](t, rec)
				if tm.Slug != "frontend" || tm.Name != "frontend" || tm.TenantID != f.acme.ID || tm.ID == "" {
					t.Fatalf("team = %+v", tm)
				}
				if e := auditDetail(t, f, "team.create"); e.TenantID != f.acme.ID || e.Actor != "alice" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "create by server admin", method: "POST", path: teams, as: "admin", body: map[string]string{"slug": "ops"}, status: 201},
		{name: "create bad slug", method: "POST", path: teams, as: "alice", body: map[string]string{"slug": "Front End"}, status: 400, code: "bad_request"},
		{name: "create one char slug", method: "POST", path: teams, as: "alice", body: map[string]string{"slug": "a"}, status: 400},
		{name: "create long slug", method: "POST", path: teams, as: "alice", body: map[string]string{"slug": strings.Repeat("a", 41)}, status: 400},
		{name: "create duplicate", method: "POST", path: teams, as: "alice", body: map[string]string{"slug": "backend"}, status: 409, code: "conflict"},
		{name: "create maintainer forbidden", method: "POST", path: teams, as: "maria", body: map[string]string{"slug": "x1"}, status: 403},
		{name: "create unknown field", method: "POST", path: teams, as: "alice", body: `{"slug":"x1","owner":"me"}`, status: 400},
		{name: "get as team member", method: "GET", path: teams + "/backend", as: "carol", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				d := decode[teamDetail](t, rec)
				if d.Team.Slug != "backend" || len(d.Members) != 2 || d.Members[0].Account.Username != "bob" ||
					d.Members[0].Role != domain.TeamRoleMaintainer || len(d.Grants) != 1 || d.Grants[0].TreeID != f.web.ID {
					t.Fatalf("detail = %+v", d)
				}
				if strings.Contains(rec.Body.String(), "password") {
					t.Fatalf("leaks password: %s", rec.Body.String())
				}
			}},
		{name: "get non-member", method: "GET", path: teams + "/backend", as: "dave", status: 403},
		{name: "get unknown as admin", method: "GET", path: teams + "/nope", as: "alice", status: 404},
		{name: "get unknown hides existence", method: "GET", path: teams + "/nope", as: "dave", status: 403},
		{name: "patch", method: "PATCH", path: teams + "/backend", as: "mallory", body: map[string]string{"name": "Back end", "description": "APIs"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				tm := decode[domain.Team](t, rec)
				if tm.Name != "Back end" || tm.Description != "APIs" || tm.Slug != "backend" {
					t.Fatalf("team = %+v", tm)
				}
				hasAudit(t, f, "team.update")
			}},
		{name: "patch empty name", method: "PATCH", path: teams + "/backend", as: "alice", body: map[string]string{"name": " "}, status: 400},
		{name: "patch team maintainer forbidden", method: "PATCH", path: teams + "/backend", as: "bob", body: map[string]string{"name": "x"}, status: 403},
		{name: "delete", method: "DELETE", path: teams + "/backend", as: "alice", status: 204,
			check: func(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) {
				hasAudit(t, f, "team.delete")
				if len(f.iamStore().teams) != 0 || len(f.iamStore().teamMembers) != 0 {
					t.Fatalf("team not deleted")
				}
			}},
		{name: "delete unknown", method: "DELETE", path: teams + "/nope", as: "alice", status: 404},
		{name: "member add by team maintainer", method: "PUT", path: teams + "/backend/members/mallory", as: "bob", body: map[string]string{}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				m := decode[teamMemberView](t, rec)
				if m.Account.Username != "mallory" || m.Role != domain.TeamRoleMember {
					t.Fatalf("member = %+v", m)
				}
				if e := auditDetail(t, f, "team.member.set"); e.Details["username"] != "mallory" || e.Details["team"] != "backend" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "member promote by admin", method: "PUT", path: teams + "/backend/members/carol", as: "alice", body: map[string]string{"role": "maintainer"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if e := auditDetail(t, f, "team.member.set"); e.Details["previous_role"] != "member" || e.Details["role"] != "maintainer" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "member add by plain member", method: "PUT", path: teams + "/backend/members/mallory", as: "carol", body: map[string]string{}, status: 403},
		{name: "member add non tenant member", method: "PUT", path: teams + "/backend/members/dave", as: "alice", body: map[string]string{}, status: 400},
		{name: "member bad role", method: "PUT", path: teams + "/backend/members/mallory", as: "alice", body: map[string]string{"role": "owner"}, status: 400},
		{name: "member unknown account", method: "PUT", path: teams + "/backend/members/zed", as: "alice", body: map[string]string{}, status: 404},
		{name: "member self leave", method: "DELETE", path: teams + "/backend/members/carol", as: "carol", status: 204, check: audited("team.member.remove")},
		{name: "member removed by maintainer", method: "DELETE", path: teams + "/backend/members/carol", as: "bob", status: 204},
		{name: "member remove by member", method: "DELETE", path: teams + "/backend/members/bob", as: "carol", status: 403},
		{name: "member remove non-member", method: "DELETE", path: teams + "/backend/members/mallory", as: "alice", status: 404},
	})
}

func TestIAMRoles(t *testing.T) {
	const roles = "/api/v1/tenants/acme/roles"
	runIAMCases(t, []endpointCase{
		{name: "list", method: "GET", path: roles, as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				rs := decode[[]roleView](t, rec)
				names := []string{}
				for _, r := range rs {
					names = append(names, r.Name)
				}
				if strings.Join(names, ",") != "owner,admin,maintainer,developer,viewer,auditor" {
					t.Fatalf("roles = %v", names)
				}
				if !rs[0].Builtin || rs[5].Builtin {
					t.Fatalf("builtin flags wrong: %+v", rs)
				}
				if !slices.Contains(rs[2].Permissions, domain.ActIAMRead) || slices.Contains(rs[2].Permissions, domain.ActIAMAdmin) {
					t.Fatalf("maintainer perms = %v", rs[2].Permissions)
				}
				if !slices.Contains(rs[1].Permissions, domain.ActIAMAdmin) || slices.Contains(rs[1].Permissions, domain.ActServerAdmin) {
					t.Fatalf("admin perms = %v", rs[1].Permissions)
				}
				if !slices.Equal(rs[4].Permissions, []domain.Action{domain.ActTenantRead, domain.ActTreeRead, domain.ActTreeFork}) {
					t.Fatalf("viewer perms = %v", rs[4].Permissions)
				}
			}},
		{name: "list viewer forbidden", method: "GET", path: roles, as: "carol", status: 403},
		{name: "create", method: "POST", path: roles, as: "alice",
			body: map[string]any{"name": "release-manager", "description": "ships", "permissions": []string{"mr:*", "tree:read", "tree:read"}}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[roleView](t, rec)
				if r.Name != "release-manager" || r.Builtin || !slices.Equal(r.Permissions, []domain.Action{"mr:*", "tree:read"}) {
					t.Fatalf("role = %+v", r)
				}
				if _, err := f.iamStore().GetRole(context.Background(), f.acme.ID, "release-manager"); err != nil {
					t.Fatal(err)
				}
				hasAudit(t, f, "role.create")
			}},
		{name: "create builtin name", method: "POST", path: roles, as: "alice", body: map[string]any{"name": "admin", "permissions": []string{}}, status: 409},
		{name: "create duplicate", method: "POST", path: roles, as: "alice", body: map[string]any{"name": "auditor", "permissions": []string{"tree:read"}}, status: 409},
		{name: "create unknown action", method: "POST", path: roles, as: "alice", body: map[string]any{"name": "x1", "permissions": []string{"tree:fly"}}, status: 400},
		{name: "create unknown area", method: "POST", path: roles, as: "alice", body: map[string]any{"name": "x1", "permissions": []string{"foo:*"}}, status: 400},
		{name: "create star", method: "POST", path: roles, as: "alice", body: map[string]any{"name": "x1", "permissions": []string{"*"}}, status: 400},
		{name: "create server admin", method: "POST", path: roles, as: "alice", body: map[string]any{"name": "x1", "permissions": []string{"server:admin"}}, status: 400},
		{name: "create bad name", method: "POST", path: roles, as: "alice", body: map[string]any{"name": "Bad_Name", "permissions": []string{}}, status: 400},
		{name: "create maintainer forbidden", method: "POST", path: roles, as: "maria", body: map[string]any{"name": "x1", "permissions": []string{}}, status: 403},
		{name: "get builtin", method: "GET", path: roles + "/viewer", as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[roleView](t, rec); !r.Builtin || r.Name != "viewer" {
					t.Fatalf("role = %+v", r)
				}
			}},
		{name: "get custom", method: "GET", path: roles + "/auditor", as: "maria", status: 200},
		{name: "get missing", method: "GET", path: roles + "/nope", as: "maria", status: 404},
		{name: "patch", method: "PATCH", path: roles + "/auditor", as: "alice", body: map[string]any{"permissions": []string{"tree:read", "mr:approve"}}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if r := decode[roleView](t, rec); !slices.Equal(r.Permissions, []domain.Action{"mr:approve", "tree:read"}) {
					t.Fatalf("role = %+v", r)
				}
				if e := auditDetail(t, f, "role.update"); e.Details["previous_permissions"] != "tree:read" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "patch invalid permission", method: "PATCH", path: roles + "/auditor", as: "alice", body: map[string]any{"permissions": []string{"nope"}}, status: 400},
		{name: "patch builtin", method: "PATCH", path: roles + "/admin", as: "alice", body: map[string]any{"description": "x"}, status: 400},
		{name: "patch missing", method: "PATCH", path: roles + "/nope", as: "alice", body: map[string]any{"description": "x"}, status: 404},
		{name: "delete unused", method: "DELETE", path: roles + "/auditor", as: "alice", status: 204, check: audited("role.delete")},
		{name: "delete builtin", method: "DELETE", path: roles + "/viewer", as: "alice", status: 400},
		{name: "delete assigned to member", method: "DELETE", path: roles + "/auditor", as: "alice", status: 409, code: "conflict",
			setup: func(t *testing.T, f *fixture) {
				must(t, f.store.PutMembership(context.Background(), &domain.Membership{TenantID: f.acme.ID, AccountID: "acct-dave", Role: "auditor"}))
			}},
		{name: "delete assigned to grant", method: "DELETE", path: roles + "/auditor", as: "alice", status: 409,
			setup: func(t *testing.T, f *fixture) {
				must(t, f.iamStore().PutGrant(context.Background(), &domain.Grant{TreeID: f.docs.ID, SubjectType: "user", SubjectID: "acct-dave", Role: "auditor"}))
			}},
		{name: "delete assigned to service account", method: "DELETE", path: roles + "/auditor", as: "alice", status: 409,
			setup: func(t *testing.T, f *fixture) {
				must(t, f.iamStore().CreateServiceAccount(context.Background(), &domain.ServiceAccount{TenantID: f.acme.ID, Name: "bot", Role: "auditor"}))
			}},
	})
}

func TestIAMPolicies(t *testing.T) {
	const pols = "/api/v1/tenants/acme/policies"
	good := map[string]any{"name": "office-hours", "description": "deny pushes at night", "statements": []any{
		map[string]any{"sid": "night", "effect": "deny", "actions": []string{"tree:write", "branch:*"}, "resources": []string{"wt:tree/acme/*", "wt:path/acme/web/**"},
			"principals": []string{"authenticated"}, "conditions": map[string]any{"ip_ranges": []string{"10.0.0.0/8"}, "require_mfa": true,
				"time_window": map[string]any{"days": []string{"mon", "fri"}, "start": "18:00", "end": "08:00", "timezone": "Europe/Berlin"}}},
	}}
	bad := map[string]any{"statements": []any{
		map[string]any{"effect": "maybe", "actions": []string{"tree:fly"}, "resources": []string{"wt:tree/acme"}, "principals": []string{"bob"},
			"conditions": map[string]any{"ip_ranges": []string{"nope"}, "time_window": map[string]any{"start": "25:00", "end": "08:00", "timezone": "Mars/Base"}}},
		map[string]any{"effect": "allow", "actions": []string{}, "resources": []string{"*"}, "principals": []string{"*"}},
	}}
	runIAMCases(t, []endpointCase{
		{name: "list", method: "GET", path: pols, as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if ps := decode[[]domain.Policy](t, rec); len(ps) != 1 || ps[0].ID != "pol-1" {
					t.Fatalf("policies = %+v", ps)
				}
			}},
		{name: "list viewer", method: "GET", path: pols, as: "carol", status: 403},
		{name: "create", method: "POST", path: pols, as: "alice", body: good, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				p := decode[domain.Policy](t, rec)
				if p.ID == "" || p.Version != 1 || !p.Enabled || p.CreatedBy != "alice" || len(p.Statements) != 1 ||
					p.Statements[0].Conditions.TimeWindow.Timezone != "Europe/Berlin" {
					t.Fatalf("policy = %+v", p)
				}
				hasAudit(t, f, "policy.create")
			}},
		{name: "create disabled", method: "POST", path: pols, as: "alice", body: map[string]any{"name": "off", "enabled": false, "statements": []any{}}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if p := decode[domain.Policy](t, rec); p.Enabled {
					t.Fatalf("policy enabled: %+v", p)
				}
			}},
		{name: "create invalid", method: "POST", path: pols, as: "alice", body: map[string]any{"name": "x", "statements": bad["statements"]}, status: 400, code: "bad_request"},
		{name: "create duplicate name", method: "POST", path: pols, as: "alice", body: map[string]any{"name": "Baseline", "statements": []any{}}, status: 409},
		{name: "create missing name", method: "POST", path: pols, as: "alice", body: map[string]any{"statements": []any{}}, status: 400},
		{name: "create maintainer", method: "POST", path: pols, as: "maria", body: good, status: 403},
		{name: "validate good", method: "POST", path: pols + "/validate", as: "maria", body: map[string]any{"statements": good["statements"]}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				v := decode[validatePolicyResponse](t, rec)
				if !v.Valid || v.Errors == nil || len(v.Errors) != 0 {
					t.Fatalf("validate = %s", rec.Body.String())
				}
			}},
		{name: "validate bad", method: "POST", path: pols + "/validate", as: "maria", body: bad, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				v := decode[validatePolicyResponse](t, rec)
				if v.Valid {
					t.Fatalf("validate = %s", rec.Body.String())
				}
				got := map[string]bool{}
				for _, e := range v.Errors {
					got[strings.Join([]string{string(rune('0' + e.Statement)), e.Field}, ":")] = true
					if e.Message == "" {
						t.Fatalf("empty message: %+v", e)
					}
				}
				for _, want := range []string{"0:effect", "0:actions[0]", "0:resources[0]", "0:principals[0]", "0:conditions.ip_ranges[0]",
					"0:conditions.time_window.start", "0:conditions.time_window.timezone", "1:actions"} {
					if !got[want] {
						t.Errorf("missing error %s; have %v", want, got)
					}
				}
			}},
		{name: "validate unknown statement field", method: "POST", path: pols + "/validate", as: "maria",
			body: `{"statements":[{"effect":"allow","actions":["*"],"resources":["*"],"principals":["*"],"bogus":1}]}`, status: 400},
		{name: "validate viewer", method: "POST", path: pols + "/validate", as: "carol", body: map[string]any{"statements": []any{}}, status: 403},
		{name: "get", method: "GET", path: pols + "/pol-1", as: "maria", status: 200},
		{name: "get missing", method: "GET", path: pols + "/nope", as: "maria", status: 404},
		{name: "put replaces and bumps version", method: "PUT", path: pols + "/pol-1", as: "alice",
			body: map[string]any{"id": "ignored", "name": "baseline", "enabled": false, "version": 3, "statements": good["statements"]}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				p := decode[domain.Policy](t, rec)
				if p.ID != "pol-1" || p.Version != 4 || p.Enabled || p.CreatedBy != "alice" || !p.CreatedAt.Equal(t0) || p.Statements[0].Sid != "night" {
					t.Fatalf("policy = %+v", p)
				}
				if e := auditDetail(t, f, "policy.update"); e.Details["version"] != "4" || e.Details["enabled"] != "false" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "put stale version", method: "PUT", path: pols + "/pol-1", as: "alice", body: map[string]any{"name": "baseline", "version": 2, "statements": []any{}}, status: 409},
		{name: "put invalid", method: "PUT", path: pols + "/pol-1", as: "alice", body: map[string]any{"name": "baseline", "statements": bad["statements"]}, status: 400},
		{name: "put missing", method: "PUT", path: pols + "/nope", as: "alice", body: map[string]any{"name": "x", "statements": []any{}}, status: 404},
		{name: "put maintainer", method: "PUT", path: pols + "/pol-1", as: "maria", body: map[string]any{"name": "x", "statements": []any{}}, status: 403},
		{name: "delete", method: "DELETE", path: pols + "/pol-1", as: "alice", status: 204, check: audited("policy.delete")},
		{name: "delete missing", method: "DELETE", path: pols + "/nope", as: "alice", status: 404},
	})
}

// Validation is the authorization engine's own (authz.ValidateStatements),
// so field paths and messages are the engine's.
func TestIAMPolicyValidationUsesEngine(t *testing.T) {
	runIAMCases(t, []endpointCase{
		{name: "engine rejects an unknown resource kind", method: "POST", path: "/api/v1/tenants/acme/policies/validate", as: "alice",
			body: map[string]any{"statements": []any{
				map[string]any{"effect": "allow", "actions": []string{"tree:read"}, "resources": []string{"*"}, "principals": []string{"*"}},
				map[string]any{"effect": "allow", "actions": []string{"tree:read"}, "resources": []string{"wt:repo/x"}, "principals": []string{"*"}},
			}}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				v := decode[validatePolicyResponse](t, rec)
				if v.Valid || len(v.Errors) != 1 || v.Errors[0].Statement != 1 || !strings.HasPrefix(v.Errors[0].Field, "resources") {
					t.Fatalf("validate = %s", rec.Body.String())
				}
			}},
	})
}

func TestIAMGrants(t *testing.T) {
	const grants = "/api/v1/trees/acme/web/grants"
	runIAMCases(t, []endpointCase{
		{name: "list", method: "GET", path: grants, as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				gs := decode[[]domain.Grant](t, rec)
				if len(gs) != 1 || gs[0].Subject != "backend" || gs[0].SubjectType != "team" {
					t.Fatalf("grants = %+v", gs)
				}
			}},
		{name: "list anonymous", method: "GET", path: grants, status: 401},
		{name: "list viewer", method: "GET", path: grants, as: "carol", status: 403},
		{name: "list unknown tree", method: "GET", path: "/api/v1/trees/acme/nope/grants", as: "alice", status: 404},
		{name: "set user", method: "PUT", path: grants, as: "mallory", body: map[string]string{"subject_type": "user", "subject": "dave", "role": "viewer"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				g := decode[domain.Grant](t, rec)
				if g.SubjectID != "acct-dave" || g.Subject != "dave" || g.TreeID != f.web.ID || g.Role != "viewer" || g.CreatedBy != "mallory" {
					t.Fatalf("grant = %+v", g)
				}
				if e := auditDetail(t, f, "grant.set"); e.Details["subject"] != "dave" || e.Details["role"] != "viewer" || e.TenantID != f.acme.ID {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "set team replaces", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "team", "subject": "backend", "role": "maintainer"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				g := decode[domain.Grant](t, rec)
				if g.SubjectID != "team-backend" || !g.CreatedAt.Equal(t0) {
					t.Fatalf("grant = %+v", g)
				}
				if e := auditDetail(t, f, "grant.set"); e.Details["previous_role"] != "developer" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "set service account", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "service_account", "subject": "ci", "role": "auditor"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if g := decode[domain.Grant](t, rec); g.SubjectID != "sa-ci" || g.Role != "auditor" {
					t.Fatalf("grant = %+v", g)
				}
			}},
		{name: "set sa alias", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "sa", "subject": "ci", "role": "viewer"}, status: 200},
		{name: "set unknown user", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "user", "subject": "zed", "role": "viewer"}, status: 404},
		{name: "set unknown team", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "team", "subject": "zed", "role": "viewer"}, status: 404},
		{name: "set unknown role", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "user", "subject": "dave", "role": "wizard"}, status: 400},
		{name: "set bad subject type", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "group", "subject": "dave", "role": "viewer"}, status: 400},
		{name: "set missing subject", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "user", "role": "viewer"}, status: 400},
		{name: "set owner by admin", method: "PUT", path: grants, as: "mallory", body: map[string]string{"subject_type": "user", "subject": "dave", "role": "owner"}, status: 403},
		{name: "set owner by owner", method: "PUT", path: grants, as: "alice", body: map[string]string{"subject_type": "user", "subject": "dave", "role": "owner"}, status: 200},
		{name: "set maintainer forbidden", method: "PUT", path: grants, as: "maria", body: map[string]string{"subject_type": "user", "subject": "dave", "role": "viewer"}, status: 403},
		{name: "remove", method: "DELETE", path: grants + "?subject_type=team&subject=backend", as: "alice", status: 204,
			check: func(t *testing.T, f *fixture, _ *httptest.ResponseRecorder) {
				if e := auditDetail(t, f, "grant.remove"); e.Details["previous_role"] != "developer" {
					t.Fatalf("audit = %+v", e)
				}
				if len(f.iamStore().grants) != 0 {
					t.Fatalf("grant not removed")
				}
			}},
		{name: "remove missing", method: "DELETE", path: grants + "?subject_type=user&subject=dave", as: "alice", status: 404},
		{name: "remove no subject", method: "DELETE", path: grants + "?subject_type=user", as: "alice", status: 400},
		{name: "remove forbidden", method: "DELETE", path: grants + "?subject_type=team&subject=backend", as: "bob", status: 403},
	})
}

func TestIAMServiceAccounts(t *testing.T) {
	const sas = "/api/v1/tenants/acme/service-accounts"
	runIAMCases(t, []endpointCase{
		{name: "list", method: "GET", path: sas, as: "maria", status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				if l := decode[[]domain.ServiceAccount](t, rec); len(l) != 1 || l[0].Name != "ci" {
					t.Fatalf("sas = %+v", l)
				}
			}},
		{name: "list viewer", method: "GET", path: sas, as: "carol", status: 403},
		{name: "create", method: "POST", path: sas, as: "alice", body: map[string]string{"name": "deploy-bot", "role": "auditor", "description": "CD"}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				sa := decode[domain.ServiceAccount](t, rec)
				if sa.ID == "" || sa.Role != "auditor" || sa.CreatedBy != "alice" || sa.TenantID != f.acme.ID {
					t.Fatalf("sa = %+v", sa)
				}
				hasAudit(t, f, "sa.create")
			}},
		{name: "create duplicate", method: "POST", path: sas, as: "alice", body: map[string]string{"name": "ci", "role": "viewer"}, status: 409},
		{name: "create unknown role", method: "POST", path: sas, as: "alice", body: map[string]string{"name": "x1", "role": "wizard"}, status: 400},
		{name: "create missing role", method: "POST", path: sas, as: "alice", body: map[string]string{"name": "x1"}, status: 400},
		{name: "create owner", method: "POST", path: sas, as: "alice", body: map[string]string{"name": "x1", "role": "owner"}, status: 400},
		{name: "create bad name", method: "POST", path: sas, as: "alice", body: map[string]string{"name": "-x", "role": "viewer"}, status: 400},
		{name: "create maintainer", method: "POST", path: sas, as: "maria", body: map[string]string{"name": "x1", "role": "viewer"}, status: 403},
		{name: "get with keys", method: "GET", path: sas + "/ci", as: "maria", status: 200,
			setup: func(t *testing.T, f *fixture) {
				_, _, err := f.iamSec().CreateServiceAccountKey(context.Background(), "sa-ci", "k1", 0, []string{"tree:read"})
				must(t, err)
			},
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				d := decode[serviceAccountDetail](t, rec)
				if d.ServiceAccount.Name != "ci" || len(d.Keys) != 1 || d.Keys[0].Name != "k1" {
					t.Fatalf("detail = %+v", d)
				}
				if strings.Contains(rec.Body.String(), "token_hash") || strings.Contains(rec.Body.String(), "wt_") {
					t.Fatalf("leaks secrets: %s", rec.Body.String())
				}
			}},
		{name: "get missing", method: "GET", path: sas + "/nope", as: "maria", status: 404},
		{name: "patch", method: "PATCH", path: sas + "/ci", as: "alice", body: map[string]any{"disabled": true, "role": "viewer", "description": "x"}, status: 200,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				sa := decode[domain.ServiceAccount](t, rec)
				if !sa.Disabled || sa.Role != "viewer" || sa.Description != "x" {
					t.Fatalf("sa = %+v", sa)
				}
				if e := auditDetail(t, f, "sa.update"); e.Details["previous_role"] != "developer" || e.Details["disabled"] != "true" {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "patch bad role", method: "PATCH", path: sas + "/ci", as: "alice", body: map[string]any{"role": "wizard"}, status: 400},
		{name: "delete", method: "DELETE", path: sas + "/ci", as: "alice", status: 204, check: audited("sa.delete")},
		{name: "delete missing", method: "DELETE", path: sas + "/nope", as: "alice", status: 404},
		{name: "key create", method: "POST", path: sas + "/ci/keys", as: "alice", body: map[string]any{"name": "deploy", "ttl_hours": 24, "scopes": []string{"tree:read", "tree:write"}}, status: 201,
			check: func(t *testing.T, f *fixture, rec *httptest.ResponseRecorder) {
				r := decode[createTokenResponse](t, rec)
				if !strings.HasPrefix(r.Token, "wt_") || r.APIToken.ServiceAccountID != "sa-ci" || r.APIToken.ExpiresAt == nil ||
					!slices.Equal(r.APIToken.Scopes, []string{"tree:read", "tree:write"}) {
					t.Fatalf("key = %s", rec.Body.String())
				}
				if e := auditDetail(t, f, "sa.key.create"); e.Details["key_id"] != r.APIToken.ID || strings.Contains(strings.Join(mapValues(e.Details), ""), r.Token) {
					t.Fatalf("audit = %+v", e)
				}
			}},
		{name: "key create default name", method: "POST", path: sas + "/ci/keys", as: "alice", body: map[string]any{}, status: 201},
		{name: "key bad scope", method: "POST", path: sas + "/ci/keys", as: "alice", body: map[string]any{"scopes": []string{"tree:fly"}}, status: 400},
		{name: "key bad ttl", method: "POST", path: sas + "/ci/keys", as: "alice", body: map[string]any{"ttl_hours": -1}, status: 400},
		{name: "key disabled account", method: "POST", path: sas + "/ci/keys", as: "alice", body: map[string]any{}, status: 409,
			setup: func(t *testing.T, f *fixture) {
				sa, _ := f.iamStore().GetServiceAccount(context.Background(), "sa-ci")
				sa.Disabled = true
				must(t, f.iamStore().UpdateServiceAccount(context.Background(), sa))
			}},
		{name: "key maintainer", method: "POST", path: sas + "/ci/keys", as: "maria", body: map[string]any{}, status: 403},
		{name: "key delete", method: "DELETE", path: sas + "/ci/keys/key-1", as: "alice", status: 204,
			setup: func(t *testing.T, f *fixture) {
				must(t, f.store.CreateToken(context.Background(), &domain.APIToken{ID: "key-1", ServiceAccountID: "sa-ci", Kind: domain.TokenService}))
			},
			check: audited("sa.key.delete")},
		{name: "key delete missing", method: "DELETE", path: sas + "/ci/keys/nope", as: "alice", status: 404},
		{name: "service account principal manages with admin role", method: "GET", path: sas, as: "sa-token", status: 200,
			setup: func(t *testing.T, f *fixture) {
				addServiceAccountToken(f, "sa-token")
				sa, _ := f.iamStore().GetServiceAccount(context.Background(), "sa-ci")
				sa.Role = domain.RoleAdmin
				must(t, f.iamStore().UpdateServiceAccount(context.Background(), sa))
			}},
	})
}

func TestIAMServiceAccountKeyWithoutSecurity(t *testing.T) {
	runIAMCases(t, []endpointCase{
		{name: "501", method: "POST", path: "/api/v1/tenants/acme/service-accounts/ci/keys", as: "alice", body: map[string]any{}, status: 501, code: "not_implemented"},
	}, noSecurity)
}

func mapValues(m map[string]string) []string {
	out := make([]string, 0, len(m))
	for _, v := range m {
		out = append(out, v)
	}
	return out
}

// jsonKeys returns the top-level keys of a JSON object response.
func jsonKeys(t *testing.T, rec *httptest.ResponseRecorder) []string {
	t.Helper()
	var m map[string]json.RawMessage
	must(t, json.Unmarshal(rec.Body.Bytes(), &m))
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	slices.Sort(keys)
	return keys
}
