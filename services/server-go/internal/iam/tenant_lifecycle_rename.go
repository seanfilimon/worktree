package iam

import (
	"context"
	"errors"
	"fmt"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Rename changes the tenant's slug (docs/ENGINEERING_PLAN.md §16.4). The
// old slug is reserved for SlugReservationPeriod: requests naming it are
// answered with a "moved" error carrying the new slug (MovedSlug) and no
// other tenant or account can claim it. References that store the slug are
// rewritten: policy resources (wt:tenant/<slug>, wt:tree/<slug>/...,
// wt:branch/..., wt:path/...) of the tenant and tenant:<slug> principals of
// every tenant, license-rule grantees and the display name of tenant
// grants. Personal tenants (slug = username) cannot be renamed. Publishes
// tenant.renamed {old_slug, new_slug}.
func (l *TenantLifecycle) Rename(ctx context.Context, actor string, t *domain.Tenant, newSlug string) (*domain.Tenant, *SlugReservation, error) {
	newSlug = strings.TrimSpace(newSlug)
	if err := ValidateTenantSlug(newSlug); err != nil {
		return nil, nil, fmt.Errorf("iam: %w", err)
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	cur, err := l.d.Store.GetTenant(ctx, t.ID)
	if err != nil {
		return nil, nil, err
	}
	if cur.Kind == domain.TenantUser {
		return nil, nil, lifecycleErr(domain.ErrConflict, CodePersonalTenant,
			"%s is a personal tenant: its slug is the account's username and cannot be changed", cur.Slug)
	}
	if err := l.checkNotScheduled(ctx, cur); err != nil {
		return nil, nil, err
	}
	old := cur.Slug
	if newSlug == old {
		return nil, nil, fmt.Errorf("iam: tenant is already called %q: %w", newSlug, domain.ErrInvalid)
	}
	if _, err := l.d.Store.GetTenantBySlug(ctx, newSlug); err == nil {
		return nil, nil, lifecycleErr(domain.ErrConflict, CodeSlugTaken, "tenant %q already exists", newSlug)
	} else if !errors.Is(err, domain.ErrNotFound) {
		return nil, nil, err
	}
	if _, err := l.d.Store.GetAccountByUsername(ctx, newSlug); err == nil {
		return nil, nil, lifecycleErr(domain.ErrConflict, CodeSlugTaken, "%q is an account's username", newSlug)
	} else if !errors.Is(err, domain.ErrNotFound) {
		return nil, nil, err
	}
	if reserved, err := slugReservedAt(ctx, l.d.Store, newSlug, cur.ID, l.now()); err != nil {
		return nil, nil, err
	} else if reserved {
		return nil, nil, lifecycleErr(domain.ErrConflict, CodeSlugReserved,
			"slug %q is reserved: it was used by another tenant in the last %d days", newSlug, int(SlugReservationPeriod.Hours()/24))
	}

	cur.Slug = newSlug
	if err := l.d.Store.UpdateTenant(ctx, cur); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return nil, nil, lifecycleErr(domain.ErrConflict, CodeSlugTaken, "tenant %q already exists", newSlug)
		}
		return nil, nil, err
	}
	now := l.now()
	res := &SlugReservation{Slug: old, TenantID: cur.ID, RenamedTo: newSlug, ReservedAt: now, ExpiresAt: now.Add(SlugReservationPeriod)}
	if err := l.d.Store.PutDoc(ctx, CollTenantSlugs, old, res); err != nil {
		return nil, nil, fmt.Errorf("iam: reserving slug %q: %w", old, err)
	}
	// Renaming back to an earlier slug of the same tenant frees it.
	if err := l.d.Store.DeleteDoc(ctx, CollTenantSlugs, newSlug); err != nil && !errors.Is(err, domain.ErrNotFound) {
		return nil, nil, err
	}
	if err := l.rewriteSlugReferences(ctx, cur.ID, old, newSlug); err != nil {
		l.d.Logger.ErrorContext(ctx, "tenant rename: rewriting references failed", "tenant", cur.ID, "old", old, "new", newSlug, "err", err)
		return nil, nil, err
	}
	l.publish(domain.Event{
		Type:     domain.EventTenantRenamed,
		TenantID: cur.ID,
		Actor:    actor,
		Payload:  map[string]any{"old_slug": old, "new_slug": newSlug},
	})
	return cur, res, nil
}

// rewriteSlugReferences replaces old with newSlug wherever a slug is
// stored by value.
func (l *TenantLifecycle) rewriteSlugReferences(ctx context.Context, tenantID, old, newSlug string) error {
	st := l.d.Store
	tenants, err := st.ListTenants(ctx)
	if err != nil {
		return err
	}
	for _, ten := range tenants {
		pols, err := st.ListPolicies(ctx, ten.ID)
		if err != nil {
			return err
		}
		for i := range pols {
			p := pols[i]
			if !rewritePolicy(&p, ten.ID == tenantID, old, newSlug) {
				continue
			}
			if err := st.UpdatePolicy(ctx, &p); err != nil {
				return fmt.Errorf("iam: rewriting policy %s: %w", p.Name, err)
			}
		}
	}
	trees, err := st.ListTrees(ctx, "")
	if err != nil {
		return err
	}
	for _, tr := range trees {
		rules, err := st.ListLicenseRules(ctx, tr.ID)
		if err != nil {
			return err
		}
		for i := range rules {
			if rules[i].Grantee != old {
				continue
			}
			r := rules[i]
			r.Grantee = newSlug
			if err := st.PutLicenseRule(ctx, &r); err != nil {
				return err
			}
		}
	}
	grants, err := st.ListSubjectGrants(ctx, domain.SubjectTenant, tenantID)
	if err != nil {
		return err
	}
	for i := range grants {
		g := grants[i]
		g.Subject = newSlug
		if err := st.PutGrant(ctx, &g); err != nil {
			return err
		}
	}
	return nil
}

// rewritePolicy rewrites slug references of one policy; own reports
// whether the policy belongs to the renamed tenant (only its resources
// name the tenant's slug). It reports whether anything changed.
func rewritePolicy(p *domain.Policy, own bool, old, newSlug string) bool {
	changed := false
	stmts := make([]domain.PolicyStatement, len(p.Statements))
	for i, s := range p.Statements {
		s.Principals = append([]string(nil), s.Principals...)
		for j, pr := range s.Principals {
			if pr == "tenant:"+old {
				s.Principals[j] = "tenant:" + newSlug
				changed = true
			}
		}
		s.Resources = append([]string(nil), s.Resources...)
		if own {
			for j, r := range s.Resources {
				if nr, ok := rewriteResource(r, old, newSlug); ok {
					s.Resources[j] = nr
					changed = true
				}
			}
		}
		stmts[i] = s
	}
	if changed {
		p.Statements = stmts
	}
	return changed
}

// rewriteResource maps wt:<kind>/<old>[/...] to wt:<kind>/<new>[/...].
func rewriteResource(r, old, newSlug string) (string, bool) {
	for _, kind := range []string{"tenant", "tree", "branch", "path"} {
		prefix := "wt:" + kind + "/" + old
		if r == prefix {
			return "wt:" + kind + "/" + newSlug, true
		}
		if strings.HasPrefix(r, prefix+"/") {
			return "wt:" + kind + "/" + newSlug + r[len(prefix):], true
		}
	}
	return r, false
}
