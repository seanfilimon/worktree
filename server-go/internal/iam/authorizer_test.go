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
