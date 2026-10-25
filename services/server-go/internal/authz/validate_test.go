package authz

import (
	"context"
	"encoding/json"
	"net/netip"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestValidateStatements(t *testing.T) {
	past := time.Date(2020, 1, 1, 0, 0, 0, 0, time.UTC)
	future := past.Add(time.Hour)
	valid := func() domain.PolicyStatement {
		return stmt(domain.EffectDeny, strs("branch:push"), strs("wt:branch/acme/repo/release/**"), strs("role:developer"))
	}
	with := func(fn func(*domain.PolicyStatement)) domain.PolicyStatement {
		s := valid()
		fn(&s)
		return s
	}
	tests := []struct {
		name  string
		st    domain.PolicyStatement
		field string // "" = valid
		msg   string
	}{
		{"valid", valid(), "", ""},
		{"valid everything", domain.PolicyStatement{Sid: "all", Effect: "Allow", Actions: strs("*", "tree:*", "iam:admin"),
			Resources:  strs("*", "wt:tenant/acme", "wt:tree/acme/*", "wt:tree/**", "wt:path/acme/repo/**/*.md", "wt:branch/*/*/main"),
			Principals: strs("*", "authenticated", "anonymous", "user:al*", "team:core", "role:viewer", "sa:ci", "sa:acme/ci"),
			Conditions: domain.PolicyConditions{IPRanges: strs("10.0.0.0/8", "2001:db8::/32", "192.0.2.1"), RequireMFA: true,
				NotBefore: &past, NotAfter: &future,
				TimeWindow: &domain.TimeWindow{Days: strs("mon", "FRI"), Start: "22:00", End: "02:00", Timezone: "Asia/Tokyo"}}}, "", ""},
		{"missing effect", with(func(s *domain.PolicyStatement) { s.Effect = "" }), "effect", "required"},
		{"bad effect", with(func(s *domain.PolicyStatement) { s.Effect = "permit" }), "effect", "allow or deny"},
		{"no actions", with(func(s *domain.PolicyStatement) { s.Actions = nil }), "actions", "at least one"},
		{"unknown action", with(func(s *domain.PolicyStatement) { s.Actions = strs("tree:read", "tree:explode") }), "actions[1]", "unknown action"},
		{"unknown area", with(func(s *domain.PolicyStatement) { s.Actions = strs("repo:*") }), "actions[0]", "unknown action area"},
		{"no resources", with(func(s *domain.PolicyStatement) { s.Resources = nil }), "resources", "at least one"},
		{"resource without prefix", with(func(s *domain.PolicyStatement) { s.Resources = strs("tree/acme") }), "resources[0]", "must be"},
		{"unknown kind", with(func(s *domain.PolicyStatement) { s.Resources = strs("wt:repo/acme") }), "resources[0]", "unknown kind"},
		{"empty segment", with(func(s *domain.PolicyStatement) { s.Resources = strs("wt:tree/acme//x") }), "resources[0]", "empty segment"},
		{"no body", with(func(s *domain.PolicyStatement) { s.Resources = strs("wt:tree") }), "resources[0]", "no path"},
		{"tenant too deep", with(func(s *domain.PolicyStatement) { s.Resources = strs("wt:tenant/acme/x") }), "resources[0]", "at most 1"},
		{"tree too short", with(func(s *domain.PolicyStatement) { s.Resources = strs("wt:tree/acme") }), "resources[0]", "at least 2"},
		{"branch too short", with(func(s *domain.PolicyStatement) { s.Resources = strs("wt:branch/acme/repo") }), "resources[0]", "at least 3"},
		{"partial globstar", with(func(s *domain.PolicyStatement) { s.Resources = strs("wt:path/acme/repo/a**") }), "resources[0]", "whole segment"},
		{"no principals", with(func(s *domain.PolicyStatement) { s.Principals = nil }), "principals", "at least one"},
		{"unknown principal", with(func(s *domain.PolicyStatement) { s.Principals = strs("group:x") }), "principals[0]", "unknown principal"},
		{"empty principal name", with(func(s *domain.PolicyStatement) { s.Principals = strs("user:") }), "principals[0]", "no name"},
		{"slash in user", with(func(s *domain.PolicyStatement) { s.Principals = strs("user:a/b") }), "principals[0]", "must not contain"},
		{"bad sa", with(func(s *domain.PolicyStatement) { s.Principals = strs("sa:/ci") }), "principals[0]", "sa:<name>"},
		{"bad cidr", with(func(s *domain.PolicyStatement) { s.Conditions.IPRanges = strs("10.0.0.0/8", "10.0.0.0/99") }), "conditions.ip_ranges[1]", "invalid CIDR"},
		{"inverted dates", with(func(s *domain.PolicyStatement) { s.Conditions.NotBefore, s.Conditions.NotAfter = &future, &past }), "conditions.not_after", "after not_before"},
		{"bad day", with(func(s *domain.PolicyStatement) {
			s.Conditions.TimeWindow = &domain.TimeWindow{Days: strs("funday"), Start: "09:00", End: "10:00"}
		}), "conditions.time_window.days[0]", "unknown day"},
		{"bad start", with(func(s *domain.PolicyStatement) {
			s.Conditions.TimeWindow = &domain.TimeWindow{Start: "9am", End: "10:00"}
		}), "conditions.time_window.start", "HH:MM"},
		{"start 24:00", with(func(s *domain.PolicyStatement) {
			s.Conditions.TimeWindow = &domain.TimeWindow{Start: "24:00", End: "10:00"}
		}), "conditions.time_window.start", "24:00"},
		{"bad end", with(func(s *domain.PolicyStatement) {
			s.Conditions.TimeWindow = &domain.TimeWindow{Start: "09:00", End: "25:00"}
		}), "conditions.time_window.end", "between"},
		{"empty window", with(func(s *domain.PolicyStatement) {
			s.Conditions.TimeWindow = &domain.TimeWindow{Start: "09:00", End: "09:00"}
		}), "conditions.time_window.end", "differ"},
		{"bad tz", with(func(s *domain.PolicyStatement) {
			s.Conditions.TimeWindow = &domain.TimeWindow{Start: "09:00", End: "10:00", Timezone: "Mars/Olympus"}
		}), "conditions.time_window.timezone", "IANA"},
		{"long sid", with(func(s *domain.PolicyStatement) { s.Sid = strings.Repeat("x", 200) }), "sid", "at most"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			errs := ValidateStatements([]domain.PolicyStatement{valid(), tt.st})
			if tt.field == "" {
				if len(errs) != 0 {
					t.Fatalf("unexpected errors: %v", errs)
				}
				return
			}
			if len(errs) != 1 {
				t.Fatalf("errors = %v, want exactly one on %s", errs, tt.field)
			}
			e := errs[0]
			if e.Statement != 1 || e.Field != tt.field || !strings.Contains(e.Message, tt.msg) {
				t.Fatalf("error = %+v, want statement 1 field %q message ~%q", e, tt.field, tt.msg)
			}
			if !strings.Contains(e.Error(), "statement 1: "+tt.field) {
				t.Fatalf("Error() = %q", e.Error())
			}
		})
	}
}

