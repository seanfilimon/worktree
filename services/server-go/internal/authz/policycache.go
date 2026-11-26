package authz

import (
	"slices"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Compiled-policy cache (hardening, docs/ENGINEERING_PLAN.md §15.10).
//
// Compiling a tenant's policies (parsing resources, CIDRs and time windows,
// loading time zones) dominated authorization for tenants with many
// policies, and it ran on every evaluation without a request cache — e.g.
// once per tree for a code search over many trees. The cache keeps the last
// compilation per tenant together with a deep copy of the policies it was
// compiled from, and reuses it only while the store still returns exactly
// those policies (compared field by field), so an update, a delete or a
// re-created policy is picked up on the next evaluation; there is no
// expiry to tune and no staleness.

// maxCachedTenants bounds the cache; when it is full it starts over.
const maxCachedTenants = 4096

type policyCache struct {
	mu sync.Mutex
	m  map[string]*policyCacheEntry // tenant id -> last compilation
}

type policyCacheEntry struct {
	src      []domain.Policy // private deep copy of the compiled policies
	compiled []compiledPolicy
	// gen is the store's policy generation read before the policies were
	// listed (hasGen false when unknown); see policyGenerationer.
	gen    uint64
	hasGen bool
}

// policyGenerationer is implemented by stores that count policy writes
// (meta.Bolt). While the generation is unchanged the store's policies
// are too, so the cache can skip listing (and JSON-decoding) them.
type policyGenerationer interface {
	PolicyGeneration() uint64
}

// byGeneration returns the cached compilation for tenantID if it was made
// from a listing taken at generation gen.
func (c *policyCache) byGeneration(tenantID string, gen uint64) ([]compiledPolicy, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if e := c.m[tenantID]; e != nil && e.hasGen && e.gen == gen {
		return e.compiled, true
	}
	return nil, false
}

// compileAt is compile for policies listed at generation gen.
func (c *policyCache) compileAt(tenantID string, pols []domain.Policy, gen uint64) []compiledPolicy {
	compiled := c.compile(tenantID, pols)
	c.mu.Lock()
	if e := c.m[tenantID]; e != nil && sameCompiled(e.compiled, compiled) {
		e.gen, e.hasGen = gen, true
	}
	c.mu.Unlock()
	return compiled
}

// sameCompiled reports whether a and b are the same compilation (same
// backing array), i.e. the entry was not replaced concurrently.
func sameCompiled(a, b []compiledPolicy) bool {
	return len(a) == len(b) && (len(a) == 0 || &a[0] == &b[0])
}

// compile returns compilePolicies(pols), reusing the cached result for
// tenantID when pols are unchanged. The result must not be modified.
func (c *policyCache) compile(tenantID string, pols []domain.Policy) []compiledPolicy {
	c.mu.Lock()
	e := c.m[tenantID]
	c.mu.Unlock()
	if e != nil && samePolicies(e.src, pols) {
		return e.compiled
	}
	src := clonePolicies(pols)
	compiled := compilePolicies(src) // aliases src only, never the caller's slices
	c.mu.Lock()
	if c.m == nil || len(c.m) >= maxCachedTenants {
		c.m = make(map[string]*policyCacheEntry)
	}
	c.m[tenantID] = &policyCacheEntry{src: src, compiled: compiled}
	c.mu.Unlock()
	return compiled
}

// samePolicies reports whether a and b agree on every field that
// compilePolicies reads. TestPolicyCacheCoversEveryField fails when a
// policy type gains a field, so that this comparison is kept complete.
func samePolicies(a, b []domain.Policy) bool {
	if len(a) != len(b) {
		return false
	}
	for i := range a {
		x, y := &a[i], &b[i]
		if x.ID != y.ID || x.Name != y.Name || x.Enabled != y.Enabled || len(x.Statements) != len(y.Statements) {
			return false
		}
		for j := range x.Statements {
			if !sameStatement(&x.Statements[j], &y.Statements[j]) {
				return false
			}
		}
	}
	return true
}

func sameStatement(x, y *domain.PolicyStatement) bool {
	return x.Sid == y.Sid && x.Effect == y.Effect &&
		slices.Equal(x.Actions, y.Actions) && slices.Equal(x.Resources, y.Resources) &&
		slices.Equal(x.Principals, y.Principals) && sameConditions(&x.Conditions, &y.Conditions)
}

func sameConditions(x, y *domain.PolicyConditions) bool {
	if x.RequireMFA != y.RequireMFA || !slices.Equal(x.IPRanges, y.IPRanges) ||
		!sameTime(x.NotBefore, y.NotBefore) || !sameTime(x.NotAfter, y.NotAfter) ||
		!slices.EqualFunc(x.Attributes, y.Attributes, sameAttrCond) { // v0.8 ABAC
		return false
	}
	switch {
	case x.TimeWindow == nil || y.TimeWindow == nil:
		return x.TimeWindow == nil && y.TimeWindow == nil
	default:
		v, w := x.TimeWindow, y.TimeWindow
		return slices.Equal(v.Days, w.Days) && v.Start == w.Start && v.End == w.End && v.Timezone == w.Timezone
	}
}

func sameAttrCond(a, b domain.AttributeCondition) bool {
	return a.Key == b.Key && a.Op == b.Op && slices.Equal(a.Values, b.Values)
}

// sameTime compares instants and locations (the location shows in
// decision details).
func sameTime(a, b *time.Time) bool {
	if a == nil || b == nil {
		return a == nil && b == nil
	}
	return a.Equal(*b) && a.Location() == b.Location()
}

// clonePolicies deep-copies the fields compilePolicies reads (and the rest
// of each policy shallowly).
func clonePolicies(pols []domain.Policy) []domain.Policy {
	out := slices.Clone(pols)
	for i := range out {
		sts := slices.Clone(out[i].Statements)
		for j := range sts {
			st := &sts[j]
			st.Actions = slices.Clone(st.Actions)
			st.Resources = slices.Clone(st.Resources)
			st.Principals = slices.Clone(st.Principals)
			c := &st.Conditions
			c.IPRanges = slices.Clone(c.IPRanges)
			if c.NotBefore != nil {
				t := *c.NotBefore
				c.NotBefore = &t
			}
			if c.NotAfter != nil {
				t := *c.NotAfter
				c.NotAfter = &t
			}
			if c.TimeWindow != nil {
				tw := *c.TimeWindow
				tw.Days = slices.Clone(tw.Days)
				c.TimeWindow = &tw
			}
			if c.Attributes != nil { // v0.8 ABAC
				c.Attributes = slices.Clone(c.Attributes)
				for k := range c.Attributes {
					c.Attributes[k].Values = slices.Clone(c.Attributes[k].Values)
				}
			}
		}
		out[i].Statements = sts
	}
	return out
}
