package iam_test

import (
	"context"
	"os"
	"path/filepath"
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
	a := iam.NewPolicyAuthorizer([]iam.PolicyRule{
		{Effect: iam.EffectAllow, Tenant: "acme", Actions: []string{"staged:*"}, Resources: []string{"tenant:acme/*"}},
		{Effect: iam.EffectDeny, Tenant: "acme", Account: "alice", Actions: []string{"staged:create"}, Resources: []string{"tenant:acme/api/*"}},
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

func TestLoadPolicyFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "policy.json")
	data := []byte(`{"rules":[{"effect":"allow","tenant":"acme","actions":["staged:list"],"resources":["tenant:acme/*"]}]}`)
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatal(err)
	}
	rules, err := iam.LoadPolicyFile(path)
	if err != nil {
		t.Fatalf("LoadPolicyFile: %v", err)
	}
	if len(rules) != 1 || rules[0].Effect != iam.EffectAllow {
		t.Fatalf("rules = %#v", rules)
	}
}