func TestValidateStatementsMultipleErrors(t *testing.T) {
	errs := ValidateStatements([]domain.PolicyStatement{{}})
	fields := map[string]bool{}
	for _, e := range errs {
		fields[e.Field] = true
	}
	for _, f := range []string{"effect", "actions", "resources", "principals"} {
		if !fields[f] {
			t.Errorf("missing error for %s: %v", f, errs)
		}
	}
	if ValidateStatements(nil) != nil {
		t.Error("no statements: no errors")
	}
	b, _ := json.Marshal(errs[0])
	if !strings.Contains(string(b), `"statement":0`) || !strings.Contains(string(b), `"field":"effect"`) {
		t.Errorf("JSON = %s", b)
	}
}

// Every statement accepted by ValidateStatements compiles cleanly, so the
// evaluator never silently drops a validated statement.
func TestValidatedStatementsCompile(t *testing.T) {
	st := domain.PolicyStatement{Effect: "deny", Actions: strs("*"), Resources: strs("wt:tree/**", "*"), Principals: strs("*"),
		Conditions: domain.PolicyConditions{IPRanges: strs("::ffff:10.0.0.0/104"), TimeWindow: &domain.TimeWindow{Start: "00:00", End: "24:00"}}}
	if errs := ValidateStatements([]domain.PolicyStatement{st}); errs != nil {
		t.Fatal(errs)
	}
	cs := compileStatement(0, st)
	if cs.invalid != nil || cs.cond.err != nil {
		t.Fatalf("compile: %v / %v", cs.invalid, cs.cond.err)
	}
	if !cs.cond.ipRanges[0].Contains(mustAddr(t, "10.2.3.4")) {
		t.Fatal("v4-mapped CIDR should cover plain IPv4")
	}
}

