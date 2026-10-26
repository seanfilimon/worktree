package authz

// ABAC attribute conditions (v0.8, docs/ENGINEERING_PLAN.md §16.5).
//
// A statement's conditions.attributes test attributes of the caller
// (principal.*: Account.Attributes; service accounts and anonymous callers
// have none), of the resource's tree (tree.*: Tree.Attributes; none for
// tenant-level requests) and of the resource's tenant (tenant.*:
// Tenant.Attributes). All conditions must hold, together with the other
// conditions of the statement.
//
// Cost: attribute conditions are compiled with the statement (and cached
// with the compiled policies). The attribute source of a request is only
// built when one of the tenant's enabled policies uses attribute
// conditions; policies without them evaluate exactly as before. The
// attributes themselves are read from the account and tenant the
// evaluation loads anyway (per request, see cache.go) and from the tree
// of the request, so an attribute change is visible to the next request.

import (
	"context"
	"fmt"
	"slices"
	"strconv"
	"strings"
	"sync"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// SourceAttribute is the DecisionReason.Source of the per-condition
// explanation of attribute conditions (effect "info").
const SourceAttribute = "attribute"

// attrCond is a compiled AttributeCondition.
type attrCond struct {
	ref    string // "principal.department"
	scope  string // domain.AttrScope*
	key    string // "department"
	op     string // domain.AttrOp*
	values []string
}

// attrResult is the outcome of one attribute condition.
type attrResult struct {
	cond    *attrCond
	value   string
	present bool
	matched bool
}

// compileAttrConds compiles attribute conditions. Statements are validated
// on write, so an error only guards against corrupt data (it makes the
// whole condition set broken: see compiledConditions.err).
func compileAttrConds(conds []domain.AttributeCondition) ([]attrCond, error) {
	if len(conds) == 0 {
		return nil, nil
	}
	out := make([]attrCond, 0, len(conds))
	for i, c := range conds {
		if errs := checkAttrCond(c); len(errs) > 0 {
			return nil, fmt.Errorf("attribute condition %d: %s", i+1, errs[0].msg)
		}
		scope, key, _ := domain.SplitAttributeRef(strings.TrimSpace(c.Key))
		out = append(out, attrCond{
			ref: scope + "." + key, scope: scope, key: key,
			op:     strings.ToLower(strings.TrimSpace(c.Op)),
			values: slices.Clone(c.Values),
		})
	}
	return out, nil
}

// attrProblem is one validation problem of an attribute condition.
type attrProblem struct {
	field string // "key", "op", "values", "values[2]"
	msg   string
}

// checkAttrCond validates one attribute condition.
func checkAttrCond(c domain.AttributeCondition) []attrProblem {
	var out []attrProblem
	if strings.TrimSpace(c.Key) == "" {
		out = append(out, attrProblem{"key", "is required (principal.<key>, tree.<key> or tenant.<key>)"})
	} else if _, _, err := domain.SplitAttributeRef(strings.TrimSpace(c.Key)); err != nil {
		out = append(out, attrProblem{"key", err.Error()})
	}
	op := strings.ToLower(strings.TrimSpace(c.Op))
	n := len(c.Values)
	switch op {
	case domain.AttrOpEquals, domain.AttrOpNotEquals:
		if n != 1 {
			out = append(out, attrProblem{"values", fmt.Sprintf("%s needs exactly one value, got %d", op, n)})
		}
	case domain.AttrOpIn, domain.AttrOpNotIn, domain.AttrOpStartsWith:
		if n == 0 {
			out = append(out, attrProblem{"values", fmt.Sprintf("%s needs at least one value", op)})
		}
	case domain.AttrOpExists, domain.AttrOpNotExists:
		if n != 0 {
			out = append(out, attrProblem{"values", fmt.Sprintf("%s takes no values, got %d", op, n)})
		}
	case "":
		out = append(out, attrProblem{"op", "is required (" + strings.Join(domain.AttributeOps, ", ") + ")"})
	default:
		out = append(out, attrProblem{"op", fmt.Sprintf("unknown operator %q (want %s)", c.Op, strings.Join(domain.AttributeOps, ", "))})
	}
	if n > domain.MaxAttributeCondValues {
		out = append(out, attrProblem{"values", fmt.Sprintf("at most %d values are allowed", domain.MaxAttributeCondValues)})
	}
	for j, v := range c.Values {
		if err := domain.ValidateAttributeValue(v); err != nil {
			out = append(out, attrProblem{fmt.Sprintf("values[%d]", j), err.Error()})
		}
	}
	return out
}

// validateAttributeConditions returns the ValidationErrors of a statement's
// attribute conditions (statement index i).
func validateAttributeConditions(i int, conds []domain.AttributeCondition) []ValidationError {
	var errs []ValidationError
	if len(conds) > domain.MaxAttributeConditions {
		errs = append(errs, ValidationError{Statement: i, Field: "conditions.attributes",
			Message: fmt.Sprintf("at most %d attribute conditions are allowed", domain.MaxAttributeConditions)})
	}
	for j, c := range conds {
		for _, p := range checkAttrCond(c) {
			errs = append(errs, ValidationError{Statement: i, Field: fmt.Sprintf("conditions.attributes[%d].%s", j, p.field), Message: p.msg})
		}
	}
	return errs
}

// eval decides one condition against v (present reports whether the
// attribute is set).
func (c *attrCond) eval(v string, present bool) bool {
	switch c.op {
	case domain.AttrOpEquals:
		return present && v == c.values[0]
	case domain.AttrOpNotEquals:
		return !present || v != c.values[0]
	case domain.AttrOpIn:
		return present && slices.Contains(c.values, v)
	case domain.AttrOpNotIn:
		return !present || !slices.Contains(c.values, v)
	case domain.AttrOpStartsWith:
		if !present {
			return false
		}
		for _, p := range c.values {
			if strings.HasPrefix(v, p) {
				return true
			}
		}
		return false
	case domain.AttrOpExists:
		return present
	case domain.AttrOpNotExists:
		return !present
	}
	return false
}

func (c *attrCond) String() string {
	switch c.op {
	case domain.AttrOpExists, domain.AttrOpNotExists:
		return c.ref + " " + c.op
	case domain.AttrOpEquals, domain.AttrOpNotEquals:
		return c.ref + " " + c.op + " " + strconv.Quote(c.values[0])
	}
	q := make([]string, len(c.values))
	for i, v := range c.values {
		q[i] = strconv.Quote(v)
	}
	return c.ref + " " + c.op + " [" + strings.Join(q, ", ") + "]"
}

func (r *attrResult) actual() string {
	if !r.present {
		return r.cond.ref + " is unset"
	}
	return r.cond.ref + " is " + strconv.Quote(r.value)
}

// ---- attribute sources ----

// attrSource holds the attributes visible to one evaluation.
type attrSource struct {
	principal, tree, tenant map[string]string
	trace                   *AttributeTrace
}

func (s *attrSource) lookup(scope, key string) (string, bool) {
	if s == nil {
		return "", false
	}
	var m map[string]string
	switch scope {
	case domain.AttrScopePrincipal:
		m = s.principal
	case domain.AttrScopeTree:
		m = s.tree
	case domain.AttrScopeTenant:
		m = s.tenant
	}
	v, ok := m[key]
	return v, ok
}

// AttributeSet is the set of attributes an evaluation sees, by scope.
type AttributeSet struct {
	Principal map[string]string `json:"principal"`
	Tree      map[string]string `json:"tree"`
	Tenant    map[string]string `json:"tenant"`
}

// EffectiveAttributes returns the attributes an evaluation sees for the
// given objects (any may be nil) after applying overrides (see
// WithAttributeOverrides). The maps are fresh copies and never nil.
func EffectiveAttributes(acct *domain.Account, tree *domain.Tree, tenant *domain.Tenant, overrides map[string]*string) AttributeSet {
	var p, tr, tn map[string]string
	if acct != nil {
		p = acct.Attributes
	}
	if tree != nil {
		tr = tree.Attributes
	}
	if tenant != nil {
		tn = tenant.Attributes
	}
	set := AttributeSet{Principal: maps(p), Tree: maps(tr), Tenant: maps(tn)}
	for ref, v := range overrides {
		scope, key, err := domain.SplitAttributeRef(ref)
		if err != nil {
			continue
		}
		m := set.Principal
		switch scope {
		case domain.AttrScopeTree:
			m = set.Tree
		case domain.AttrScopeTenant:
			m = set.Tenant
		}
		if v == nil {
			delete(m, key)
		} else {
			m[key] = *v
		}
	}
	return set
}

func maps(m map[string]string) map[string]string {
	out := make(map[string]string, len(m))
	for k, v := range m {
		out[k] = v
	}
	return out
}

// attributeSource builds the attribute source of an evaluation (only
// called when a policy of the tenant uses attribute conditions).
func attributeSource(ctx context.Context, s *subject, tenant *domain.Tenant, res domain.Resource) *attrSource {
	src := &attrSource{trace: traceFrom(ctx)}
	if ov := overridesFrom(ctx); ov != nil {
		set := EffectiveAttributes(s.acct, res.Tree, tenant, ov)
		src.principal, src.tree, src.tenant = set.Principal, set.Tree, set.Tenant
		return src
	}
	if s.acct != nil {
		src.principal = s.acct.Attributes
	}
	if res.Tree != nil {
		src.tree = res.Tree.Attributes
	}
	if tenant != nil {
		src.tenant = tenant.Attributes
	}
	return src
}

// checkAttributes evaluates the attribute conditions once the other
// conditions hold (cr). Without a trace it stops at the first condition
// that fails; with one (the simulator) every condition is evaluated and
// returned in cr.attrs so that the decision can explain each of them.
func (c *compiledConditions) checkAttributes(env evalEnv, cr condResult) condResult {
	if env.attrs == nil || env.attrs.trace == nil {
		for i := range c.attrs {
			ac := &c.attrs[i]
			v, ok := env.attrs.lookup(ac.scope, ac.key)
			if !ac.eval(v, ok) {
				r := attrResult{cond: ac, value: v, present: ok}
				cr.holds = false
				cr.detail = "attribute condition " + ac.String() + " is not met (" + r.actual() + ")"
				return cr
			}
		}
		return cr
	}
	results := make([]attrResult, len(c.attrs))
	var failed *attrResult
	for i := range c.attrs {
		ac := &c.attrs[i]
		v, ok := env.attrs.lookup(ac.scope, ac.key)
		results[i] = attrResult{cond: ac, value: v, present: ok, matched: ac.eval(v, ok)}
		if !results[i].matched && failed == nil {
			failed = &results[i]
		}
	}
	cr.attrs = results
	if failed != nil {
		cr.holds = false
		cr.detail = "attribute condition " + failed.cond.String() + " is not met (" + failed.actual() + ")"
	}
	return cr
}

// recordAttributeChecks explains the attribute conditions of a statement
// when the context carries a trace (the simulator): one SourceAttribute
// reason per condition in the decision, and one AttributeCheck in the
// trace. Other evaluations skip this (the policy reason still names the
// first condition that failed), keeping Authorize cheap.
func recordAttributeChecks(env evalEnv, e *evaluation, pol *compiledPolicy, st *compiledStatement, results []attrResult) {
	var tr *AttributeTrace
	if env.attrs != nil {
		tr = env.attrs.trace
	}
	if tr == nil {
		return
	}
	label := st.label(pol)
	for i := range results {
		r := &results[i]
		verdict := "not met"
		if r.matched {
			verdict = "met"
		}
		e.info(SourceAttribute, "%s: %s; %s: %s", label, r.actual(), r.cond.String(), verdict)
		stmt := st.sid
		if stmt == "" {
			stmt = "#" + strconv.Itoa(st.index+1)
		}
		chk := AttributeCheck{Policy: pol.name, PolicyID: pol.id, Statement: stmt, Effect: st.effect,
			Key: r.cond.ref, Op: r.cond.op, Values: nonNilStrings(r.cond.values), Matched: r.matched}
		if r.present {
			v := r.value
			chk.Value = &v
		}
		tr.add(chk)
	}
}

func nonNilStrings(s []string) []string {
	if s == nil {
		return []string{}
	}
	return slices.Clone(s)
}

// ---- simulator support ----

// AttributeCheck is the outcome of one attribute condition of a statement
// whose other parts matched the request (POST /iam/simulate).
type AttributeCheck struct {
	Policy    string   `json:"policy"`    // policy name
	PolicyID  string   `json:"policy_id"` // policy id ("" for unsaved candidates)
	Statement string   `json:"statement"` // sid, or "#<n>" (1-based position)
	Effect    string   `json:"effect"`    // allow | deny
	Key       string   `json:"key"`       // "principal.department"
	Op        string   `json:"op"`
	Values    []string `json:"values"`
	Value     *string  `json:"value"` // the attribute's value; null when unset
	Matched   bool     `json:"matched"`
}

// AttributeTrace collects the attribute checks of evaluations made with
// its context (WithAttributeTrace).
type AttributeTrace struct {
	mu     sync.Mutex
	checks []AttributeCheck
}

func (t *AttributeTrace) add(c AttributeCheck) {
	t.mu.Lock()
	t.checks = append(t.checks, c)
	t.mu.Unlock()
}

// Checks returns the recorded checks in evaluation order (never nil).
func (t *AttributeTrace) Checks() []AttributeCheck {
	t.mu.Lock()
	defer t.mu.Unlock()
	return append([]AttributeCheck{}, t.checks...)
}

type traceKey struct{}
type overridesKey struct{}

// WithAttributeTrace returns a context whose evaluations record every
// attribute condition they decide in the returned trace.
func WithAttributeTrace(ctx context.Context) (context.Context, *AttributeTrace) {
	t := &AttributeTrace{}
	return context.WithValue(ctx, traceKey{}, t), t
}

func traceFrom(ctx context.Context) *AttributeTrace {
	t, _ := ctx.Value(traceKey{}).(*AttributeTrace)
	return t
}

// WithAttributeOverrides returns a context whose evaluations see the
// given attribute values instead of the stored ones: keys are condition
// keys ("principal.department", "tree.classification", "tenant.region"),
// a nil value unsets the attribute. For "what if" simulations
// (POST /iam/simulate context.attributes); validate them with
// ValidateAttributeOverrides first.
func WithAttributeOverrides(ctx context.Context, overrides map[string]*string) context.Context {
	cp := make(map[string]*string, len(overrides))
	for k, v := range overrides {
		cp[strings.TrimSpace(k)] = v
	}
	return context.WithValue(ctx, overridesKey{}, cp)
}

func overridesFrom(ctx context.Context) map[string]*string {
	m, _ := ctx.Value(overridesKey{}).(map[string]*string)
	return m
}

// ValidateAttributeOverrides checks simulator overrides: condition keys
// and attribute values (at most domain.MaxAttributes * 3 entries).
func ValidateAttributeOverrides(overrides map[string]*string) error {
	if len(overrides) > 3*domain.MaxAttributes {
		return fmt.Errorf("at most %d attribute overrides are allowed", 3*domain.MaxAttributes)
	}
	keys := make([]string, 0, len(overrides))
	for k := range overrides {
		keys = append(keys, k)
	}
	slices.Sort(keys)
	for _, k := range keys {
		if _, _, err := domain.SplitAttributeRef(strings.TrimSpace(k)); err != nil {
			return err
		}
		if v := overrides[k]; v != nil {
			if err := domain.ValidateAttributeValue(*v); err != nil {
				return fmt.Errorf("attribute %s: %v", k, err)
			}
		}
	}
	return nil
}

// InvalidateAttributes drops the accounts and tenants memoized by the
// request cache of ctx (if any), so that evaluations later in the same
// request see attribute changes made by it. Attributes are never cached
// across requests, and the compiled-policy cache does not depend on them.
func InvalidateAttributes(ctx context.Context) {
	c := cacheFrom(ctx)
	if c == nil {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	for k := range c.m {
		if strings.HasPrefix(k, "acct:") || strings.HasPrefix(k, "tenant:") {
			delete(c.m, k)
		}
	}
}
