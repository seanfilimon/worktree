package iam

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strconv"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// deletionReason is the status reason of a tenant scheduled for deletion.
func deletionReason(purgeAt time.Time) string {
	return "scheduled for deletion on " + purgeAt.UTC().Format("2006-01-02 15:04 MST")
}

// Deletion returns the scheduled deletion of tenantID (ErrNotFound when
// none is scheduled).
func (l *TenantLifecycle) Deletion(ctx context.Context, tenantID string) (*TenantDeletion, error) {
	var d TenantDeletion
	if err := l.d.Store.GetDoc(ctx, CollTenantDeletions, tenantID, &d); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, lifecycleErr(domain.ErrNotFound, CodeNotScheduled, "no deletion is scheduled for this tenant")
		}
		return nil, err
	}
	return &d, nil
}

func (l *TenantLifecycle) checkNotScheduled(ctx context.Context, t *domain.Tenant) error {
	d, err := l.Deletion(ctx, t.ID)
	if err == nil {
		return lifecycleErr(domain.ErrConflict, CodeDeletionScheduled,
			"tenant %s is scheduled for deletion on %s; restore it first", t.Slug, d.PurgeAt.Format(time.RFC3339))
	}
	if errors.Is(err, domain.ErrNotFound) {
		return nil
	}
	return err
}

// Delete schedules the tenant's deletion: it starts the recovery window
// (DeletionWindow), suspends the tenant (read-only for everyone but server
// admins) and revokes its service-account keys. The purge job removes the
// tenant once PurgeAt passes; Restore cancels it. Personal tenants cannot
// be deleted.
func (l *TenantLifecycle) Delete(ctx context.Context, actor string, t *domain.Tenant) (*TenantDeletion, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	cur, err := l.d.Store.GetTenant(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	if cur.Kind == domain.TenantUser {
		return nil, lifecycleErr(domain.ErrConflict, CodePersonalTenant,
			"%s is a personal tenant: it is deleted with its account, not on its own", cur.Slug)
	}
	if err := l.checkNotScheduled(ctx, cur); err != nil {
		return nil, err
	}
	now := l.now()
	d := &TenantDeletion{
		TenantID:       cur.ID,
		Slug:           cur.Slug,
		RequestedBy:    actor,
		RequestedAt:    now,
		PurgeAt:        now.Add(l.o.DeletionWindow),
		PreviousStatus: cur.Status,
		PreviousReason: cur.StatusReason,
	}
	if err := l.d.Store.CreateDoc(ctx, CollTenantDeletions, cur.ID, d); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return nil, lifecycleErr(domain.ErrConflict, CodeDeletionScheduled, "tenant %s is already scheduled for deletion", cur.Slug)
		}
		return nil, err
	}
	cur.Status = domain.TenantSuspended
	cur.StatusReason = deletionReason(d.PurgeAt)
	if err := l.d.Store.UpdateTenant(ctx, cur); err != nil {
		_ = l.d.Store.DeleteDoc(ctx, CollTenantDeletions, cur.ID)
		return nil, err
	}
	n, err := l.RevokeServiceAccountKeys(ctx, cur.ID)
	if err != nil {
		l.d.Logger.WarnContext(ctx, "tenant delete: revoking service-account keys failed", "tenant", cur.Slug, "err", err)
	}
	d.RevokedKeys = n
	_ = l.d.Store.PutDoc(ctx, CollTenantDeletions, cur.ID, d)
	return d, nil
}