func TestValidatePermissions(t *testing.T) {
	ok := [][]domain.Action{nil, {"tree:read"}, {"tree:*", "mr:merge", "iam:*", domain.ActBranchPush}}
	for _, p := range ok {
		if err := ValidatePermissions(p); err != nil {
			t.Errorf("ValidatePermissions(%v) = %v", p, err)
		}
	}
	bad := [][]domain.Action{{"*"}, {"server:admin"}, {"server:*"}, {"tree:explode"}, {""}, {"x:*"}}
	for _, p := range bad {
		if err := ValidatePermissions(p); err == nil {
			t.Errorf("ValidatePermissions(%v) = nil, want error", p)
		}
	}
}

func TestActionsCatalog(t *testing.T) {
	all := []domain.Action{
		domain.ActTreeRead, domain.ActTreeWrite, domain.ActTreeCreate, domain.ActTreeAdmin,
		domain.ActBranchCreate, domain.ActBranchDelete, domain.ActBranchPush,
		domain.ActMRCreate, domain.ActMRApprove, domain.ActMRMerge,
		domain.ActTenantRead, domain.ActTenantAdmin, domain.ActServerAdmin,
		domain.ActIAMRead, domain.ActIAMAdmin,
		domain.ActReleaseCreate, domain.ActReleaseDelete, domain.ActCIReport,
	}
	got := Actions()
	if len(got) != len(all) {
		t.Fatalf("catalog has %d actions, want %d", len(got), len(all))
	}
	seen := map[domain.Action]bool{}
	for _, a := range got {
		if seen[a.Action] {
			t.Errorf("duplicate %s", a.Action)
		}
		seen[a.Action] = true
		if a.Description == "" || a.Area != areaOf(string(a.Action)) {
			t.Errorf("bad entry %+v", a)
		}
	}
	for _, a := range all {
		if !seen[a] || !IsAction(a) {
			t.Errorf("catalog lacks %s", a)
		}
	}
	got[0].Description = "mutated"
	if Actions()[0].Description == "mutated" {
		t.Error("Actions must return a copy")
	}
	b, _ := json.Marshal(Actions()[0])
	if !strings.Contains(string(b), `"action":"tenant:read"`) || !strings.Contains(string(b), `"area":"tenant"`) {
		t.Errorf("JSON = %s", b)
	}
}

func TestRequestCache(t *testing.T) {
	f := newAcme(t)
	dev := f.user("dev")
	f.member(f.acme.ID, dev, domain.RoleDeveloper)
	f.policy(f.acme.ID, "p", true, stmt(domain.EffectDeny, strs("branch:push"), strs("wt:branch/acme/repo/main"), strs("*")))

	ctx := WithRequestCache(context.Background())
	if WithRequestCache(ctx) != ctx {
		t.Fatal("WithRequestCache must be idempotent")
	}
	for i := 0; i < 3; i++ {
		if err := f.az.Authorize(ctx, dev, domain.ActTreeWrite, f.acme.ID, f.repo); err != nil {
			t.Fatal(err)
		}
		if _, err := f.az.Evaluate(ctx, dev, domain.ActBranchPush, domain.Resource{Tree: f.repo, Branch: "dev"}, domain.RequestEnv{}); err != nil {
			t.Fatal(err)
		}
	}
	for _, m := range []string{"GetAccount", "GetMembership", "ListGrants", "ListPolicies", "GetTenant"} {
		if n := f.store.callCount(m); n != 1 {
			t.Errorf("%s called %d times with a request cache, want 1", m, n)
		}
	}

	// Without a request cache, every call sees fresh data.
	plain := context.Background()
	if err := f.az.Authorize(plain, dev, domain.ActTreeWrite, f.acme.ID, f.repo); err != nil {
		t.Fatal(err)
	}
	f.member(f.acme.ID, dev, domain.RoleViewer)
	if err := f.az.Authorize(plain, dev, domain.ActTreeWrite, f.acme.ID, f.repo); err == nil {
		t.Fatal("a role change must apply to the next request")
	}
	// EffectivePermissions reads each input once.
	before := f.store.callCount("ListPolicies")
	if _, err := f.az.EffectivePermissions(plain, dev, f.acme.ID, f.repo); err != nil {
		t.Fatal(err)
	}
	if n := f.store.callCount("ListPolicies") - before; n != 1 {
		t.Errorf("EffectivePermissions listed policies %d times", n)
	}
}

func mustAddr(t *testing.T, s string) netip.Addr {
	t.Helper()
	ip, ok := parseRemoteIP(s)
	if !ok {
		t.Fatalf("bad address %q", s)
	}
	return ip
}
