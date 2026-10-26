package meta

import (
	"context"
	"errors"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// tenant-lifecycle (§16.4): DeleteTenant removes the record and its slug.
func TestDeleteTenant(t *testing.T) {
	for name, st := range map[string]domain.MetaStore{"memory": NewMemory(), "bolt": newBoltForTest(t)} {
		t.Run(name, func(t *testing.T) {
			ctx := context.Background()
			del, ok := st.(interface {
				DeleteTenant(context.Context, string) error
			})
			if !ok {
				t.Fatal("store has no DeleteTenant")
			}
			ten := &domain.Tenant{ID: "t1", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, Plan: "free", CreatedAt: time.Now()}
			if err := st.CreateTenant(ctx, ten); err != nil {
				t.Fatal(err)
			}
			if err := del.DeleteTenant(ctx, "t1"); err != nil {
				t.Fatal(err)
			}
			if _, err := st.GetTenant(ctx, "t1"); !errors.Is(err, domain.ErrNotFound) {
				t.Fatalf("GetTenant = %v", err)
			}
			if _, err := st.GetTenantBySlug(ctx, "acme"); !errors.Is(err, domain.ErrNotFound) {
				t.Fatalf("GetTenantBySlug = %v", err)
			}
			if ts, _ := st.ListTenants(ctx); len(ts) != 0 {
				t.Fatalf("ListTenants = %+v", ts)
			}
			if err := del.DeleteTenant(ctx, "t1"); !errors.Is(err, domain.ErrNotFound) {
				t.Fatalf("second delete = %v", err)
			}
			// The slug can be used again.
			if err := st.CreateTenant(ctx, &domain.Tenant{ID: "t2", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, Plan: "free", CreatedAt: time.Now()}); err != nil {
				t.Fatal(err)
			}
		})
	}
}
