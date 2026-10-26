package authz

// Benchmarks of ABAC attribute conditions (v0.8, §16.5). Compare
// BenchmarkAuthorize (no attribute conditions anywhere: unchanged by
// attribute support, the attribute source is never built) with
// BenchmarkAuthorizeAttributes (the same tenant where a tenth of the
// policies carry attribute conditions):
//
//	go test ./internal/authz -run '^$' -bench 'Authorize' -benchmem

import (
	"context"
	"fmt"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// newAttrBench is newAuthzBench with attribute conditions on every tenth
// policy (matching the developer's tree) and attributes on the developer,
// the tenant and the tree.
func newAttrBench() *authzBench {
	ab := newAuthzBench()
	st := ab.e.store
	pols := st.policies["t-acme"]
	for i := range pols {
		if i%10 != 0 {
			continue
		}
		allow := withAttrs(stmt(domain.EffectAllow, strs("tree:*"), strs("wt:tree/acme/repo"), strs("authenticated")),
			attr("principal.department", "in", "eng", "platform"),
			attr("tree.classification", "not_equals", "secret"),
			attr("tenant.region", "starts_with", "eu-"))
		deny := withAttrs(stmt(domain.EffectDeny, strs("tree:admin"), strs("wt:tree/acme/repo"), strs("*")),
			attr("principal.clearance", "not_exists"))
		pols[i].Statements = append(pols[i].Statements, allow, deny)
		pols[i].Name = fmt.Sprintf("p%03d-abac", i)
	}
	ab.e.updateAccount(ab.dev, func(a *domain.Account) {
		a.Attributes = map[string]string{"department": "eng", "location": "berlin"}
	})
	tn := st.tenants["t-acme"]
	tn.Attributes = map[string]string{"region": "eu-west"}
	st.tenants["t-acme"] = tn
	ab.tree.Attributes = map[string]string{"classification": "internal"}
	return ab
}

func BenchmarkAuthorizeAttributes(b *testing.B) {
	ab := newAttrBench()
	ctx := context.Background()
	for _, act := range []domain.Action{domain.ActTreeRead, domain.ActTreeWrite, domain.ActMRApprove} {
		b.Run(string(act), func(b *testing.B) {
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				_ = ab.e.az.Authorize(ctx, ab.dev, act, ab.tree.TenantID, ab.tree)
			}
		})
	}
}

func TestAttrBenchFixture(t *testing.T) {
	ab := newAttrBench()
	ctx := context.Background()
	if err := ab.e.az.Authorize(ctx, ab.dev, domain.ActTreeWrite, ab.tree.TenantID, ab.tree); err != nil {
		t.Fatalf("tree:write: %v", err)
	}
	if err := ab.e.az.Authorize(ctx, ab.dev, domain.ActTreeAdmin, ab.tree.TenantID, ab.tree); err == nil {
		t.Fatal("tree:admin must be denied by the clearance statement")
	}
}
