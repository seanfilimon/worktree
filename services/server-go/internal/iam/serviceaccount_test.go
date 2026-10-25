package iam

import (
	"context"
	"errors"
	"reflect"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// newOrg creates an org tenant directly in the store.
func (e *testEnv) newOrg(t *testing.T, slug string) *domain.Tenant {
	t.Helper()
	ten := &domain.Tenant{ID: "tenant-" + slug, Slug: slug, Name: slug, Kind: domain.TenantOrg, Plan: "free"}
	if err := e.store.CreateTenant(context.Background(), ten); err != nil {
		t.Fatal(err)
	}
	return ten
}

func (e *testEnv) newSA(t *testing.T, ten *domain.Tenant, name, role string) *domain.ServiceAccount {
	t.Helper()
	sa := &domain.ServiceAccount{TenantID: ten.ID, Name: name, Role: role}
	if err := e.store.CreateServiceAccount(context.Background(), sa); err != nil {
		t.Fatal(err)
	}
	return sa
}

func TestServiceAccountKeys(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	acme := e.newOrg(t, "acme")
	sa := e.newSA(t, acme, "ci", domain.RoleDeveloper)

	raw, key, err := e.svc.CreateServiceAccountKey(ctx, sa.ID, "  deploy ", 24*time.Hour, []string{"tree:*"})
	if err != nil {
		t.Fatal(err)
	}
	if key.Kind != domain.TokenService || key.AccountID != "" || key.ServiceAccountID != sa.ID || key.Name != "deploy" ||
		!reflect.DeepEqual(key.Scopes, []string{"tree:*"}) || key.ExpiresAt == nil || !wellFormedToken(raw) {
		t.Fatalf("key %+v", key)
	}
	p, err := e.svc.Authenticate(ctx, raw)
	if err != nil {
		t.Fatal(err)
	}
	want := &domain.Principal{
		Username:         "sa:acme/ci",
		TenantID:         acme.ID,
		Kind:             domain.PrincipalServiceAccount,
		ServiceAccountID: sa.ID,
		Scopes:           []string{"tree:*"},
		TokenID:          key.ID,
	}
	if !reflect.DeepEqual(p, want) {
		t.Fatalf("principal = %+v, want %+v", p, want)
	}
	if !p.IsServiceAccount() {
		t.Fatal("IsServiceAccount false")
	}

	// Default name; scopes validated; ttl validated.
	if _, k, err := e.svc.CreateServiceAccountKey(ctx, sa.ID, "", 0, nil); err != nil || k.Name != "key" || k.ExpiresAt != nil {
		t.Fatalf("default key = %+v, %v", k, err)
	}
	if _, _, err := e.svc.CreateServiceAccountKey(ctx, sa.ID, "x", 0, []string{"bogus:*"}); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad scope = %v", err)
	}
	if _, _, err := e.svc.CreateServiceAccountKey(ctx, sa.ID, "x", -time.Hour, nil); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("negative ttl = %v", err)
	}
	if _, _, err := e.svc.CreateServiceAccountKey(ctx, "nope", "x", 0, nil); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unknown sa = %v", err)
	}
	if _, _, err := e.svc.CreateServiceAccountKey(ctx, "", "x", 0, nil); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("empty sa = %v", err)
	}

	// Disabling the service account invalidates its keys and blocks new ones.
	sa.Disabled = true
	_ = e.store.UpdateServiceAccount(ctx, sa)
	if _, err := e.svc.Authenticate(ctx, raw); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("disabled sa key = %v", err)
	}
	if _, _, err := e.svc.CreateServiceAccountKey(ctx, sa.ID, "x", 0, nil); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("key for disabled sa = %v", err)
	}
	sa.Disabled = false
	_ = e.store.UpdateServiceAccount(ctx, sa)
	if _, err := e.svc.Authenticate(ctx, raw); err != nil {
		t.Fatalf("re-enabled sa key = %v", err)
	}

	// Revocation is scoped to the owning service account.
	other := e.newSA(t, acme, "other", domain.RoleViewer)
	if err := e.svc.RevokeServiceAccountKey(ctx, other.ID, key.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("revoke via other sa = %v", err)
	}
	if err := e.svc.RevokeServiceAccountKey(ctx, sa.ID, "missing"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("revoke unknown key = %v", err)
	}
	if err := e.svc.RevokeServiceAccountKey(ctx, sa.ID, key.ID); err != nil {
		t.Fatal(err)
	}
	if _, err := e.svc.Authenticate(ctx, raw); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("revoked key = %v", err)
	}
}

