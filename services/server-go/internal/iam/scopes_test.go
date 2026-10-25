package iam

import (
	"context"
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestValidateScopes(t *testing.T) {
	tooMany := make([]string, maxScopes+1)
	for i := range tooMany {
		tooMany[i] = "tree:read"
	}
	tests := []struct {
		name string
		in   []string
		want []string
		ok   bool
	}{
		{"nil", nil, nil, true},
		{"empty", []string{}, nil, true},
		{"concrete", []string{"tree:read", "mr:create"}, []string{"tree:read", "mr:create"}, true},
		{"area wildcard", []string{"tree:*", "iam:*"}, []string{"tree:*", "iam:*"}, true},
		{"star", []string{"*"}, []string{"*"}, true},
		{"normalized and deduplicated", []string{" Tree:Read ", "tree:read", "branch:push"}, []string{"tree:read", "branch:push"}, true},
		{"unknown area", []string{"foo:*"}, nil, false},
		{"unknown action", []string{"tree:fly"}, nil, false},
		{"unknown area action", []string{"repo:read"}, nil, false},
		{"no verb", []string{"tree"}, nil, false},
		{"empty verb", []string{"tree:"}, nil, false},
		{"empty area", []string{":read"}, nil, false},
		{"empty scope", []string{""}, nil, false},
		{"double star", []string{"**"}, nil, false},
		{"glob", []string{"tree:re*"}, nil, false},
		{"too long", []string{"tree:" + string(make([]byte, 80))}, nil, false},
		{"too many", tooMany, nil, false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got, err := ValidateScopes(tt.in)
			if tt.ok != (err == nil) {
				t.Fatalf("ValidateScopes(%q) = %v", tt.in, err)
			}
			if !tt.ok {
				if !errors.Is(err, domain.ErrInvalid) {
					t.Fatalf("error %v does not wrap ErrInvalid", err)
				}
				return
			}
			if !reflect.DeepEqual(got, tt.want) {
				t.Fatalf("ValidateScopes(%q) = %q, want %q", tt.in, got, tt.want)
			}
		})
	}
	// Every catalog action is a valid scope.
	for _, a := range KnownActions() {
		if err := ValidateScopePattern(string(a)); err != nil {
			t.Errorf("%s: %v", a, err)
		}
		if !IsKnownAction(a) {
			t.Errorf("IsKnownAction(%s) = false", a)
		}
	}
}

func TestScopeAllows(t *testing.T) {
	tests := []struct {
		scopes []string
		act    domain.Action
		want   bool
	}{
		{nil, domain.ActTreeAdmin, true},
		{[]string{"tree:read"}, domain.ActTreeRead, true},
		{[]string{"tree:read"}, domain.ActTreeWrite, false},
		{[]string{"tree:*"}, domain.ActTreeAdmin, true},
		{[]string{"tree:*"}, domain.ActTenantRead, false},
		{[]string{"tree:*"}, domain.ActBranchPush, true}, // via tree:write
		{[]string{"tree:write"}, domain.ActBranchPush, true},
		{[]string{"branch:push"}, domain.ActTreeWrite, false},
		{[]string{"*"}, domain.ActServerAdmin, true},
		{[]string{"mr:*", "tenant:read"}, domain.ActTenantRead, true},
		{[]string{"mr:*", "tenant:read"}, domain.ActIAMRead, false},
	}
	for _, tt := range tests {
		if got := ScopeAllows(tt.scopes, tt.act); got != tt.want {
			t.Errorf("ScopeAllows(%q, %s) = %v, want %v", tt.scopes, tt.act, got, tt.want)
		}
	}
}

func TestCreateScopedToken(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	raw, rec, err := e.svc.CreateScopedToken(ctx, a.ID, "ci", time.Hour, []string{"tree:read", "Tree:Read", "mr:*"})
	if err != nil {
		t.Fatal(err)
	}
	if rec.Kind != domain.TokenAPI || !reflect.DeepEqual(rec.Scopes, []string{"tree:read", "mr:*"}) || rec.ExpiresAt == nil {
		t.Fatalf("token %+v", rec)
	}
	p, err := e.svc.Authenticate(ctx, raw)
	if err != nil || !reflect.DeepEqual(p.Scopes, []string{"tree:read", "mr:*"}) || p.TokenID != rec.ID || p.MFA {
		t.Fatalf("Authenticate = %+v, %v", p, err)
	}
	// The principal does not alias the stored slice.
	p.Scopes[0] = "*"
	if tok, _ := e.store.token(rec.ID); tok.Scopes[0] != "tree:read" {
		t.Fatal("principal scopes alias the token record")
	}

	// Unrestricted CreateToken is Kind api without scopes.
	_, plain, _ := e.svc.CreateToken(ctx, a.ID, "plain", 0)
	if plain.Kind != domain.TokenAPI || plain.Scopes != nil {
		t.Fatalf("CreateToken = %+v", plain)
	}

	for _, bad := range [][]string{{"nope:*"}, {"tree:fly"}, {"tree"}} {
		if _, _, err := e.svc.CreateScopedToken(ctx, a.ID, "x", 0, bad); !errors.Is(err, domain.ErrInvalid) {
			t.Fatalf("scopes %q = %v, want ErrInvalid", bad, err)
		}
	}
	if _, _, err := e.svc.CreateScopedToken(ctx, a.ID, "", 0, nil); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty name = %v", err)
	}
	if _, _, err := e.svc.CreateScopedToken(ctx, a.ID, "x", -time.Second, nil); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("negative ttl = %v", err)
	}
	if _, _, err := e.svc.CreateScopedToken(ctx, "nope", "x", 0, nil); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown account = %v", err)
	}
}

func TestAuthorizeEnforcesScopes(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	a := e.register(t, "alice")
	tree := &domain.Tree{ID: "t1", TenantID: a.TenantID, Visibility: domain.VisibilityPrivate}
	raw, _, err := e.svc.CreateScopedToken(ctx, a.ID, "ro", 0, []string{"tree:read"})
	if err != nil {
		t.Fatal(err)
	}
	p, _ := e.svc.Authenticate(ctx, raw)
	if err := e.svc.Authorize(ctx, p, domain.ActTreeRead, "", tree); err != nil {
		t.Fatalf("in-scope read = %v", err)
	}
	if err := e.svc.Authorize(ctx, p, domain.ActTreeWrite, "", tree); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("out-of-scope write = %v", err)
	}
	// Scopes bind server admins too.
	a.IsAdmin = true
	_ = e.store.UpdateAccount(ctx, a)
	p, _ = e.svc.Authenticate(ctx, raw)
	if err := e.svc.Authorize(ctx, p, domain.ActServerAdmin, "", nil); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("admin out-of-scope = %v", err)
	}
}
