package authz

// Tests of the compiled-policy cache (hardening, docs/ENGINEERING_PLAN.md
// §15.10).

import (
	"context"
	"errors"
	"reflect"
	"slices"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func fieldNames(v any) []string {
	t := reflect.TypeOf(v)
	out := make([]string, t.NumField())
	for i := range out {
		out[i] = t.Field(i).Name
	}
	return out
}

// TestPolicyCacheCoversEveryField fails when a policy type gains a field:
// decide whether compilePolicies uses it and, if so, compare and copy it
// in samePolicies / clonePolicies before updating the lists here.
func TestPolicyCacheCoversEveryField(t *testing.T) {
	cases := []struct {
		v    any
		want []string
	}{
		// Compared: ID, Name, Enabled, Statements. The others do not affect
		// compilation.
		{domain.Policy{}, []string{"ID", "TenantID", "Name", "Description", "Enabled", "Statements", "Version", "ManagedBy", "TreeID", "CreatedBy", "CreatedAt", "UpdatedAt"}},
		{domain.PolicyStatement{}, []string{"Sid", "Effect", "Actions", "Resources", "Principals", "Conditions"}},
		{domain.PolicyConditions{}, []string{"IPRanges", "RequireMFA", "NotBefore", "NotAfter", "TimeWindow", "Attributes"}},
		{domain.AttributeCondition{}, []string{"Key", "Op", "Values"}},
		{domain.TimeWindow{}, []string{"Days", "Start", "End", "Timezone"}},
	}
	for _, c := range cases {
		if got := fieldNames(c.v); !slices.Equal(got, c.want) {
			t.Errorf("%T fields = %v, want %v: update policycache.go", c.v, got, c.want)
		}
	}
}

func cachePolicies() []domain.Policy {
	nb := time.Date(2026, 1, 1, 0, 0, 0, 0, time.UTC)
	return []domain.Policy{
		{ID: "p1", Name: "one", Enabled: true, Version: 1, Statements: []domain.PolicyStatement{
			stmt(domain.EffectAllow, strs("tree:read"), strs("wt:tree/acme/a"), strs("user:bob")),
			{Effect: domain.EffectDeny, Actions: strs("tree:write"), Resources: strs("*"), Principals: strs("*"),
				Conditions: domain.PolicyConditions{IPRanges: strs("10.0.0.0/8"), NotBefore: &nb,
					TimeWindow: &domain.TimeWindow{Days: strs("mon"), Start: "09:00", End: "17:00", Timezone: "Europe/Berlin"}}},
		}},
		{ID: "p2", Name: "two", Enabled: false, Statements: []domain.PolicyStatement{
			stmt(domain.EffectDeny, strs("*"), strs("*"), strs("*")),
		}},
	}
}

func TestPolicyCacheReusesUnchangedPolicies(t *testing.T) {
	var c policyCache
	first := c.compile("t1", cachePolicies())
	if len(first) != 1 || len(first[0].statements) != 2 {
		t.Fatalf("compiled = %+v", first)
	}
	// Equal content (fresh slices, other metadata changed) is a hit.
	again := cachePolicies()
	again[0].Version, again[0].Description = 7, "metadata only"
	if got := c.compile("t1", again); &got[0] != &first[0] {
		t.Fatal("unchanged policies were recompiled")
	}
	// Another tenant has its own entry.
	if got := c.compile("t2", cachePolicies()); &got[0] == &first[0] {
		t.Fatal("tenants share a cache entry")
	}
	// The cache holds a private copy: mutating the caller's slices after
	// compiling neither changes the result nor counts as unchanged.
	pols := cachePolicies()
	compiled := c.compile("t3", pols)
	pols[0].Statements[0].Resources[0] = "wt:tree/acme/b"
	if compiled[0].statements[0].resources[0].segs[1] != "a" {
		t.Fatal("compiled policy aliases the caller's slices")
	}
	if got := c.compile("t3", pols); &got[0] == &compiled[0] {
		t.Fatal("mutated policies were served from the cache")
	}
}

func TestPolicyCacheDetectsEveryChange(t *testing.T) {
	later := time.Date(2026, 6, 1, 0, 0, 0, 0, time.UTC)
	changes := map[string]func(p []domain.Policy) []domain.Policy{
		"id":         func(p []domain.Policy) []domain.Policy { p[0].ID = "p9"; return p },
		"name":       func(p []domain.Policy) []domain.Policy { p[0].Name = "renamed"; return p },
		"enabled":    func(p []domain.Policy) []domain.Policy { p[1].Enabled = true; return p },
		"deleted":    func(p []domain.Policy) []domain.Policy { return p[:1] },
		"statement":  func(p []domain.Policy) []domain.Policy { p[0].Statements = p[0].Statements[:1]; return p },
		"sid":        func(p []domain.Policy) []domain.Policy { p[0].Statements[0].Sid = "s"; return p },
		"effect":     func(p []domain.Policy) []domain.Policy { p[0].Statements[0].Effect = domain.EffectDeny; return p },
		"action":     func(p []domain.Policy) []domain.Policy { p[0].Statements[0].Actions[0] = "tree:write"; return p },
		"resource":   func(p []domain.Policy) []domain.Policy { p[0].Statements[0].Resources[0] = "*"; return p },
		"principal":  func(p []domain.Policy) []domain.Policy { p[0].Statements[0].Principals[0] = "*"; return p },
		"ip":         func(p []domain.Policy) []domain.Policy { p[0].Statements[1].Conditions.IPRanges[0] = "::1"; return p },
		"mfa":        func(p []domain.Policy) []domain.Policy { p[0].Statements[1].Conditions.RequireMFA = true; return p },
		"not_before": func(p []domain.Policy) []domain.Policy { p[0].Statements[1].Conditions.NotBefore = &later; return p },
		"not_after":  func(p []domain.Policy) []domain.Policy { p[0].Statements[1].Conditions.NotAfter = &later; return p },
		"window_nil": func(p []domain.Policy) []domain.Policy { p[0].Statements[1].Conditions.TimeWindow = nil; return p },
		"window_day": func(p []domain.Policy) []domain.Policy {
			p[0].Statements[1].Conditions.TimeWindow.Days[0] = "tue"
			return p
		},
		"window_start": func(p []domain.Policy) []domain.Policy {
			p[0].Statements[1].Conditions.TimeWindow.Start = "08:00"
			return p
		},
		"window_end": func(p []domain.Policy) []domain.Policy {
			p[0].Statements[1].Conditions.TimeWindow.End = "18:00"
			return p
		},
		"window_tz": func(p []domain.Policy) []domain.Policy {
			p[0].Statements[1].Conditions.TimeWindow.Timezone = "UTC"
			return p
		},
	}
	for name, change := range changes {
		var c policyCache
		first := c.compile("t", cachePolicies())
		got := c.compile("t", change(cachePolicies()))
		if len(got) > 0 && len(first) > 0 && &got[0] == &first[0] {
			t.Errorf("%s: change not detected", name)
		}
		if !reflect.DeepEqual(got, compilePolicies(change(cachePolicies()))) {
			t.Errorf("%s: cached result differs from a fresh compilation", name)
		}
	}
}

// TestPolicyUpdatesApplyImmediately: the authorizer sees a changed policy
// on the very next evaluation, without a request cache.
func TestPolicyUpdatesApplyImmediately(t *testing.T) {
	ctx := context.Background()
	e := newEnv(t)
	acme := e.tenant("t-acme", "acme")
	bob := e.user("bob")
	e.member(acme.ID, bob, domain.RoleDeveloper)
	repo := tree("tree-repo", acme.ID, "repo", domain.VisibilityPrivate)
	e.policy(acme.ID, "freeze", true, stmt(domain.EffectDeny, strs("tree:write"), strs("wt:tree/acme/repo"), strs("user:bob")))

	if err := e.az.Authorize(ctx, bob, domain.ActTreeWrite, acme.ID, repo); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("frozen: %v", err)
	}
	// Edit in place: the deny now names another tree.
	e.store.policies[acme.ID][0].Statements[0].Resources[0] = "wt:tree/acme/other"
	if err := e.az.Authorize(ctx, bob, domain.ActTreeWrite, acme.ID, repo); err != nil {
		t.Fatalf("after edit: %v", err)
	}
	// Disable, re-enable, delete.
	e.store.policies[acme.ID][0].Statements[0].Resources[0] = "wt:tree/acme/repo"
	e.store.policies[acme.ID][0].Enabled = false
	if err := e.az.Authorize(ctx, bob, domain.ActTreeWrite, acme.ID, repo); err != nil {
		t.Fatalf("disabled: %v", err)
	}
	e.store.policies[acme.ID][0].Enabled = true
	if err := e.az.Authorize(ctx, bob, domain.ActTreeWrite, acme.ID, repo); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("re-enabled: %v", err)
	}
	e.store.policies[acme.ID] = nil
	if err := e.az.Authorize(ctx, bob, domain.ActTreeWrite, acme.ID, repo); err != nil {
		t.Fatalf("deleted: %v", err)
	}
}
