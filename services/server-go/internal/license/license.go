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
// # Grantees and expiry (v0.5)
//
// A rule with a Grantee (a tenant slug) applies only to principals of that
// tenant: its members (any role), the principal whose personal tenant it
// is, and its service accounts. A rule without a Grantee applies to every
// non-member. For a given principal the rules naming one of its tenants
// take precedence over the general rules: when any grantee rule matches a
// path, the most specific of those decides; otherwise the most specific
// general rule does. So a general "**" proprietary rule plus a grantee
// "src/**" modify rule lets that tenant read and push src/ while keeping
// the rest closed. Rules whose ExpiresAt has passed are ignored entirely
// (a lapsed grant falls back to the general rules; a lapsed restriction
// lifts).
//
// Violations are reported as *domain.PushDenied{Kind: "license_violation"}
// naming the first offending path (in input order).
package license

import (
	"context"
	"errors"
	"fmt"
	"regexp"
	"time"

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
	now   func() time.Time // nil = time.Now (tests override it)
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
	if len(paths) == 0 {
		return nil
	}
	sc, err := c.scopeFor(ctx, p, tree)
	if err != nil || sc == nil {
		return err
	}
	for _, raw := range paths {
		if d := sc.check(op, raw); d != nil {
			return d
		}
	}
	return nil
}

// UnreadablePaths returns the paths (in input order) that p may not read
// under the tree's license rules; nil when every path is readable. Invalid
// paths are reported as unreadable (fail closed). An error means the rules
// or memberships could not be loaded (callers must treat it as a denial).
func (c *Checker) UnreadablePaths(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) ([]string, error) {
	if tree == nil {
		return nil, fmt.Errorf("license: nil tree: %w", domain.ErrInvalid)
	}
	if len(paths) == 0 {
		return nil, nil
	}
	sc, err := c.scopeFor(ctx, p, tree)
	if err != nil || sc == nil {
		return nil, err
	}
	var out []string
	for _, raw := range paths {
		if sc.check(OpRead, raw) != nil {
			out = append(out, raw)
		}
	}
	return out, nil
}

// scope is the set of rules that bind one principal on one tree.
type scope struct {
	grantee ruleSet // rules naming one of the principal's tenants
	general ruleSet // rules without a grantee
}

// check returns the denial of op on raw, or nil when it is permitted.
func (s *scope) check(op Operation, raw string) *domain.PushDenied {
	segs, ok := splitPath(raw)
	if !ok {
		return &domain.PushDenied{
			Kind:   KindLicenseViolation,
			Path:   raw,
			Reason: fmt.Sprintf("license check failed for operation %q: invalid path %q", op, raw),
		}
	}
	rule := s.grantee.resolve(segs)
	if rule == nil {
		rule = s.general.resolve(segs)
	}
	if rule == nil || rightsOf(&rule.rule).permits(op) {
		return nil
	}
	return denial(op, raw, &rule.rule)
}

// scopeFor loads the rules that restrict p on tree. It returns nil (and no
// error) when p is unrestricted: an owner, member or server admin, or no
// rule applies to p.
func (c *Checker) scopeFor(ctx context.Context, p *domain.Principal, tree *domain.Tree) (*scope, error) {
	if ownerOrAdmin(p, tree) {
		return nil, nil
	}
	if c.store == nil {
		return nil, errors.New("license: no store configured")
	}
	rules, err := c.store.ListLicenseRules(ctx, tree.ID)
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return nil, fmt.Errorf("license: list rules for tree %s: %w", tree.ID, err)
	}
	now := c.clock()
	live := rules[:0:0]
	for _, r := range rules {
		if r.ExpiresAt == nil || now.Before(*r.ExpiresAt) {
			live = append(live, r)
		}
	}
	if len(live) == 0 {
		return nil, nil
	}
	member, err := c.isMember(ctx, p, tree)
	if err != nil || member {
		return nil, err
	}

	var general, granted []domain.LicenseRule
	inTenant := map[string]bool{}
	for _, r := range live {
		if r.Grantee == "" {
			general = append(general, r)
			continue
		}
		in, seen := inTenant[r.Grantee]
		if !seen {
			if in, err = c.inTenant(ctx, p, r.Grantee); err != nil {
				return nil, err
			}
			inTenant[r.Grantee] = in
		}
		if in {
			granted = append(granted, r)
		}
	}
	if len(general) == 0 && len(granted) == 0 {
		return nil, nil
	}
	sc := &scope{}
	if sc.general, err = compileRules(general); err != nil {
		return nil, fmt.Errorf("license: tree %s: %w", tree.ID, err)
	}
	if sc.grantee, err = compileRules(granted); err != nil {
		return nil, fmt.Errorf("license: tree %s: %w", tree.ID, err)
	}
	return sc, nil
}

// inTenant reports whether p belongs to the tenant with the given slug: it
// is p's personal (or, for service accounts, owning) tenant, or p's
// account is a member of it. An unknown slug matches nobody.
func (c *Checker) inTenant(ctx context.Context, p *domain.Principal, slug string) (bool, error) {
	if p == nil {
		return false, nil
	}
	t, err := c.store.GetTenantBySlug(ctx, slug)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		return false, nil
	case err != nil:
		return false, fmt.Errorf("license: tenant %q: %w", slug, err)
	}
	if p.TenantID != "" && p.TenantID == t.ID {
		return true, nil
	}
	if p.AccountID == "" {
		return false, nil
	}
	_, err = c.store.GetMembership(ctx, t.ID, p.AccountID)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		return false, nil
	case err != nil:
		return false, fmt.Errorf("license: membership of %s in tenant %q: %w", p.AccountID, slug, err)
	}
	return true, nil
}

func (c *Checker) clock() time.Time {
	if c.now != nil {
		return c.now()
	}
	return time.Now()
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
// "GPL-2.0-only WITH Classpath-exception-2.0", "LicenseRef-Acme"), plus a
// grantee that is "" or has tenant-slug syntax. TreeID, the existence of
// the grantee tenant and whether ExpiresAt lies in the future are not
// checked (the REST handler does). Errors wrap domain.ErrInvalid.
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
	if rule.Grantee != "" && !granteeRe.MatchString(rule.Grantee) {
		return fmt.Errorf("%w: grantee %q is not a tenant slug", domain.ErrInvalid, rule.Grantee)
	}
	if rule.ExpiresAt != nil && rule.ExpiresAt.IsZero() {
		return fmt.Errorf("%w: expires_at must be a valid time", domain.ErrInvalid)
	}
	return nil
}

// granteeRe is the tenant slug syntax (personal tenant slugs are usernames).
var granteeRe = regexp.MustCompile(`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`)
