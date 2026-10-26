package meta

import (
	"context"

	bolt "go.etcd.io/bbolt"
)

// Tenant deletion (tenant-lifecycle, docs/ENGINEERING_PLAN.md §16.4).
//
// domain.MetaStore has no tenant deletion; the purge job of
// iam.TenantLifecycle uses this optional method (TenantDeleter) after it
// removed the tenant's trees, memberships, teams, service accounts, roles,
// policies, invitations and documents. DeleteTenant removes only the tenant
// record and its slug index entry.

// DeleteTenant removes the tenant record (ErrNotFound when missing).
func (m *Memory) DeleteTenant(ctx context.Context, id string) error {
	return m.write(ctx, func() error {
		t, ok := m.tenants[id]
		if !ok {
			return notFound("tenant", id)
		}
		delete(m.tenants, id)
		if m.tenantBySlug[t.Slug] == id {
			delete(m.tenantBySlug, t.Slug)
		}
		return nil
	})
}

// DeleteTenant removes the tenant record (ErrNotFound when missing).
func (b *Bolt) DeleteTenant(ctx context.Context, id string) error {
	return b.update(ctx, func(tx *bolt.Tx) error {
		t, err := getTenantTx(tx, id)
		if err != nil {
			return err
		}
		ib := tx.Bucket(bktTenantSlug)
		if string(ib.Get([]byte(t.Slug))) == id {
			if err := ib.Delete([]byte(t.Slug)); err != nil {
				return err
			}
		}
		return tx.Bucket(bktTenants).Delete([]byte(id))
	})
}
