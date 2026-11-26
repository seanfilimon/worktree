package meta

import (
	"context"
	"testing"
)

// TestPolicyGenerationMovesOnEveryPolicyWrite: authz reuses compiled
// policies while the generation is unchanged, so every policy write must
// move it — and reads must not.
func TestPolicyGenerationMovesOnEveryPolicyWrite(t *testing.T) {
	ctx := context.Background()
	b := newBoltForTest(t).(*Bolt)
	gen := b.PolicyGeneration()
	moved := func(what string) {
		t.Helper()
		if g := b.PolicyGeneration(); g == gen {
			t.Fatalf("%s did not move the policy generation", what)
		} else {
			gen = g
		}
	}
	p := samplePolicy()
	must(t, b.CreatePolicy(ctx, p))
	moved("CreatePolicy")
	if _, err := b.ListPolicies(ctx, p.TenantID); err != nil {
		t.Fatal(err)
	}
	if _, err := b.GetPolicy(ctx, p.TenantID, p.ID); err != nil {
		t.Fatal(err)
	}
	if b.PolicyGeneration() != gen {
		t.Fatal("reads moved the policy generation")
	}
	p.Enabled = !p.Enabled
	must(t, b.UpdatePolicy(ctx, p))
	moved("UpdatePolicy")
	must(t, b.DeletePolicy(ctx, p.TenantID, p.ID))
	moved("DeletePolicy")
}
