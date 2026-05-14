package iam_test

import (
	"context"
	"os"
	"testing"

	"github.com/jackc/pgx/v5/pgxpool"
	"github.com/ramizik/worktree/server-go/internal/auth"
	"github.com/ramizik/worktree/server-go/internal/iam"
)

func testDSN(t *testing.T) string {
	t.Helper()
	dsn := os.Getenv("WT_TEST_DATABASE_URL")
	if dsn == "" {
		t.Skip("WT_TEST_DATABASE_URL not set; skipping Postgres tests")
	}
	return dsn
}

func TestDBAuthorizer(t *testing.T) {
	dsn := testDSN(t)
	ctx := context.Background()

	pool, err := pgxpool.New(ctx, dsn)
	if err != nil {
		t.Fatalf("failed to connect to db: %v", err)
	}
	defer pool.Close()

	// Ensure the table exists
	_, err = pool.Exec(ctx, `
		CREATE TABLE IF NOT EXISTS tenant_permissions (
			id SERIAL PRIMARY KEY,
			tenant TEXT NOT NULL,
			effect TEXT NOT NULL,
			account TEXT NOT NULL DEFAULT '',
			action TEXT NOT NULL,
			resource_pattern TEXT NOT NULL,
			conditions JSONB
		)
	`)
	if err != nil {
		t.Fatalf("failed to create table: %v", err)
	}
	// Clean up table to ensure isolation
	_, _ = pool.Exec(ctx, "DELETE FROM tenant_permissions")

	authorizer := iam.NewDBAuthorizer(pool)

	// Set up rules
	rules := []iam.PolicyRule{
		{
			Effect:    iam.EffectAllow,
			Actions:   []string{"staged:create"},
			Resources: []string{"tenant:acme/*"},
		},
	}
	if err := authorizer.UpdateTenantRules(ctx, "acme", rules); err != nil {
		t.Fatalf("UpdateTenantRules: %v", err)
	}

	p := auth.Principal{Tenant: "acme", Account: "alice", Authenticated: true}

	decision, err := authorizer.Authorize(ctx, p, "staged:create", "acme/wt/main/snap-1")
	if err != nil {
		t.Fatalf("Authorize: %v", err)
	}
	if decision != iam.Allow {
		t.Errorf("Authorize returned %v, want Allow", decision)
	}

	// Should deny for a different action
	decision, err = authorizer.Authorize(ctx, p, "staged:delete", "acme/wt/main/snap-1")
	if err != nil {
		t.Fatalf("Authorize: %v", err)
	}
	if decision != iam.Deny {
		t.Errorf("Authorize returned %v, want Deny", decision)
	}
}
