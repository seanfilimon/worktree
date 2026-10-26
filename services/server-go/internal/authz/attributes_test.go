package authz

// Tests of ABAC attribute conditions (v0.8, docs/ENGINEERING_PLAN.md §16.5).

import (
	"context"
	"errors"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func attr(key, op string, values ...string) domain.AttributeCondition {
	return domain.AttributeCondition{Key: key, Op: op, Values: values}
}

func withAttrs(st domain.PolicyStatement, conds ...domain.AttributeCondition) domain.PolicyStatement {
	st.Conditions.Attributes = conds
	return st
}

func TestAttributeOperators(t *testing.T) {
	cases := []struct {
		op      string
		values  []string
		value   string
		present bool
		want    bool
	}{
		{domain.AttrOpEquals, strs("eng"), "eng", true, true},
		{domain.AttrOpEquals, strs("eng"), "Eng", true, false},
		{domain.AttrOpEquals, strs("eng"), "", false, false},
		{domain.AttrOpNotEquals, strs("eng"), "eng", true, false},
		{domain.AttrOpNotEquals, strs("eng"), "ops", true, true},
		{domain.AttrOpNotEquals, strs("eng"), "", false, true},
		{domain.AttrOpIn, strs("eng", "ops"), "ops", true, true},
		{domain.AttrOpIn, strs("eng", "ops"), "sales", true, false},
		{domain.AttrOpIn, strs("eng"), "", false, false},
		{domain.AttrOpNotIn, strs("eng", "ops"), "ops", true, false},
		{domain.AttrOpNotIn, strs("eng", "ops"), "sales", true, true},
		{domain.AttrOpNotIn, strs("eng"), "", false, true},
		{domain.AttrOpStartsWith, strs("eu-", "uk"), "eu-west", true, true},
		{domain.AttrOpStartsWith, strs("eu-"), "us-east", true, false},
		{domain.AttrOpStartsWith, strs("eu-"), "", false, false},
		{domain.AttrOpExists, nil, "x", true, true},
		{domain.AttrOpExists, nil, "", false, false},
		{domain.AttrOpNotExists, nil, "x", true, false},
		{domain.AttrOpNotExists, nil, "", false, true},
	}
	for _, c := range cases {
		conds, err := compileAttrConds([]domain.AttributeCondition{attr("principal.department", c.op, c.values...)})
		if err != nil {
			t.Fatalf("%s %v: %v", c.op, c.values, err)
		}
		if got := conds[0].eval(c.value, c.present); got != c.want {
			t.Errorf("%s %v on (%q, present=%v) = %v, want %v", c.op, c.values, c.value, c.present, got, c.want)
		}
	}
}

func TestValidateAttributeConditions(t *testing.T) {
	base := stmt(domain.EffectAllow, strs("tree:read"), strs("*"), strs("*"))
	ok := withAttrs(base,
		attr("principal.department", "equals", "eng"),
		attr("tree.classification", "NOT_IN", "secret", "top-secret"),
		attr("tenant.region", "starts_with", "eu-"),
		attr("principal.org.unit", "exists"),
		attr(" tree.owner ", "not_exists"))
	if errs := ValidateStatements([]domain.PolicyStatement{ok}); errs != nil {
		t.Fatalf("valid conditions rejected: %v", errs)
	}
	long := strings.Repeat("x", domain.MaxAttributeValueLen+1)
	bad := withAttrs(base,
		attr("department", "equals", "eng"),      // 0: no scope
		attr("account.department", "exists"),     // 1: unknown scope
		attr("principal.Dept", "exists"),         // 2: bad key
		attr("principal.dept", "like", "x"),      // 3: unknown op
		attr("principal.dept", ""),               // 4: missing op
		attr("principal.dept", "equals"),         // 5: equals needs one value
		attr("principal.dept", "in"),             // 6: in needs values
		attr("principal.dept", "exists", "x"),    // 7: exists takes none
		attr("principal.dept", "in", "", long),   // 8: empty and long values
		attr("", "exists"),                       // 9: key required
		attr("principal.dept", "equals", "a\nb"), // 10: control characters
	)
	errs := ValidateStatements([]domain.PolicyStatement{bad})
	want := []string{
		"conditions.attributes[0].key", "conditions.attributes[1].key", "conditions.attributes[2].key",
		"conditions.attributes[3].op", "conditions.attributes[4].op", "conditions.attributes[5].values",
		"conditions.attributes[6].values", "conditions.attributes[7].values", "conditions.attributes[8].values[0]",
		"conditions.attributes[8].values[1]", "conditions.attributes[9].key", "conditions.attributes[10].values[0]",
	}
	got := map[string]bool{}
	for _, e := range errs {
		got[e.Field] = true
	}
	for _, f := range want {
		if !got[f] {
			t.Errorf("missing error for %s in %v", f, errs)
		}
	}
	if len(errs) != len(want) {
		t.Errorf("got %d errors, want %d: %v", len(errs), len(want), errs)
	}
	many := make([]domain.AttributeCondition, domain.MaxAttributeConditions+1)
	for i := range many {
		many[i] = attr("principal.dept", "exists")
	}
	errs = ValidateStatements([]domain.PolicyStatement{withAttrs(base, many...)})
	if len(errs) != 1 || errs[0].Field != "conditions.attributes" {
		t.Fatalf("too many conditions: %v", errs)
	}
}

// abacFixture: tenant acme (region eu-west) with trees repo (unclassified)
// and vault (classification secret); alice (department eng) and bob
// (department sales) are viewers.
type abacFixture struct {
	*acmeFixture
	vault      *domain.Tree
	alice, bob *domain.Principal
}

func newABAC(t *testing.T) *abacFixture {
	f := &abacFixture{acmeFixture: newAcme(t)}
	tn := f.store.tenants[f.acme.ID]
	tn.Attributes = map[string]string{"region": "eu-west"}
	f.store.tenants[f.acme.ID] = tn
	f.vault = tree("tree-vault", f.acme.ID, "vault", domain.VisibilityPrivate)
	f.vault.Attributes = map[string]string{"classification": "secret"}
	f.alice, f.bob = f.user("alice"), f.user("bob")
	f.member(f.acme.ID, f.alice, domain.RoleViewer)
	f.member(f.acme.ID, f.bob, domain.RoleViewer)
	f.updateAccount(f.alice, func(a *domain.Account) { a.Attributes = map[string]string{"department": "eng", "clearance": "secret"} })
	f.updateAccount(f.bob, func(a *domain.Account) { a.Attributes = map[string]string{"department": "sales"} })
	f.policy(f.acme.ID, "eng-writes", true,
		withAttrs(domain.PolicyStatement{Sid: "eng", Effect: domain.EffectAllow, Actions: strs("tree:write"), Resources: strs("wt:tree/acme/*"), Principals: strs("authenticated")},
			attr("principal.department", "in", "eng", "platform"),
			attr("tenant.region", "starts_with", "eu-")))
	f.policy(f.acme.ID, "secret-trees", true,
		withAttrs(domain.PolicyStatement{Sid: "need-clearance", Effect: domain.EffectDeny, Actions: strs("tree:*"), Resources: strs("wt:tree/acme/*"), Principals: strs("*")},
			attr("tree.classification", "equals", "secret"),
			attr("principal.clearance", "not_in", "secret", "top-secret")))
	return f
}

func TestAttributeConditionsDecide(t *testing.T) {
	f := newABAC(t)
	cases := []struct {
		who  *domain.Principal
		act  domain.Action
		tree *domain.Tree
		want bool
	}{
		{f.alice, domain.ActTreeWrite, f.repo, true},  // eng in the EU
		{f.bob, domain.ActTreeWrite, f.repo, false},   // sales: the allow does not apply
		{f.bob, domain.ActTreeRead, f.repo, true},     // viewer role
		{f.alice, domain.ActTreeRead, f.vault, true},  // cleared
		{f.bob, domain.ActTreeRead, f.vault, false},   // no clearance: explicit deny
		{f.alice, domain.ActTreeWrite, f.vault, true}, // cleared and eng
	}
	for _, c := range cases {
		if got := f.allowed(c.who, c.act, c.tree); got != c.want {
			t.Errorf("%s %s on %s = %v, want %v", c.who.Username, c.act, c.tree.Name, got, c.want)
		}
	}
	// The tenant attribute matters: outside the EU the allow stops applying.
	tn := f.store.tenants[f.acme.ID]
	tn.Attributes = map[string]string{"region": "us-east"}
	f.store.tenants[f.acme.ID] = tn
	if f.allowed(f.alice, domain.ActTreeWrite, f.repo) {
		t.Fatal("tenant.region us-east must not satisfy starts_with eu-")
	}
	// Attribute changes are visible to the next evaluation.
	f.updateAccount(f.bob, func(a *domain.Account) { a.Attributes["clearance"] = "top-secret" })
	if !f.allowed(f.bob, domain.ActTreeRead, f.vault) {
		t.Fatal("bob's new clearance was not picked up")
	}
}

func TestAttributeExplanation(t *testing.T) {
	f := newABAC(t)
	ctx, trace := WithAttributeTrace(context.Background())
	d, err := f.az.Evaluate(ctx, f.bob, domain.ActTreeRead, domain.Resource{Tree: f.vault}, domain.RequestEnv{})
	if err != nil {
		t.Fatal(err)
	}
	if d.Allowed || d.Effect != effectDeny {
		t.Fatalf("decision = %+v", d)
	}
	var attrReasons []string
	for _, r := range d.Reasons {
		if r.Source == SourceAttribute {
			attrReasons = append(attrReasons, r.Detail)
		}
	}
	wantReasons := []string{
		`policy "secret-trees" statement "need-clearance": tree.classification is "secret"; tree.classification equals "secret": met`,
		`policy "secret-trees" statement "need-clearance": principal.clearance is unset; principal.clearance not_in ["secret", "top-secret"]: met`,
	}
	if strings.Join(attrReasons, "\n") != strings.Join(wantReasons, "\n") {
		t.Fatalf("attribute reasons =\n%s\nwant\n%s", strings.Join(attrReasons, "\n"), strings.Join(wantReasons, "\n"))
	}
	checks := trace.Checks()
	if len(checks) != 2 || checks[0].Policy != "secret-trees" || checks[0].PolicyID != "pol-secret-trees" ||
		checks[0].Statement != "need-clearance" || checks[0].Effect != "deny" || checks[0].Key != "tree.classification" ||
		checks[0].Value == nil || *checks[0].Value != "secret" || !checks[0].Matched ||
		checks[1].Value != nil || !checks[1].Matched {
		t.Fatalf("trace = %+v", checks)
	}

	// A failing condition is named in the policy reason.
	d, err = f.az.Evaluate(context.Background(), f.bob, domain.ActTreeWrite, domain.Resource{Tree: f.repo}, domain.RequestEnv{})
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, r := range d.Reasons {
		if r.Source == SourcePolicy && strings.Contains(r.Detail, `does not apply: attribute condition principal.department in ["eng", "platform"] is not met (principal.department is "sales")`) {
			found = true
		}
	}
	if !found || d.Allowed {
		t.Fatalf("reasons = %+v", d.Reasons)
	}
}

func TestAttributeOverrides(t *testing.T) {
	f := newABAC(t)
	res := domain.Resource{Tree: f.vault}
	secret := "secret"
	ov := map[string]*string{"principal.clearance": &secret}
	if err := ValidateAttributeOverrides(ov); err != nil {
		t.Fatal(err)
	}
	ctx := WithAttributeOverrides(context.Background(), ov)
	d, err := f.az.Evaluate(ctx, f.bob, domain.ActTreeRead, res, domain.RequestEnv{})
	if err != nil || !d.Allowed {
		t.Fatalf("override clearance: %+v %v", d, err)
	}
	// nil unsets: alice without clearance is denied.
	ctx = WithAttributeOverrides(context.Background(), map[string]*string{"principal.clearance": nil, "tree.classification": &secret})
	d, err = f.az.Evaluate(ctx, f.alice, domain.ActTreeRead, domain.Resource{Tree: f.repo}, domain.RequestEnv{})
	if err != nil || d.Allowed {
		t.Fatalf("override unset: %+v %v", d, err)
	}
	set := EffectiveAttributes(&domain.Account{Attributes: map[string]string{"clearance": "x", "department": "eng"}},
		nil, &domain.Tenant{Attributes: map[string]string{"region": "eu"}},
		map[string]*string{"principal.clearance": nil, "tree.classification": &secret})
	if len(set.Principal) != 1 || set.Principal["department"] != "eng" || set.Tree["classification"] != "secret" || set.Tenant["region"] != "eu" {
		t.Fatalf("EffectiveAttributes = %+v", set)
	}
	for _, bad := range []map[string]*string{{"department": &secret}, {"principal.x": new(string)}} {
		if err := ValidateAttributeOverrides(bad); err == nil {
			t.Errorf("override %v accepted", bad)
		}
	}
}

func TestAttributesOfServiceAccountsAndAnonymous(t *testing.T) {
	f := newAcme(t)
	f.repo.Visibility = domain.VisibilityPublic
	sa := f.serviceAccount("sa-ci", f.acme.ID, "ci", domain.RoleViewer)
	f.policy(f.acme.ID, "humans-only", true,
		withAttrs(stmt(domain.EffectDeny, strs("tree:read"), strs("*"), strs("*")), attr("principal.department", "not_exists")))
	if f.allowed(sa, domain.ActTreeRead, f.repo) {
		t.Fatal("service accounts have no principal attributes: not_exists must hold")
	}
	if f.allowed(nil, domain.ActTreeRead, f.repo) {
		t.Fatal("anonymous callers have no principal attributes: not_exists must hold")
	}
	dev := f.user("dev")
	f.member(f.acme.ID, dev, domain.RoleViewer)
	f.updateAccount(dev, func(a *domain.Account) { a.Attributes = map[string]string{"department": "eng"} })
	if !f.allowed(dev, domain.ActTreeRead, f.repo) {
		t.Fatal("dev has a department")
	}
}

func TestBrokenAttributeConditionsFailClosed(t *testing.T) {
	f := newAcme(t)
	dev := f.user("dev")
	f.member(f.acme.ID, dev, domain.RoleDeveloper)
	// Corrupt data (bypassing validation): an allow never applies, a deny always does.
	f.policy(f.acme.ID, "corrupt", true,
		withAttrs(stmt(domain.EffectAllow, strs("tree:admin"), strs("*"), strs("*")), attr("principal.dept", "like", "x")),
		withAttrs(stmt(domain.EffectDeny, strs("tree:write"), strs("*"), strs("*")), attr("bogus", "equals", "x")))
	if f.allowed(dev, domain.ActTreeAdmin, f.repo) {
		t.Fatal("allow with a broken attribute condition applied")
	}
	if f.allowed(dev, domain.ActTreeWrite, f.repo) {
		t.Fatal("deny with a broken attribute condition did not apply")
	}
}

func TestEffectivePermissionsUseAttributes(t *testing.T) {
	f := newABAC(t)
	perms, err := f.az.EffectivePermissions(context.Background(), f.alice, f.acme.ID, f.repo)
	if err != nil {
		t.Fatal(err)
	}
	if !containsAction(perms, domain.ActTreeWrite) {
		t.Fatalf("alice's attribute-conditioned allow is missing: %v", perms)
	}
	perms, err = f.az.EffectivePermissions(context.Background(), f.bob, f.acme.ID, f.vault)
	if err != nil {
		t.Fatal(err)
	}
	if containsAction(perms, domain.ActTreeRead) {
		t.Fatalf("bob must not read the vault: %v", perms)
	}
}

func containsAction(as []domain.Action, a domain.Action) bool {
	for _, x := range as {
		if x == a {
			return true
		}
	}
	return false
}

func TestInvalidateAttributesInRequestCache(t *testing.T) {
	f := newABAC(t)
	ctx := WithRequestCache(context.Background())
	if f.allowedCtx(ctx, f.bob, domain.ActTreeRead, f.vault) {
		t.Fatal("bob has no clearance yet")
	}
	f.updateAccount(f.bob, func(a *domain.Account) {
		a.Attributes = map[string]string{"department": "sales", "clearance": "secret"}
	})
	if f.allowedCtx(ctx, f.bob, domain.ActTreeRead, f.vault) {
		t.Fatal("the request cache should still hold the old account")
	}
	InvalidateAttributes(ctx)
	if !f.allowedCtx(ctx, f.bob, domain.ActTreeRead, f.vault) {
		t.Fatal("InvalidateAttributes did not drop the cached account")
	}
	InvalidateAttributes(context.Background()) // no cache: no-op
}

func (f *abacFixture) allowedCtx(ctx context.Context, p *domain.Principal, act domain.Action, tr *domain.Tree) bool {
	f.t.Helper()
	d, err := f.az.Evaluate(ctx, p, act, domain.Resource{Tree: tr}, domain.RequestEnv{})
	if err != nil {
		f.t.Fatal(err)
	}
	return d.Allowed
}

func TestPolicyCacheTracksAttributeConditions(t *testing.T) {
	var c policyCache
	pols := []domain.Policy{{ID: "p", Name: "p", Enabled: true, Statements: []domain.PolicyStatement{
		withAttrs(stmt(domain.EffectAllow, strs("tree:read"), strs("*"), strs("*")), attr("principal.dept", "in", "eng")),
	}}}
	first := c.compile("t", pols)
	if !first[0].usesAttrs || len(first[0].statements[0].cond.attrs) != 1 {
		t.Fatalf("compiled = %+v", first[0])
	}
	// The cache keeps a private copy of the values.
	pols[0].Statements[0].Conditions.Attributes[0].Values[0] = "ops"
	second := c.compile("t", pols)
	if &second[0] == &first[0] || second[0].statements[0].cond.attrs[0].values[0] != "ops" {
		t.Fatal("changing a condition value must recompile")
	}
	if got := c.compile("t", pols); &got[0] != &second[0] {
		t.Fatal("unchanged attribute conditions were recompiled")
	}
	if first[0].statements[0].cond.attrs[0].values[0] != "eng" {
		t.Fatal("compiled conditions alias the caller's slices")
	}
}

func TestAttributeValidationHelpers(t *testing.T) {
	if err := domain.ValidateAttributes(map[string]string{"department": "eng", "org.unit": "a-b", "x_1": "y"}); err != nil {
		t.Fatal(err)
	}
	for _, bad := range []map[string]string{{"Dept": "x"}, {"1x": "x"}, {"dept": ""}, {"dept": strings.Repeat("a", 257)}, {strings.Repeat("a", 65): "x"}} {
		if err := domain.ValidateAttributes(bad); !errors.Is(err, domain.ErrInvalid) {
			t.Errorf("ValidateAttributes(%v) = %v", bad, err)
		}
	}
	many := map[string]string{}
	for i := 0; i <= domain.MaxAttributes; i++ {
		many["k"+strings.Repeat("a", i)] = "v"
	}
	if err := domain.ValidateAttributes(many); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("too many attributes accepted: %v", err)
	}
}