func TestServiceAccountKeyEdgeCases(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	acme := e.newOrg(t, "acme")
	sa := e.newSA(t, acme, "ci", domain.RoleDeveloper)
	alice := e.register(t, "alice")

	raw, key, _ := e.svc.CreateServiceAccountKey(ctx, sa.ID, "k", 0, nil)
	// A record naming both an account and a service account is rejected.
	e.store.mu.Lock()
	tk := e.store.tokens[key.ID]
	tk.AccountID = alice.ID
	e.store.tokens[key.ID] = tk
	e.store.mu.Unlock()
	if _, err := e.svc.Authenticate(ctx, raw); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("ambiguous key = %v", err)
	}
	// Kind service without a service account id is rejected.
	tk.AccountID, tk.ServiceAccountID = alice.ID, ""
	e.store.mu.Lock()
	e.store.tokens[key.ID] = tk
	e.store.mu.Unlock()
	if _, err := e.svc.Authenticate(ctx, raw); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("service kind without sa = %v", err)
	}

	// Deleted service account.
	raw2, _, _ := e.svc.CreateServiceAccountKey(ctx, sa.ID, "k2", 0, nil)
	e.store.mu.Lock()
	delete(e.store.iam().sas, sa.ID)
	e.store.mu.Unlock()
	if _, err := e.svc.Authenticate(ctx, raw2); !errors.Is(err, domain.ErrUnauthenticated) {
		t.Fatalf("deleted sa = %v", err)
	}
	// Service-account keys are not personal tokens.
	if toks, _ := e.svc.ListSessions(ctx, ""); len(toks) != 0 {
		t.Fatalf("ListSessions(\"\") = %+v", toks)
	}
}

func TestAuthorizeServiceAccount(t *testing.T) {
	ctx := context.Background()
	e := newTestEnv(t, nil)
	acme := e.newOrg(t, "acme")
	other := e.newOrg(t, "other")
	sa := e.newSA(t, acme, "ci", domain.RoleDeveloper)
	raw, _, _ := e.svc.CreateServiceAccountKey(ctx, sa.ID, "k", 0, nil)
	p, err := e.svc.Authenticate(ctx, raw)
	if err != nil {
		t.Fatal(err)
	}
	own := &domain.Tree{ID: "t1", TenantID: acme.ID, Visibility: domain.VisibilityPrivate}
	foreign := &domain.Tree{ID: "t2", TenantID: other.ID, Visibility: domain.VisibilityPrivate}
	internal := &domain.Tree{ID: "t3", TenantID: other.ID, Visibility: domain.VisibilityInternal}
	tests := []struct {
		act  domain.Action
		tree *domain.Tree
		ok   bool
	}{
		{domain.ActTreeRead, own, true},
		{domain.ActTreeWrite, own, true},
		{domain.ActTreeAdmin, own, false},
		{domain.ActTreeRead, foreign, false},
		{domain.ActTreeRead, internal, true},
		{domain.ActTreeWrite, internal, false},
		{domain.ActServerAdmin, nil, false},
	}
	for _, tt := range tests {
		err := e.svc.Authorize(ctx, p, tt.act, "", tt.tree)
		if (err == nil) != tt.ok {
			t.Errorf("Authorize(%s, %v) = %v, want ok=%v", tt.act, tt.tree, err, tt.ok)
		}
		if err != nil && !errors.Is(err, domain.ErrForbidden) {
			t.Errorf("Authorize(%s) error %v does not wrap ErrForbidden", tt.act, err)
		}
	}
	sa.Disabled = true
	_ = e.store.UpdateServiceAccount(ctx, sa)
	if err := e.svc.Authorize(ctx, p, domain.ActTreeRead, "", own); !errors.Is(err, domain.ErrForbidden) {
		t.Fatalf("disabled sa = %v", err)
	}
}