// Restore cancels a scheduled deletion during the recovery window: the
// tenant gets back the status it had before (normally active). Revoked
// service-account keys stay revoked.
func (l *TenantLifecycle) Restore(ctx context.Context, t *domain.Tenant) (*domain.Tenant, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	d, err := l.Deletion(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	cur, err := l.d.Store.GetTenant(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	cur.Status, cur.StatusReason = d.PreviousStatus, d.PreviousReason
	if cur.Status == domain.TenantActive {
		cur.Status = ""
	}
	if err := l.d.Store.UpdateTenant(ctx, cur); err != nil {
		return nil, err
	}
	if err := l.d.Store.DeleteDoc(ctx, CollTenantDeletions, cur.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
		return nil, err
	}
	return cur, nil
}

// RevokeServiceAccountKeys deletes every key of the tenant's service
// accounts and returns how many were deleted. It runs whenever a tenant is
// suspended or deactivated (deletion, or a server admin's status change).
func (l *TenantLifecycle) RevokeServiceAccountKeys(ctx context.Context, tenantID string) (int, error) {
	sas, err := l.d.Store.ListServiceAccounts(ctx, tenantID)
	if err != nil {
		return 0, err
	}
	n := 0
	for _, sa := range sas {
		keys, err := l.d.Store.ListServiceAccountTokens(ctx, sa.ID)
		if err != nil {
			return n, err
		}
		for _, k := range keys {
			if err := l.d.Store.DeleteToken(ctx, k.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
				return n, err
			}
			n++
		}
	}
	return n, nil
}

// ---- purge job ---------------------------------------------------------

// Start runs the purge job every PurgeInterval until ctx ends: tenants
// whose recovery window passed are purged, expired verification codes and
// slug reservations are dropped. It returns immediately.
func (l *TenantLifecycle) Start(ctx context.Context) {
	go func() {
		tick := time.NewTicker(l.o.PurgeInterval)
		defer tick.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case <-tick.C:
				if _, err := l.RunOnce(ctx); err != nil && ctx.Err() == nil {
					l.d.Logger.WarnContext(ctx, "tenant purge job failed", "err", err)
				}
			}
		}
	}()
}

// RunOnce purges every tenant whose recovery window has passed and prunes
// expired codes and reservations. It returns the purged tenant ids.
func (l *TenantLifecycle) RunOnce(ctx context.Context) ([]string, error) {
	raws, err := l.d.Store.ListDocs(ctx, CollTenantDeletions, "")
	if err != nil {
		return nil, err
	}
	now := l.now()
	var purged []string
	var errs []error
	for _, raw := range raws {
		var d TenantDeletion
		if json.Unmarshal(raw, &d) != nil || d.TenantID == "" || now.Before(d.PurgeAt) {
			continue
		}
		if err := l.Purge(ctx, d.TenantID); err != nil {
			errs = append(errs, fmt.Errorf("purging tenant %s: %w", d.Slug, err))
			continue
		}
		purged = append(purged, d.TenantID)
	}
	if err := l.prune(ctx, now); err != nil {
		errs = append(errs, err)
	}
	return purged, errors.Join(errs...)
}

// prune drops expired (or long used) verification codes and expired slug
// reservations.
func (l *TenantLifecycle) prune(ctx context.Context, now time.Time) error {
	keys, err := l.tokenKeys(ctx, func(t EmailToken) bool { return !now.Before(t.ExpiresAt) })
	if err != nil {
		return err
	}
	for _, k := range keys {
		_ = l.d.Store.DeleteDoc(ctx, CollEmailTokens, k)
	}
	raws, err := l.d.Store.ListDocs(ctx, CollTenantSlugs, "")
	if err != nil {
		return err
	}
	for _, raw := range raws {
		var r SlugReservation
		if json.Unmarshal(raw, &r) == nil && r.Slug != "" && !now.Before(r.ExpiresAt) {
			_ = l.d.Store.DeleteDoc(ctx, CollTenantSlugs, r.Slug)
		}
	}
	return nil
}

// tenantDocCollections are tenant-scoped collections keyed by tenant id or
// DocKey(tenantID, ...) whose documents the purge removes directly; the
// owners of other collections clean up on tree.deleted / tenant.deleted.
var tenantDocCollections = []string{"access_docs", "linked_groups"}

