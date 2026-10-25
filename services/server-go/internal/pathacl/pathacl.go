// Package pathacl enforces path-scoped access policies
// (docs/ENGINEERING_PLAN.md §13.1).
//
// IAM policies may name file paths of a tree with
// "wt:path/<tenant>/<tree>/<path>" resources ("*" and "**" wildcards). Only
// explicit denies matter at path level: a principal that may read (write)
// a tree may read (write) every path in it unless an enabled deny
// statement of the tree's tenant covers that path for tree:read
// (tree:write).
//
// Checker is the push side (a domain.PushChecker: every changed path is
// evaluated for tree:write). DeniedPaths is the shared primitive used by
// Pull (stubbed paths) and the REST read endpoints.
package pathacl

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Deps are the collaborators of the pathacl package. Store is required.
// Path policies are only evaluated when Authz is also a
// domain.AccessEvaluator (otherwise the checker allows every push).
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Checker rejects pushes that change a path an access policy denies the
// pusher to write. It implements domain.PushChecker.
type Checker struct {
	d Deps
}

var _ domain.PushChecker = (*Checker)(nil)

// New returns a Checker.
func New(d Deps) *Checker {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Checker{d: d}
}

// CheckPushPlan evaluates tree:write on every changed path (deletions
// included) and rejects the push with
// *domain.PushDenied{Kind: "access_denied"} naming the first denied path
// and the denying policy.
func (c *Checker) CheckPushPlan(ctx context.Context, p *domain.Principal, tree *domain.Tree, plan *domain.PushPlan) error {
	ev, ok := c.d.Authz.(domain.AccessEvaluator)
	if !ok || plan == nil || len(plan.ChangedPaths) == 0 {
		return nil
	}
	denied, err := DeniedPaths(ctx, c.d.Store, ev, p, tree, domain.ActTreeWrite, plan.ChangedPaths)
	if err != nil {
		return fmt.Errorf("pathacl: %w", err)
	}
	for _, path := range plan.ChangedPaths {
		if policy, ok := denied[path]; ok {
			return &domain.PushDenied{
				Kind:   "access_denied",
				Path:   path,
				Reason: fmt.Sprintf("writing %s is denied by policy %s", path, policy),
			}
		}
	}
	return nil
}

// DeniedPaths returns the paths among paths for which an access policy
// explicitly denies act to p on tree, mapped to the name of the denying
// policy ("" when the decision names none). Implicit denies are ignored:
// callers have already established that p may perform act on the tree.
//
// When store is non-nil and no enabled policy of the tree's tenant has a
// deny statement on a "wt:path/" resource, no path is evaluated (the
// common case costs one cached policy listing). ev nil means no path
// policies (nil map).
func DeniedPaths(ctx context.Context, store domain.MetaStore, ev domain.AccessEvaluator, p *domain.Principal, tree *domain.Tree, act domain.Action, paths []string) (map[string]string, error) {
	if ev == nil || tree == nil || len(paths) == 0 {
		return nil, nil
	}
	if store != nil {
		has, err := hasPathDenies(ctx, store, tree.TenantID)
		if err != nil {
			return nil, err
		}
		if !has {
			return nil, nil
		}
	}
	ctx = authz.WithRequestCache(ctx)
	env := authz.RequestEnvFrom(ctx)
	var out map[string]string
	for _, path := range paths {
		if path == "" {
			continue
		}
		if _, done := out[path]; done {
			continue
		}
		d, err := ev.Evaluate(ctx, p, act, domain.Resource{TenantID: tree.TenantID, Tree: tree, Path: path}, env)
		if err != nil {
			return nil, fmt.Errorf("evaluate %s on %q: %w", act, path, err)
		}
		if d.Allowed || d.Effect != domain.EffectDeny {
			continue
		}
		if out == nil {
			out = make(map[string]string)
		}
		out[path] = DenyingPolicy(d)
	}
	return out, nil
}

// DenyingPolicy returns the name of the first policy whose statement
// denied d ("" when none is named).
func DenyingPolicy(d domain.Decision) string {
	for _, r := range d.Reasons {
		if r.Source != authz.SourcePolicy || r.Effect != domain.EffectDeny {
			continue
		}
		rest, ok := strings.CutPrefix(r.Detail, "policy ")
		if !ok {
			continue
		}
		if q, err := strconv.QuotedPrefix(rest); err == nil {
			if name, err := strconv.Unquote(q); err == nil {
				return name
			}
		}
	}
	return ""
}

// hasPathDenies reports whether an enabled policy of the tenant has a deny
// statement on a path resource.
func hasPathDenies(ctx context.Context, store domain.MetaStore, tenantID string) (bool, error) {
	if tenantID == "" {
		return false, nil
	}
	pols, err := store.ListPolicies(ctx, tenantID)
	if errors.Is(err, domain.ErrNotFound) {
		return false, nil
	}
	if err != nil {
		return false, fmt.Errorf("list policies: %w", err)
	}
	for _, pol := range pols {
		if !pol.Enabled {
			continue
		}
		for _, st := range pol.Statements {
			if !strings.EqualFold(strings.TrimSpace(st.Effect), domain.EffectDeny) {
				continue
			}
			for _, r := range st.Resources {
				if strings.HasPrefix(strings.TrimSpace(r), "wt:path/") {
					return true, nil
				}
			}
		}
	}
	return false, nil
}
