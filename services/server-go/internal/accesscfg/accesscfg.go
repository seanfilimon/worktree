// Package accesscfg applies declarative access documents (`wt access
// apply`, SyncAccessConfig, POST .../access-config/apply) to tenant
// policies and custom roles (docs/ENGINEERING_PLAN.md §13.7).
//
// A document is the canonical JSON compiled by
// worktree_protocol::access::declarative from `.wt/access/roles.toml` and
// `.wt/access/policies.toml` (tenant scope) or
// `<tree>/.wt-tree/access/policies.toml` (tree scope):
//
//	{"roles": [CustomRole], "policies": [Policy]}
//
// with the Go JSON field names of domain.CustomRole and domain.Policy.
// Applying a document makes the set of config-managed roles and policies
// of its scope (ManagedBy == domain.ManagedByConfig, and for tree documents
// Policy.TreeID == the tree) equal to the document: missing ones are
// created, changed ones updated and the ones no longer listed deleted.
// Roles and policies managed through the REST API are left alone, except
// that a document entry with the same name adopts it.
//
// Tree documents may only restrict access: they hold no roles, every
// statement is a deny, and every resource lies inside the tree. Their
// policies are stored under the name "<tree>/<name>".
//
// Before anything is written the document is validated (statements with
// authz.ValidateStatements, role permissions with authz.ValidatePermissions,
// referenced users, teams, service accounts, tenants and roles must exist)
// and checked for privilege escalation: unless the caller is a tenant owner
// or a server admin, every permission granted by a created or updated role
// or allow statement must be one the caller already has in the tenant. A
// dry run stops after computing the plan.
package accesscfg

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/policyhist"
)

// Collection is the DocStore collection of applied documents (§13.14),
// keyed by domain.DocKey(tenantID, treeID) (treeID "" for tenant scope).
const Collection = "access_docs"

// KindCombined is AccessConfigDoc.Kind of a roles + policies document.
const KindCombined = "combined"

// Limits.
const (
	MaxContentSize = 1 << 20
	maxRoles       = 200
	maxPolicies    = 200
	maxStatements  = 100
	maxPermissions = 128
	maxErrors      = 50
)

