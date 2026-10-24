// Package license implements domain.LicenseChecker: per-path SPDX license
// rules that sit above IAM (both must allow an operation).
//
// # Rules
//
// A tree's rules come from MetaStore.ListLicenseRules. Each rule assigns an
// SPDX expression and a grant to the paths matched by its glob (see
// glob.go for the syntax and the specificity order). For every path only
// the most specific matching rule applies; paths no rule matches are
// unrestricted, and a tree without rules is unrestricted (IAM alone
// governs access).
//
// # Who is restricted
//
// Rules never restrict the tree's owning tenant: a principal whose personal
// tenant owns the tree, any member (any role) of the owning tenant, and
// server admins have full rights. Everyone else — accounts of other
// tenants and anonymous callers alike — is a "non-member" and gets the
// rights of the matching rule's grant:
//
//	grant          read  write(push)  export
//	read-only       ✓        ✗          ✗
//	modify          ✓        ✓          ✗
//	redistribute    ✓        ✓          ✓
//	"" (none)       license default (below)
//	unknown         ✗        ✗          ✗   (fail closed)
//
// Without a grant, a proprietary license ("Proprietary",
// "LicenseRef-Proprietary", "LicenseRef-*proprietary*"; see IsProprietary)
// gives non-members no access at all, while any other license gives full
// rights: open-source terms do not restrict reading, modifying or
// redistributing, and IAM still decides who may push.
//
// Export ("export/copy/fork/archive/git export") is not part of
// domain.LicenseChecker; callers use CheckExport. Under a "modify" grant a
// non-member may read and push but export is forbidden.
//
// Violations are reported as *domain.PushDenied{Kind: "license_violation"}
// naming the first offending path (in input order).
package license

import (
	"context"
	"errors"
	"fmt"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Grant levels for LicenseRule.Grant.
const (
	GrantReadOnly     = "read-only"
	GrantModify       = "modify"
	GrantRedistribute = "redistribute"
)

// KindLicenseViolation is the PushDenied.Kind used by this package.
const KindLicenseViolation = "license_violation"

// Operation is something a principal does with licensed paths.
type Operation string

const (
	OpRead   Operation = "read"   // view contents, pull, clone into the platform
	OpWrite  Operation = "write"  // push, stage, modify
	OpExport Operation = "export" // export, copy, fork, archive, mirror
)

// requiredGrant names the weakest grant that permits op (for messages).
func (op Operation) requiredGrant() string {
	switch op {
	case OpRead:
		return GrantReadOnly
	case OpWrite:
		return GrantModify
	default:
		return GrantRedistribute
	}
}

type rights uint8

const (
	canRead rights = 1 << iota
	canWrite
	canExport

	rightsNone rights = 0
	rightsAll         = canRead | canWrite | canExport
)

func (r rights) permits(op Operation) bool {
	switch op {
	case OpRead:
		return r&canRead != 0
	case OpWrite:
		return r&canWrite != 0
	case OpExport:
		return r&canExport != 0
	}
	return false
}

// rightsOf returns what a non-member may do on paths governed by rule.
func rightsOf(rule *domain.LicenseRule) rights {
	switch rule.Grant {
	case GrantReadOnly:
		return canRead
	case GrantModify:
		return canRead | canWrite
	case GrantRedistribute:
		return rightsAll
	case "":
		if IsProprietary(rule.SPDX) {
			return rightsNone
		}
		return rightsAll
	default:
		return rightsNone
	}
}

// Permits reports whether a non-member of the tree's tenant may perform op
// on paths governed by rule (see the package documentation).
func Permits(rule domain.LicenseRule, op Operation) bool {
	return rightsOf(&rule).permits(op)
}

// Checker enforces per-path SPDX license rules. It implements
// domain.LicenseChecker and is safe for concurrent use.
type Checker struct {
	store domain.MetaStore
}

var _ domain.LicenseChecker = (*Checker)(nil)

// New returns a license checker reading rules and memberships from store.
func New(store domain.MetaStore) *Checker { return &Checker{store: store} }

// CheckRead returns *domain.PushDenied when p may not read paths of tree.
func (c *Checker) CheckRead(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) error {
	return c.Check(ctx, p, tree, OpRead, paths)
}

// CheckPush returns *domain.PushDenied when p may not write changedPaths
// of tree.
func (c *Checker) CheckPush(ctx context.Context, p *domain.Principal, tree *domain.Tree, changedPaths []string) error {
	return c.Check(ctx, p, tree, OpWrite, changedPaths)
}

// CheckExport returns *domain.PushDenied when p may not export, copy, fork
// or archive paths of tree.
func (c *Checker) CheckExport(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) error {
	return c.Check(ctx, p, tree, OpExport, paths)
}

// Check evaluates op on paths for p. It returns nil when allowed, a
// *domain.PushDenied for a license violation, or another error when the
// rules or membership cannot be loaded (callers must treat that as a
// denial).
func (c *Checker) Check(ctx context.Context, p *domain.Principal, tree *domain.Tree, op Operation, paths []string) error {
	if tree == nil {
		return fmt.Errorf("license: nil tree: %w", domain.ErrInvalid)
	}
	switch op {
	case OpRead, OpWrite, OpExport:
	default:
		return fmt.Errorf("license: unknown operation %q: %w", op, domain.ErrInvalid)
	}
	if len(paths) == 0 || ownerOrAdmin(p, tree) {
		return nil
	}
	if c.store == nil {
		return errors.New("license: no store configured")
	}

	rules, err := c.store.ListLicenseRules(ctx, tree.ID)
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return fmt.Errorf("license: list rules for tree %s: %w", tree.ID, err)
	}
	if len(rules) == 0 {
		return nil
	}
	member, err := c.isMember(ctx, p, tree)
	if err != nil {
		return err
	}
	if member {
		return nil
	}

	set, err := compileRules(rules)
	if err != nil {
		return fmt.Errorf("license: tree %s: %w", tree.ID, err)
	}
	for _, raw := range paths {
		segs, ok := splitPath(raw)
		if !ok {
			return &domain.PushDenied{
				Kind:   KindLicenseViolation,
				Path:   raw,
				Reason: fmt.Sprintf("license check failed for operation %q: invalid path %q", op, raw),
			}
		}
		rule := set.resolve(segs)
		if rule == nil {
			continue
		}
		if !rightsOf(&rule.rule).permits(op) {
			return denial(op, raw, &rule.rule)
		}
	}
	return nil
}