// Purge deletes the tenant now: its trees (tree.deleted is published for
// each, so tree-scoped documents are cleaned up and GC reclaims the
// objects), teams, service accounts and their keys, custom roles,
// policies, invitations, memberships, tenant grants naming it on other
// tenants' trees, tenant documents and the tenant record. The final slug
// stays reserved for SlugReservationPeriod. Publishes tenant.deleted
// {slug, trees} and audits tenant.purge. Safe to retry after a failure.
func (l *TenantLifecycle) Purge(ctx context.Context, tenantID string) error {
	st := l.d.Store
	t, err := st.GetTenant(ctx, tenantID)
	if errors.Is(err, domain.ErrNotFound) {
		_ = st.DeleteDoc(ctx, CollTenantDeletions, tenantID)
		return nil
	}
	if err != nil {
		return err
	}
	trees, err := st.ListTrees(ctx, t.ID)
	if err != nil {
		return err
	}
	for _, tr := range trees {
		if err := st.DeleteTree(ctx, tr.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("deleting tree %s: %w", tr.Name, err)
		}
		l.publish(domain.Event{Type: domain.EventTreeDeleted, TenantID: t.ID, TreeID: tr.ID, Actor: "system",
			Payload: map[string]any{"name": tr.Name}})
	}
	teams, err := st.ListTeams(ctx, t.ID)
	if err != nil {
		return err
	}
	for _, tm := range teams {
		if err := st.DeleteTeam(ctx, tm.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	sas, err := st.ListServiceAccounts(ctx, t.ID)
	if err != nil {
		return err
	}
	for _, sa := range sas {
		if err := st.DeleteServiceAccount(ctx, sa.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	pols, err := st.ListPolicies(ctx, t.ID)
	if err != nil {
		return err
	}
	for _, p := range pols {
		if err := st.DeletePolicy(ctx, t.ID, p.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	roles, err := st.ListRoles(ctx, t.ID)
	if err != nil {
		return err
	}
	invs, err := st.ListInvitations(ctx, t.ID)
	if err != nil {
		return err
	}
	for _, inv := range invs {
		if err := st.DeleteInvitation(ctx, t.ID, inv.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	members, err := st.ListTenantMembers(ctx, t.ID)
	if err != nil {
		return err
	}
	for _, m := range members {
		if err := st.DeleteMembership(ctx, t.ID, m.AccountID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	// Custom roles go after the memberships that may use them.
	for _, r := range roles {
		if err := st.DeleteRole(ctx, t.ID, r.Name); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	grants, err := st.ListSubjectGrants(ctx, domain.SubjectTenant, t.ID)
	if err != nil {
		return err
	}
	for _, g := range grants {
		if err := st.DeleteGrant(ctx, g.TreeID, g.SubjectType, g.SubjectID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	for _, coll := range tenantDocCollections {
		if _, err := st.DeleteDocs(ctx, coll, domain.DocKey(t.ID)+"\x00"); err != nil {
			return err
		}
	}
	_ = st.DeleteDoc(ctx, "tenant_usage", t.ID)
	if err := l.dropTokens(ctx, EmailSubjectTenant, t.ID); err != nil {
		return err
	}
	now := l.now()
	if err := st.PutDoc(ctx, CollTenantSlugs, t.Slug, &SlugReservation{
		Slug: t.Slug, TenantID: t.ID, ReservedAt: now, ExpiresAt: now.Add(SlugReservationPeriod),
	}); err != nil {
		return err
	}
	if del, ok := st.(TenantDeleter); ok {
		if err := del.DeleteTenant(ctx, t.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	} else {
		// Without tenant deletion the record stays, closed to everyone.
		t.Status, t.StatusReason = domain.TenantDeactivated, "deleted"
		if err := st.UpdateTenant(ctx, t); err != nil {
			return err
		}
	}
	if err := st.DeleteDoc(ctx, CollTenantDeletions, t.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
		return err
	}
	l.publish(domain.Event{Type: domain.EventTenantDeleted, TenantID: t.ID, Actor: "system",
		Payload: map[string]any{"slug": t.Slug, "trees": len(trees)}})
	l.audit(ctx, domain.AuditEvent{
		TenantID: t.ID, Action: "tenant.purge", ResourceType: "tenant", ResourceID: t.ID,
		Details: map[string]string{"slug": t.Slug, "trees": strconv.Itoa(len(trees)), "members": strconv.Itoa(len(members))},
	})
	return nil
}