// roleNameRe is the syntax of custom role names (as in the REST API).
var roleNameRe = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{1,39}$`)

// Deps are the collaborators of the accesscfg package. Store and Authz are
// required; Authz should also implement domain.AccessEvaluator (the
// escalation check needs effective permissions). The others may be nil.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Service applies and serves declarative access documents.
type Service struct {
	d   Deps
	mu  sync.Mutex // serializes Apply: plans are computed against a stable state
	now func() time.Time
}

// New returns a Service.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, now: func() time.Time { return time.Now().UTC() }}
}

// Document is the canonical JSON form of a declarative access document.
type Document struct {
	Roles    []domain.CustomRole `json:"roles"`
	Policies []domain.Policy     `json:"policies"`
}

// Change is one entry of a plan.
type Change struct {
	Kind   string `json:"kind"` // "role" | "policy"
	Name   string `json:"name"` // as written in the document
	Op     string `json:"op"`   // "create" | "update" | "delete"
	Detail string `json:"detail,omitempty"`
}

// String renders the change as in AccessConfigSyncResponse.changes, e.g.
// "policy protect-main: update (2 statements)".
func (c Change) String() string {
	s := c.Kind + " " + c.Name + ": " + c.Op
	if c.Detail != "" {
		s += " (" + c.Detail + ")"
	}
	return s
}

// Result is the outcome of Apply. When Accepted is false, Errors explains
// why and nothing was changed.
type Result struct {
	Accepted bool     `json:"accepted"`
	Errors   []string `json:"errors"`
	Changes  []string `json:"changes"`
	DryRun   bool     `json:"-"`
	Plan     []Change `json:"-"`
	// Doc is the stored document (nil for dry runs and rejections).
	Doc *domain.AccessConfigDoc `json:"-"`
}

func rejected(errs []string) *Result {
	if len(errs) > maxErrors {
		errs = append(errs[:maxErrors:maxErrors], fmt.Sprintf("... and %d more errors", len(errs)-maxErrors))
	}
	return &Result{Accepted: false, Errors: errs, Changes: []string{}}
}

// Get returns the last applied document of the tenant (tree nil) or tree.
// Reading needs iam:read on the tenant, or tree:read on the tree (the tree
// document is committed in the tree anyway). ErrNotFound when none was
// applied.
func (s *Service) Get(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree) (*domain.AccessConfigDoc, error) {
	if p == nil {
		return nil, fmt.Errorf("accesscfg: sign in to read access documents: %w", domain.ErrUnauthenticated)
	}
	act := domain.ActIAMRead
	if tree != nil {
		act = domain.ActTreeRead
	}
	if err := s.d.Authz.Authorize(ctx, p, act, tenant.ID, tree); err != nil {
		return nil, err
	}
	var doc domain.AccessConfigDoc
	if err := s.d.Store.GetDoc(ctx, Collection, docKey(tenant, tree), &doc); err != nil {
		return nil, err
	}
	return &doc, nil
}

func docKey(tenant *domain.Tenant, tree *domain.Tree) string {
	if tree == nil {
		return domain.DocKey(tenant.ID, "")
	}
	return domain.DocKey(tenant.ID, tree.ID)
}

// scope describes where a document applies.
type scope struct {
	tenant *domain.Tenant
	tree   *domain.Tree // nil = tenant scope
}

func (sc scope) treeID() string {
	if sc.tree == nil {
		return ""
	}
	return sc.tree.ID
}

// storedName is the policy name used in the store for a document name.
func (sc scope) storedName(name string) string {
	if sc.tree == nil {
		return name
	}
	return sc.tree.Name + "/" + name
}

func (sc scope) label() string {
	if sc.tree == nil {
		return "tenant " + sc.tenant.Slug
	}
	return "tree " + sc.tenant.Slug + "/" + sc.tree.Name
}

// Apply validates content against the scope (tenant, or tree when tree is
// non-nil), computes the plan and, unless dryRun, makes it so. Applying
// needs iam:admin on the tenant (tenant documents) or tree:admin on the
// tree (tree documents). Authorization failures are errors; problems with
// the document are reported in the Result.
func (s *Service) Apply(ctx context.Context, p *domain.Principal, tenant *domain.Tenant, tree *domain.Tree, content []byte, dryRun bool) (*Result, error) {
	if p == nil {
		return nil, fmt.Errorf("accesscfg: sign in to apply access documents: %w", domain.ErrUnauthenticated)
	}
	if tenant == nil {
		return nil, fmt.Errorf("accesscfg: tenant is required: %w", domain.ErrInvalid)
	}
	if tree != nil && tree.TenantID != tenant.ID {
		return nil, fmt.Errorf("accesscfg: tree %q is not in tenant %s: %w", tree.Name, tenant.Slug, domain.ErrInvalid)
	}
	sc := scope{tenant: tenant, tree: tree}
	act := domain.ActIAMAdmin
	if tree != nil {
		act = domain.ActTreeAdmin
	}
	if err := s.d.Authz.Authorize(ctx, p, act, tenant.ID, tree); err != nil {
		return nil, err
	}

	doc, errs := decode(content)
	if len(errs) > 0 {
		return rejected(errs), nil
	}

	s.mu.Lock()
	defer s.mu.Unlock()

	v := &validator{s: s, ctx: ctx, sc: sc, doc: doc}
	if err := v.run(); err != nil {
		return nil, err
	}
	if len(v.errs) > 0 {
		return rejected(v.errs), nil
	}
	pl, err := s.plan(ctx, sc, doc)
	if err != nil {
		return nil, err
	}
	if len(pl.errs) > 0 {
		return rejected(pl.errs), nil
	}
	if errs, err := s.checkEscalation(ctx, p, sc, pl); err != nil {
		return nil, err
	} else if len(errs) > 0 {
		return rejected(errs), nil
	}

	res := &Result{Accepted: true, Errors: []string{}, Changes: make([]string, 0, len(pl.changes)), DryRun: dryRun, Plan: pl.changes}
	for _, c := range pl.changes {
		res.Changes = append(res.Changes, c.String())
	}
	if dryRun {
		return res, nil
	}

	// v0.6 policy history (§14.7, workstream policy-history): record a
	// PolicyVersion for every policy the apply creates, updates or deletes.
	if err := policyhist.New(s.d.Store, s.d.Bus, s.d.Logger).Track(ctx, actor(p), tenant.ID, "applied from the access document of "+sc.label(),
		func() error { return s.execute(ctx, p, sc, pl) }); err != nil {
		return nil, err
	}
	stored := &domain.AccessConfigDoc{
		TenantID: tenant.ID, TreeID: sc.treeID(), Kind: KindCombined,
		Content: append([]byte(nil), content...), Hash: hash.Sum(content).Hex(),
		UpdatedBy: actor(p), UpdatedAt: s.now(),
	}
	if err := s.d.Store.PutDoc(ctx, Collection, docKey(tenant, tree), stored); err != nil {
		return nil, fmt.Errorf("accesscfg: store access document: %w", err)
	}
	res.Doc = stored
	s.record(ctx, p, sc, stored, res.Changes)
	return res, nil
}

func actor(p *domain.Principal) string {
	if p == nil {
		return "anonymous"
	}
	return p.Username
}

// decode parses the canonical JSON document strictly (unknown fields are
// errors, so typos do not silently drop settings).
func decode(content []byte) (*Document, []string) {
	if len(bytes.TrimSpace(content)) == 0 {
		return nil, []string{"content is empty"}
	}
	if len(content) > MaxContentSize {
		return nil, []string{fmt.Sprintf("content is larger than %d bytes", MaxContentSize)}
	}
	dec := json.NewDecoder(bytes.NewReader(content))
	dec.DisallowUnknownFields()
	var doc Document
	if err := dec.Decode(&doc); err != nil {
		return nil, []string{"content is not a valid access document: " + err.Error()}
	}
	if dec.More() {
		return nil, []string{"content has trailing data after the JSON document"}
	}
	return &doc, nil
}

// ---- validation ----

type validator struct {
	s    *Service
	ctx  context.Context
	sc   scope
	doc  *Document
	errs []string
	// docRoles are the role names defined by the document.
	docRoles map[string]bool
	// seen caches reference lookups ("user:ada" -> exists).
	seen map[string]bool
}

func (v *validator) errorf(format string, args ...any) {
	v.errs = append(v.errs, fmt.Sprintf(format, args...))
}

// run normalizes the document in place and records every problem. It
// returns an error only for store failures.
func (v *validator) run() error {
	v.docRoles = map[string]bool{}
	v.seen = map[string]bool{}
	doc := v.doc
	if v.sc.tree != nil && len(doc.Roles) > 0 {
		v.errorf("roles: tree-level documents cannot define roles (define them in the tenant document)")
	}
	if len(doc.Roles) > maxRoles {
		v.errorf("roles: at most %d roles are allowed", maxRoles)
	}
	if len(doc.Policies) > maxPolicies {
		v.errorf("policies: at most %d policies are allowed", maxPolicies)
	}
	for i := range doc.Roles {
		v.role(i, &doc.Roles[i])
	}
	names := map[string]int{}
	for i := range doc.Policies {
		pol := &doc.Policies[i]
		pol.Name = strings.TrimSpace(pol.Name)
		pol.Description = strings.TrimSpace(pol.Description)
		at := fmt.Sprintf("policies[%d]", i)
		if pol.Name != "" {
			at += " (" + pol.Name + ")"
		}
		switch {
		case pol.Name == "":
			v.errorf("%s: name is required", at)
		case len(pol.Name) > 100 || strings.ContainsAny(pol.Name, "\x00\r\n/"):
			v.errorf("%s: name must be at most 100 characters without '/' or line breaks", at)
		}
		if j, dup := names[strings.ToLower(pol.Name)]; dup && pol.Name != "" {
			v.errorf("%s: duplicate policy name (also policies[%d])", at, j)
		} else {
			names[strings.ToLower(pol.Name)] = i
		}
		if len(pol.Description) > 2000 {
			v.errorf("%s: description must be at most 2000 characters", at)
		}
		if pol.Statements == nil {
			pol.Statements = []domain.PolicyStatement{}
		}
		if len(pol.Statements) > maxStatements {
			v.errorf("%s: a policy may have at most %d statements", at, maxStatements)
		}
		for _, e := range authz.ValidateStatements(pol.Statements) {
			v.errorf("%s: statements[%d].%s: %s", at, e.Statement, e.Field, e.Message)
		}
		for j := range pol.Statements {
			if err := v.statement(fmt.Sprintf("%s: statements[%d]", at, j), &pol.Statements[j]); err != nil {
				return err
			}
		}
	}
	return nil
}

func (v *validator) role(i int, r *domain.CustomRole) {
	r.Name = strings.TrimSpace(r.Name)
	r.Description = strings.TrimSpace(r.Description)
	at := fmt.Sprintf("roles[%d]", i)
	if r.Name != "" {
		at += " (" + r.Name + ")"
	}
	switch {
	case !roleNameRe.MatchString(r.Name):
		v.errorf("%s: name must be 2-40 characters of lowercase letters, digits and '-', starting with a letter or digit", at)
	case authz.IsBuiltinRole(r.Name):
		v.errorf("%s: %q is a built-in role", at, r.Name)
	case v.docRoles[r.Name]:
		v.errorf("%s: duplicate role name", at)
	}
	v.docRoles[r.Name] = true
	if len(r.Description) > 2000 {
		v.errorf("%s: description must be at most 2000 characters", at)
	}
	if len(r.Permissions) > maxPermissions {
		v.errorf("%s: at most %d permissions are allowed", at, maxPermissions)
	}
	perms := make([]domain.Action, 0, len(r.Permissions))
	for _, p := range r.Permissions {
		p = domain.Action(strings.TrimSpace(string(p)))
		if !slices.Contains(perms, p) {
			perms = append(perms, p)
		}
	}
	slices.Sort(perms)
	r.Permissions = perms
	if err := authz.ValidatePermissions(perms); err != nil {
		v.errorf("%s: %v", at, err)
	}
}

// statement checks the parts ValidateStatements cannot: tree-document
// restrictions and that named principals exist.
func (v *validator) statement(at string, st *domain.PolicyStatement) error {
	if v.sc.tree != nil {
		if !strings.EqualFold(strings.TrimSpace(st.Effect), domain.EffectDeny) {
			v.errorf("%s: tree-level documents may only contain deny statements", at)
		}
		for k, r := range st.Resources {
			if !v.insideTree(strings.TrimSpace(r)) {
				v.errorf("%s: resources[%d]: %q is outside tree %s/%s (tree documents may only name wt:tree/%s/%s, wt:branch/%s/%s/... or wt:path/%s/%s/...)",
					at, k, r, v.sc.tenant.Slug, v.sc.tree.Name,
					v.sc.tenant.Slug, v.sc.tree.Name, v.sc.tenant.Slug, v.sc.tree.Name, v.sc.tenant.Slug, v.sc.tree.Name)
			}
		}
	}
	for k, pr := range st.Principals {
		ok, err := v.principalExists(strings.TrimSpace(pr))
		if err != nil {
			return err
		}
		if !ok {
			v.errorf("%s: principals[%d]: %s does not exist", at, k, strings.TrimSpace(pr))
		}
	}
	return nil
}

func (v *validator) insideTree(r string) bool {
	base := v.sc.tenant.Slug + "/" + v.sc.tree.Name
	return r == "wt:tree/"+base ||
		strings.HasPrefix(r, "wt:branch/"+base+"/") ||
		strings.HasPrefix(r, "wt:path/"+base+"/")
}

// principalExists reports whether a concrete principal reference names an
// existing identity. Patterns with wildcards and the generic principals
// always pass; malformed ones are reported by ValidateStatements.
func (v *validator) principalExists(pr string) (bool, error) {
	if pr == "*" || pr == "authenticated" || pr == "anonymous" || strings.Contains(pr, "*") {
		return true, nil
	}
	kind, name, ok := strings.Cut(pr, ":")
	if !ok || name == "" {
		return true, nil
	}
	if known, ok := v.seen[pr]; ok {
		return known, nil
	}
	exists, err := v.lookup(kind, name)
	if err != nil {
		return false, err
	}
	v.seen[pr] = exists
	return exists, nil
}

func (v *validator) lookup(kind, name string) (bool, error) {
	ctx, st, tenant := v.ctx, v.s.d.Store, v.sc.tenant
	found := func(err error) (bool, error) {
		if errors.Is(err, domain.ErrNotFound) {
			return false, nil
		}
		return err == nil, err
	}
	switch kind {
	case "user":
		_, err := st.GetAccountByUsername(ctx, name)
		return found(err)
	case "team":
		_, err := st.GetTeamBySlug(ctx, tenant.ID, name)
		return found(err)
	case "tenant":
		_, err := st.GetTenantBySlug(ctx, name)
		return found(err)
	case "sa":
		tenantID := tenant.ID
		if t, n, ok := strings.Cut(name, "/"); ok {
			other, err := st.GetTenantBySlug(ctx, t)
			if ok, err := found(err); !ok {
				return false, err
			}
			tenantID, name = other.ID, n
		}
		_, err := st.GetServiceAccountByName(ctx, tenantID, name)
		return found(err)
	case "role":
		name = strings.ToLower(name)
		if authz.IsBuiltinRole(name) || v.docRoles[name] {
			return true, nil
		}
		r, err := st.GetRole(ctx, tenant.ID, name)
		if ok, err := found(err); !ok {
			return false, err
		}
		// A config-managed role missing from a tenant document is about to
		// be deleted.
		return v.sc.tree != nil || r.ManagedBy != domain.ManagedByConfig, nil
	}
	return true, nil
}

// ---- planning ----

type roleOp struct {
	op       string
	next     *domain.CustomRole // create / update
	prev     *domain.CustomRole // update / delete
	adopting bool
}

type policyOp struct {
	op       string
	next     *domain.Policy
	prev     *domain.Policy
	adopting bool
}

type plan struct {
	roles    []roleOp
	policies []policyOp
	changes  []Change
	errs     []string
}

func (s *Service) plan(ctx context.Context, sc scope, doc *Document) (*plan, error) {
	pl := &plan{}
	if sc.tree == nil {
		if err := s.planRoles(ctx, sc, doc, pl); err != nil {
			return nil, err
		}
	}
	if err := s.planPolicies(ctx, sc, doc, pl); err != nil {
		return nil, err
	}
	return pl, nil
}

func (s *Service) planRoles(ctx context.Context, sc scope, doc *Document, pl *plan) error {
	existing, err := s.d.Store.ListRoles(ctx, sc.tenant.ID)
	if err != nil {
		return fmt.Errorf("accesscfg: list roles: %w", err)
	}
	byName := make(map[string]*domain.CustomRole, len(existing))
	for i := range existing {
		byName[existing[i].Name] = &existing[i]
	}
	inDoc := map[string]bool{}
	for i := range doc.Roles {
		r := doc.Roles[i]
		inDoc[r.Name] = true
		next := &domain.CustomRole{TenantID: sc.tenant.ID, Name: r.Name, Description: r.Description, Permissions: r.Permissions, ManagedBy: domain.ManagedByConfig}
		prev := byName[r.Name]
		switch {
		case prev == nil:
			pl.roles = append(pl.roles, roleOp{op: "create", next: next})
			pl.changes = append(pl.changes, Change{Kind: "role", Name: r.Name, Op: "create", Detail: count(len(r.Permissions), "permission")})
		case prev.ManagedBy != domain.ManagedByConfig:
			next.CreatedAt = prev.CreatedAt
			pl.roles = append(pl.roles, roleOp{op: "update", next: next, prev: prev, adopting: true})
			pl.changes = append(pl.changes, Change{Kind: "role", Name: r.Name, Op: "update", Detail: count(len(r.Permissions), "permission") + ", now managed by config"})
		case prev.Description != next.Description || !slices.Equal(prev.Permissions, next.Permissions):
			next.CreatedAt = prev.CreatedAt
			pl.roles = append(pl.roles, roleOp{op: "update", next: next, prev: prev})
			pl.changes = append(pl.changes, Change{Kind: "role", Name: r.Name, Op: "update", Detail: count(len(r.Permissions), "permission")})
		}
	}
	for i := range existing {
		prev := &existing[i]
		if prev.ManagedBy != domain.ManagedByConfig || inDoc[prev.Name] {
			continue
		}
		where, err := s.roleUsage(ctx, sc.tenant.ID, prev.Name)
		if err != nil {
			return err
		}
		if where != "" {
			pl.errs = append(pl.errs, fmt.Sprintf("role %s: cannot be deleted while it is assigned (%s); keep it in the document or reassign first", prev.Name, where))
			continue
		}
		pl.roles = append(pl.roles, roleOp{op: "delete", prev: prev})
		pl.changes = append(pl.changes, Change{Kind: "role", Name: prev.Name, Op: "delete"})
	}
	return nil
}

func (s *Service) planPolicies(ctx context.Context, sc scope, doc *Document, pl *plan) error {
	existing, err := s.d.Store.ListPolicies(ctx, sc.tenant.ID)
	if err != nil {
		return fmt.Errorf("accesscfg: list policies: %w", err)
	}
	byName := make(map[string]*domain.Policy, len(existing))
	for i := range existing {
		byName[strings.ToLower(existing[i].Name)] = &existing[i]
	}
	mine := func(p *domain.Policy) bool { return p.ManagedBy == domain.ManagedByConfig && p.TreeID == sc.treeID() }
	inDoc := map[string]bool{}
	for i := range doc.Policies {
		d := doc.Policies[i]
		name := sc.storedName(d.Name)
		inDoc[strings.ToLower(name)] = true
		next := &domain.Policy{
			TenantID: sc.tenant.ID, Name: name, Description: d.Description, Enabled: d.Enabled,
			Statements: d.Statements, ManagedBy: domain.ManagedByConfig, TreeID: sc.treeID(),
		}
		detail := count(len(d.Statements), "statement")
		if !d.Enabled {
			detail += ", disabled"
		}
		prev := byName[strings.ToLower(name)]
		switch {
		case prev == nil:
			pl.policies = append(pl.policies, policyOp{op: "create", next: next})
			pl.changes = append(pl.changes, Change{Kind: "policy", Name: d.Name, Op: "create", Detail: detail})
		case mine(prev):
			if samePolicy(prev, next) {
				continue
			}
			pl.policies = append(pl.policies, policyOp{op: "update", next: next, prev: prev})
			pl.changes = append(pl.changes, Change{Kind: "policy", Name: d.Name, Op: "update", Detail: detail})
		case prev.ManagedBy == "" && sc.tree == nil:
			pl.policies = append(pl.policies, policyOp{op: "update", next: next, prev: prev, adopting: true})
			pl.changes = append(pl.changes, Change{Kind: "policy", Name: d.Name, Op: "update", Detail: detail + ", now managed by config"})
		default:
			pl.errs = append(pl.errs, fmt.Sprintf("policy %s: the name %q is already used by a policy managed elsewhere in tenant %s", d.Name, name, sc.tenant.Slug))
		}
	}
	for i := range existing {
		prev := &existing[i]
		if !mine(prev) || inDoc[strings.ToLower(prev.Name)] {
			continue
		}
		name := prev.Name
		if sc.tree != nil {
			name = strings.TrimPrefix(name, sc.tree.Name+"/")
		}
		pl.policies = append(pl.policies, policyOp{op: "delete", prev: prev})
		pl.changes = append(pl.changes, Change{Kind: "policy", Name: name, Op: "delete"})
	}
	return nil
}

func count(n int, noun string) string {
	if n == 1 {
		return "1 " + noun
	}
	return fmt.Sprintf("%d %ss", n, noun)
}

// samePolicy compares the document-controlled fields of two policies.
func samePolicy(a, b *domain.Policy) bool {
	if a.Description != b.Description || a.Enabled != b.Enabled || len(a.Statements) != len(b.Statements) {
		return false
	}
	ja, err1 := json.Marshal(canonStatements(a.Statements))
	jb, err2 := json.Marshal(canonStatements(b.Statements))
	return err1 == nil && err2 == nil && bytes.Equal(ja, jb)
}

// canonStatements copies statements in a form that compares equal across a
// store round trip (empty slices as nil, times in UTC, lower-case effect).
func canonStatements(in []domain.PolicyStatement) []domain.PolicyStatement {
	orNil := func(s []string) []string {
		if len(s) == 0 {
			return nil
		}
		return s
	}
	utc := func(t *time.Time) *time.Time {
		if t == nil {
			return nil
		}
		u := t.UTC()
		return &u
	}
	out := make([]domain.PolicyStatement, len(in))
	for i, st := range in {
		st.Effect = strings.ToLower(strings.TrimSpace(st.Effect))
		st.Actions, st.Resources, st.Principals = orNil(st.Actions), orNil(st.Resources), orNil(st.Principals)
		c := &st.Conditions
		c.IPRanges = orNil(c.IPRanges)
		c.NotBefore, c.NotAfter = utc(c.NotBefore), utc(c.NotAfter)
		if c.TimeWindow != nil {
			tw := *c.TimeWindow
			tw.Days = orNil(tw.Days)
			c.TimeWindow = &tw
		}
		// v0.8 ABAC attribute conditions (abac workstream).
		if len(c.Attributes) == 0 {
			c.Attributes = nil
		} else {
			attrs := make([]domain.AttributeCondition, len(c.Attributes))
			for j, a := range c.Attributes {
				attrs[j] = domain.AttributeCondition{Key: strings.TrimSpace(a.Key), Op: strings.ToLower(strings.TrimSpace(a.Op)), Values: orNil(a.Values)}
			}
			c.Attributes = attrs
		}
		out[i] = st
	}
	return out
}

// roleUsage describes where role is assigned in tenantID ("" = unused).
func (s *Service) roleUsage(ctx context.Context, tenantID, role string) (string, error) {
	ms, err := s.d.Store.ListTenantMembers(ctx, tenantID)
	if err != nil {
		return "", fmt.Errorf("accesscfg: list members: %w", err)
	}
	for _, m := range ms {
		if strings.EqualFold(m.Role, role) {
			return "tenant membership", nil
		}
	}
	sas, err := s.d.Store.ListServiceAccounts(ctx, tenantID)
	if err != nil {
		return "", fmt.Errorf("accesscfg: list service accounts: %w", err)
	}
	for _, sa := range sas {
		if strings.EqualFold(sa.Role, role) {
			return "service account " + sa.Name, nil
		}
	}
	trees, err := s.d.Store.ListTrees(ctx, tenantID)
	if err != nil {
		return "", fmt.Errorf("accesscfg: list trees: %w", err)
	}
	for _, tr := range trees {
		gs, err := s.d.Store.ListGrants(ctx, tr.ID)
		if err != nil {
			return "", fmt.Errorf("accesscfg: list grants: %w", err)
		}
		for _, g := range gs {
			if strings.EqualFold(g.Role, role) {
				return "grant on tree " + tr.Name, nil
			}
		}
	}
	return "", nil
}

// ---- escalation ----

// checkEscalation refuses roles and allow statements (created or updated)
// that grant permissions the caller does not hold, unless the caller is a
// tenant owner or a server admin.
func (s *Service) checkEscalation(ctx context.Context, p *domain.Principal, sc scope, pl *plan) ([]string, error) {
	type grantSet struct {
		what  string
		perms []string
	}
	var sets []grantSet
	for _, op := range pl.roles {
		if op.next == nil {
			continue
		}
		perms := make([]string, len(op.next.Permissions))
		for i, a := range op.next.Permissions {
			perms[i] = string(a)
		}
		sets = append(sets, grantSet{"role " + op.next.Name, perms})
	}
	for _, op := range pl.policies {
		if op.next == nil || !op.next.Enabled {
			continue
		}
		for i, st := range op.next.Statements {
			if strings.EqualFold(strings.TrimSpace(st.Effect), domain.EffectAllow) {
				sets = append(sets, grantSet{fmt.Sprintf("policy %s statement %d", op.next.Name, i+1), st.Actions})
			}
		}
	}
	if len(sets) == 0 {
		return nil, nil
	}
	privileged, err := s.privileged(ctx, p, sc.tenant.ID)
	if err != nil || privileged {
		return nil, err
	}
	ev, ok := s.d.Authz.(domain.AccessEvaluator)
	if !ok {
		return []string{"cannot verify that the document does not escalate privileges; only tenant owners and server admins may apply roles or allow statements here"}, nil
	}
	have, err := ev.EffectivePermissions(ctx, p, sc.tenant.ID, nil)
	if err != nil {
		return nil, fmt.Errorf("accesscfg: effective permissions: %w", err)
	}
	var errs []string
	for _, set := range sets {
		var missing []string
		for _, a := range authz.ExpandActions(set.perms) {
			if !slices.Contains(have, a) {
				missing = append(missing, string(a))
			}
		}
		if len(missing) > 0 {
			errs = append(errs, fmt.Sprintf("%s grants %s, which you do not have yourself (privilege escalation; ask a tenant owner to apply it)",
				set.what, strings.Join(missing, ", ")))
		}
	}
	return errs, nil
}

// privileged reports whether p is a server admin or an owner of tenantID
// (both re-read from the store).
func (s *Service) privileged(ctx context.Context, p *domain.Principal, tenantID string) (bool, error) {
	if p.IsServiceAccount() || p.AccountID == "" {
		return false, nil
	}
	acct, err := s.d.Store.GetAccount(ctx, p.AccountID)
	if err != nil {
		return false, fmt.Errorf("accesscfg: look up account: %w", err)
	}
	if acct.IsAdmin {
		return true, nil
	}
	role, err := s.d.Authz.RoleOf(ctx, p.AccountID, tenantID)
	if err != nil {
		return false, err
	}
	return role == domain.RoleOwner, nil
}

// ---- execution ----

// execute applies the plan: roles first (policies may name them), then
// policies, then role deletions. On a store failure the steps already
// taken are undone, best effort, so a failed apply leaves the previous
// configuration in place.
func (s *Service) execute(ctx context.Context, p *domain.Principal, sc scope, pl *plan) error {
	st := s.d.Store
	var undo []func(context.Context) error
	rollback := func(cause error) error {
		uctx := context.WithoutCancel(ctx)
		for i := len(undo) - 1; i >= 0; i-- {
			if err := undo[i](uctx); err != nil {
				s.d.Logger.ErrorContext(uctx, "accesscfg: rollback step failed", slog.String("tenant", sc.tenant.Slug), slog.Any("error", err))
			}
		}
		return fmt.Errorf("accesscfg: apply to %s failed (changes rolled back): %w", sc.label(), cause)
	}
	now := s.now()
	for _, op := range pl.roles {
		if op.op == "delete" {
			continue
		}
		next := *op.next
		next.UpdatedAt = now
		if next.CreatedAt.IsZero() {
			next.CreatedAt = now
		}
		if err := st.PutRole(ctx, &next); err != nil {
			return rollback(err)
		}
		if op.prev == nil {
			name := next.Name
			undo = append(undo, func(c context.Context) error { return st.DeleteRole(c, sc.tenant.ID, name) })
		} else {
			prev := *op.prev
			undo = append(undo, func(c context.Context) error { return st.PutRole(c, &prev) })
		}
	}
	for _, op := range pl.policies {
		switch op.op {
		case "create":
			next := *op.next
			next.CreatedBy, next.CreatedAt, next.UpdatedAt, next.Version = actor(p), now, now, 1
			if err := st.CreatePolicy(ctx, &next); err != nil {
				return rollback(err)
			}
			id := next.ID
			undo = append(undo, func(c context.Context) error { return st.DeletePolicy(c, sc.tenant.ID, id) })
		case "update":
			prev := *op.prev
			next := *op.next
			next.ID, next.CreatedBy, next.CreatedAt, next.Version, next.UpdatedAt = prev.ID, prev.CreatedBy, prev.CreatedAt, prev.Version+1, now
			if err := st.UpdatePolicy(ctx, &next); err != nil {
				return rollback(err)
			}
			undo = append(undo, func(c context.Context) error { return st.UpdatePolicy(c, &prev) })
		case "delete":
			prev := *op.prev
			if err := st.DeletePolicy(ctx, sc.tenant.ID, prev.ID); err != nil {
				return rollback(err)
			}
			undo = append(undo, func(c context.Context) error { return st.CreatePolicy(c, &prev) })
		}
	}
	for _, op := range pl.roles {
		if op.op != "delete" {
			continue
		}
		prev := *op.prev
		if err := st.DeleteRole(ctx, sc.tenant.ID, prev.Name); err != nil {
			return rollback(err)
		}
		undo = append(undo, func(c context.Context) error { return st.PutRole(c, &prev) })
	}
	return nil
}

// record audits and publishes access.applied.
func (s *Service) record(ctx context.Context, p *domain.Principal, sc scope, doc *domain.AccessConfigDoc, changes []string) {
	plan := strings.Join(changes, "; ")
	if plan == "" {
		plan = "no changes"
	}
	details := map[string]string{
		"scope":   sc.label(),
		"hash":    doc.Hash,
		"changes": plan,
		"count":   fmt.Sprint(len(changes)),
	}
	resourceType, resourceID := "tenant", sc.tenant.ID
	if sc.tree != nil {
		details["tree"] = sc.tree.Name
		resourceType, resourceID = "tree", sc.tree.ID
	}
	if s.d.Audit != nil {
		e := domain.AuditEvent{
			TenantID: sc.tenant.ID, Actor: actor(p), Action: domain.EventAccessApplied,
			ResourceType: resourceType, ResourceID: resourceID, Details: details, At: s.now(),
		}
		if p != nil {
			e.ActorID = p.AccountID
		}
		s.d.Audit.Record(ctx, e)
	}
	if s.d.Bus != nil {
		s.d.Bus.Publish(domain.Event{
			Type: domain.EventAccessApplied, TenantID: sc.tenant.ID, TreeID: sc.treeID(), Actor: actor(p),
			Payload: map[string]any{"changes": changes, "hash": doc.Hash, "scope": sc.label()},
			At:      s.now(),
		})
	}
}

// ---- tree deletion ----

// Run removes the access document and the config-managed policies of
// deleted trees (event tree.deleted, §13.14) until ctx is cancelled. It
// returns immediately when there is no event bus.
func (s *Service) Run(ctx context.Context) {
	if s.d.Bus == nil {
		return
	}
	ch, cancel := s.d.Bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeDeleted}})
	defer cancel()
	for {
		select {
		case <-ctx.Done():
			return
		case e, ok := <-ch:
			if !ok {
				return
			}
			// hardening (§15.10): attribute the recorded policy versions to
			// whoever deleted the tree.
			if err := s.TreeDeletedBy(ctx, e.TenantID, e.TreeID, e.Actor, eventTreeName(e.Payload)); err != nil {
				s.d.Logger.WarnContext(ctx, "accesscfg: cleaning up a deleted tree failed",
					slog.String("tree_id", e.TreeID), slog.Any("error", err))
			}
		}
	}
}

// TreeDeleted removes the tree's access document and the policies applied
// from it.
func (s *Service) TreeDeleted(ctx context.Context, tenantID, treeID string) error {
	return s.TreeDeletedBy(ctx, tenantID, treeID, "", "")
}

// hardening (§15.10): policy versions on tree deletion.
//
// TreeDeletedBy is TreeDeleted that records a "delete" PolicyVersion (and
// publishes policy.changed) for every config-managed policy it removes,
// attributed to actor ("system" when empty) with the comment
// "tree <treeName> deleted" (the tree id when the name is unknown).
func (s *Service) TreeDeletedBy(ctx context.Context, tenantID, treeID, actor, treeName string) error {
	if tenantID == "" || treeID == "" {
		return nil
	}
	if actor == "" {
		actor = "system"
	}
	if treeName == "" {
		treeName = treeID
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.d.Store.DeleteDoc(ctx, Collection, domain.DocKey(tenantID, treeID)); err != nil && !errors.Is(err, domain.ErrNotFound) {
		return err
	}
	return policyhist.New(s.d.Store, s.d.Bus, s.d.Logger).Track(ctx, actor, tenantID, "tree "+treeName+" deleted",
		func() error { return s.deleteTreePolicies(ctx, tenantID, treeID) })
}

// eventTreeName is the "name" of a tree.deleted payload ("" when absent).
func eventTreeName(payload any) string {
	switch p := payload.(type) {
	case map[string]any:
		name, _ := p["name"].(string)
		return name
	case map[string]string:
		return p["name"]
	}
	raw, err := json.Marshal(payload)
	if err != nil {
		return ""
	}
	var v struct {
		Name string `json:"name"`
	}
	_ = json.Unmarshal(raw, &v)
	return v.Name
}

// end hardening (§15.10)

func (s *Service) deleteTreePolicies(ctx context.Context, tenantID, treeID string) error {
	pols, err := s.d.Store.ListPolicies(ctx, tenantID)
	if err != nil {
		return err
	}
	for _, p := range pols {
		if p.ManagedBy == domain.ManagedByConfig && p.TreeID == treeID {
			if err := s.d.Store.DeletePolicy(ctx, tenantID, p.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
				return err
			}
		}
	}
	return nil
}
