package iam_test

import (
	"context"
	"testing"

	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/iam"
)

func TestAllowAllAuthorizer(t *testing.T) {
	a := iam.AllowAllAuthorizer{}
	p := auth.Principal{Tenant: "acme", Account: "alice"}
	decision, err := a.Authorize(context.Background(), p, "staged:create", "acme/wt/main/snap-1")
	if err != nil {
		t.Fatalf("Authorize: %v", err)
	}
	if decision != iam.Allow {
		t.Errorf("AllowAllAuthorizer returned %v, want Allow", decision)
	}
}

func TestDenyAllAuthorizer(t *testing.T) {
	a := iam.DenyAllAuthorizer{}
	p := auth.Principal{Tenant: "acme", Account: "alice"}
	decision, err := a.Authorize(context.Background(), p, "staged:create", "acme/wt/main/snap-1")
	if err != nil {
		t.Fatalf("Authorize: %v", err)
	}
	if decision != iam.Deny {
		t.Errorf("DenyAllAuthorizer returned %v, want Deny", decision)
	}
}

func TestPolicyAuthorizer_DefaultDeny(t *testing.T) {
	a := iam.NewPolicyAuthorizer(nil)
	p := auth.Principal{Tenant: "acme", Account: "alice", Authenticated: true, Scopes: []string{"staged:*"}}
	decision, err := a.Authorize(context.Background(), p, "staged:create", "acme/wt/main/snap-1")
	if err != nil {
		t.Fatalf("Authorize: %v", err)
	}
	if decision != iam.Deny {
		t.Fatalf("decision = %v, want deny", decision)
	}
}

func TestPolicyAuthorizer_DenyOverridesAllow(t *testing.T) {
	a := iam.NewPolicyAuthorizer(map[string][]iam.PolicyRule{
		"acme": {
			{Effect: iam.EffectAllow, Tenant: "acme", Actions: []string{"staged:*"}, Resources: []string{"tenant:acme/*"}},
			{Effect: iam.EffectDeny, Tenant: "acme", Account: "alice", Actions: []string{"staged:create"}, Resources: []string{"tenant:acme/api/*"}},
		},
	})
	p := auth.Principal{Tenant: "acme", Account: "alice", Authenticated: true}
	decision, err := a.Authorize(context.Background(), p, "staged:create", "acme/api/main/snap-1")
	if err != nil {
		t.Fatalf("Authorize: %v", err)
	}
	if decision != iam.Deny {
		t.Fatalf("decision = %v, want deny", decision)
	}
}

func TestParsePolicies(t *testing.T) {
	data := []byte(`{"rules":[{"effect":"allow","tenant":"acme","actions":["staged:list"],"resources":["tenant:acme/*"]}]}`)
	rules, err := iam.ParsePolicies(data)
	if err != nil {
		t.Fatalf("ParsePolicies: %v", err)
	}
	if len(rules) != 1 || rules[0].Effect != iam.EffectAllow {
		t.Fatalf("rules = %#v", rules)
	}
}

func TestPolicyAuthorizer_UpdateTenantRules(t *testing.T) {
	a := iam.NewPolicyAuthorizer(nil)
	p := auth.Principal{Tenant: "acme", Account: "alice", Authenticated: true}

	// Should deny initially
	decision, err := a.Authorize(context.Background(), p, "staged:list", "acme/api/main/snap-1")
	if err != nil || decision != iam.Deny {
		t.Fatalf("Expected deny, got %v", decision)
	}

	// Update rules
	a.UpdateTenantRules("acme", []iam.PolicyRule{
		{Effect: iam.EffectAllow, Tenant: "acme", Actions: []string{"staged:list"}, Resources: []string{"tenant:acme/*"}},
	})

	// Should allow now
	decision, err = a.Authorize(context.Background(), p, "staged:list", "acme/api/main/snap-1")
	if err != nil || decision != iam.Allow {
		t.Fatalf("Expected allow, got %v", decision)
	}
}