func denial(op Operation, path string, r *domain.LicenseRule) *domain.PushDenied {
	granted := r.Grant
	if granted == "" {
		granted = "(none)"
	}
	return &domain.PushDenied{
		Kind:    KindLicenseViolation,
		Path:    path,
		License: r.SPDX,
		Reason: fmt.Sprintf("license check failed for operation %q: path %q is licensed %q (rule %q); required grant: %s; granted: %s",
			op, path, r.SPDX, r.PathGlob, op.requiredGrant(), granted),
	}
}

// ownerOrAdmin covers the cases decidable without the store.
func ownerOrAdmin(p *domain.Principal, tree *domain.Tree) bool {
	if p == nil {
		return false
	}
	return p.IsAdmin || (p.TenantID != "" && p.TenantID == tree.TenantID)
}

// isMember reports whether p belongs to the tenant owning tree.
func (c *Checker) isMember(ctx context.Context, p *domain.Principal, tree *domain.Tree) (bool, error) {
	if p == nil || p.AccountID == "" || tree.TenantID == "" {
		return false, nil
	}
	m, err := c.store.GetMembership(ctx, tree.TenantID, p.AccountID)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		return false, nil
	case err != nil:
		return false, fmt.Errorf("license: membership of %s in tenant %s: %w", p.AccountID, tree.TenantID, err)
	}
	return m != nil, nil
}

type compiledRule struct {
	rule domain.LicenseRule
	glob *glob
}

// ruleSet is a list of rules ordered most specific first.
type ruleSet []compiledRule

// compileRules compiles and orders rules. A malformed stored glob is an
// error: skipping it would silently lift the restriction it expresses.
func compileRules(rules []domain.LicenseRule) (ruleSet, error) {
	set := make(ruleSet, 0, len(rules))
	for _, r := range rules {
		g, err := compileGlob(r.PathGlob)
		if err != nil {
			return nil, fmt.Errorf("invalid license rule: %w", err)
		}
		set = append(set, compiledRule{rule: r, glob: g})
	}
	sortBySpecificity(set, func(r compiledRule) *glob { return r.glob })
	return set, nil
}

func (s ruleSet) resolve(segs []string) *compiledRule {
	for i := range s {
		if s[i].glob.match(segs) {
			return &s[i]
		}
	}
	return nil
}

// Resolve returns the rule governing path p: the most specific rule whose
// glob matches it. Rules with malformed globs are ignored. ok is false when
// no rule applies.
func Resolve(rules []domain.LicenseRule, p string) (rule domain.LicenseRule, ok bool) {
	segs, valid := splitPath(p)
	if !valid {
		return domain.LicenseRule{}, false
	}
	set := make(ruleSet, 0, len(rules))
	for _, r := range rules {
		if g, err := compileGlob(r.PathGlob); err == nil {
			set = append(set, compiledRule{rule: r, glob: g})
		}
	}
	sortBySpecificity(set, func(r compiledRule) *glob { return r.glob })
	if r := set.resolve(segs); r != nil {
		return r.rule, true
	}
	return domain.LicenseRule{}, false
}

// Validate checks a rule submitted through the API: a well-formed path
// glob, a known grant ("", "read-only", "modify", "redistribute") and a
// syntactically valid SPDX expression (e.g. "MIT", "MIT OR Apache-2.0",
// "GPL-2.0-only WITH Classpath-exception-2.0", "LicenseRef-Acme"). TreeID
// is not checked. Errors wrap domain.ErrInvalid.
func Validate(rule domain.LicenseRule) error {
	if _, err := compileGlob(rule.PathGlob); err != nil {
		return fmt.Errorf("%w: %v", domain.ErrInvalid, err)
	}
	switch rule.Grant {
	case "", GrantReadOnly, GrantModify, GrantRedistribute:
	default:
		return fmt.Errorf("%w: unknown grant %q (want %q, %q or %q)",
			domain.ErrInvalid, rule.Grant, GrantReadOnly, GrantModify, GrantRedistribute)
	}
	if err := ValidateSPDX(rule.SPDX); err != nil {
		return fmt.Errorf("%w: %v", domain.ErrInvalid, err)
	}
	return nil
}
