// Package fork forks trees across tenants and keeps forks in sync with
// their upstream (docs/ENGINEERING_PLAN.md §16.2, specs/tree/Forks.md).
//
// A fork is an ordinary tree with Tree.ForkOf set to its upstream's id. It
// gets copies of the upstream's branches (all or a chosen set) and tags;
// branch protections, license rules, grants, webhooks and other settings
// are not copied. Objects are content-addressed and shared, so forking
// copies only snapshot records (package snapcopy). The relation is recorded
// in the `forks` collection (key DocKey(upstreamTreeID, forkTreeID) →
// Record). Deleting the upstream leaves its forks intact: Tree.ForkOf keeps
// the id and Upstream reports upstream_missing.
package fork

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"regexp"
	"slices"
	"sort"
	"strings"
	"time"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/fork/snapcopy"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Coll is the DocStore collection of fork records.
const Coll = "forks"

// Deps are the collaborators of the fork package. Store is required;
// the others may be nil unless documented otherwise.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// TreeQuota enforces the destination tenant's MaxTrees quota
// (limits.Limiter); it returns a *limits.QuotaError when exceeded.
type TreeQuota interface {
	CheckTreeCreate(ctx context.Context, t *domain.Tenant) error
}

// Exporter enforces the license redistribution rule (license.Checker).
type Exporter interface {
	CheckExport(ctx context.Context, p *domain.Principal, tree *domain.Tree, paths []string) error
}

// Options configure optional collaborators.
type Options struct {
	Quota   TreeQuota
	License Exporter
}

// Service is the fork component.
type Service struct {
	d    Deps
	opts Options
	now  func() time.Time
}

// New returns a Service.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, now: func() time.Time { return time.Now().UTC() }}
}

// Configure sets the quota and license collaborators (nil = not enforced).
func (s *Service) Configure(o Options) { s.opts = o }

// Record is a stored fork relation (collection `forks`).
type Record struct {
	UpstreamTreeID   string     `json:"upstream_tree_id"`
	UpstreamTenantID string     `json:"upstream_tenant_id"`
	TreeID           string     `json:"tree_id"`
	TenantID         string     `json:"tenant_id"`
	Branches         []string   `json:"branches"`
	CreatedBy        string     `json:"created_by"`
	CreatedAt        time.Time  `json:"created_at"`
	LastSyncAt       *time.Time `json:"last_sync_at,omitempty"`
}

// Input describes a new fork.
type Input struct {
	// Tenant receives the fork (required).
	Tenant *domain.Tenant
	// Name of the fork (default: the upstream's name).
	Name string
	// Description (default: the upstream's).
	Description string
	// Visibility (default: the upstream's); it may not be wider than the
	// upstream's (private < internal < public).
	Visibility string
	// Branches to copy (default: all). Tags are always copied.
	Branches []string
}

var treeNameRe = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$`)

func invalidf(format string, args ...any) error {
	return fmt.Errorf(format+": %w", append(args, domain.ErrInvalid)...)
}

// visibilityRank orders visibilities from the narrowest.
func visibilityRank(v string) int {
	switch v {
	case domain.VisibilityPublic:
		return 2
	case domain.VisibilityInternal:
		return 1
	default:
		return 0
	}
}

// Fork forks src into in.Tenant. The caller needs tree:fork on src and
// tree:create in the destination tenant; the destination's MaxTrees quota
// applies, and when the fork leaves src's tenant every file of the copied
// branch and tag tips must be redistributable to the destination tenant
// (license rules: same rule as archives).
func (s *Service) Fork(ctx context.Context, p *domain.Principal, src *domain.Tree, in Input) (*domain.Tree, error) {
	if p == nil {
		return nil, fmt.Errorf("forking requires a signed-in caller: %w", domain.ErrUnauthenticated)
	}
	if src == nil || in.Tenant == nil {
		return nil, invalidf("fork: source tree and destination tenant are required")
	}
	dst := in.Tenant
	if err := s.authorize(ctx, p, domain.ActTreeFork, src.TenantID, src); err != nil {
		return nil, err
	}
	if err := s.authorize(ctx, p, domain.ActTreeCreate, dst.ID, nil); err != nil {
		return nil, err
	}
	name := strings.TrimSpace(in.Name)
	if name == "" {
		name = src.Name
	}
	if !treeNameRe.MatchString(name) || strings.HasSuffix(name, ".") {
		return nil, invalidf("tree name must be 1-100 characters of letters, digits, '.', '_' and '-', starting with a letter or digit")
	}
	vis := in.Visibility
	if vis == "" {
		vis = src.Visibility
	}
	switch vis {
	case domain.VisibilityPrivate, domain.VisibilityInternal, domain.VisibilityPublic:
	default:
		return nil, invalidf("visibility must be one of private, internal, public")
	}
	if visibilityRank(vis) > visibilityRank(src.Visibility) {
		return nil, invalidf("a fork may not be more visible than its upstream (%s)", orPrivate(src.Visibility))
	}
	desc := in.Description
	if desc == "" {
		desc = src.Description
	}
	if len(desc) > 4096 {
		return nil, invalidf("description must be at most 4096 bytes")
	}

	branches, err := s.pickBranches(ctx, src, in.Branches)
	if err != nil {
		return nil, err
	}
	tags, err := s.d.Store.ListTags(ctx, src.ID)
	if err != nil {
		return nil, fmt.Errorf("fork: list tags: %w", err)
	}

	if s.opts.Quota != nil {
		if err := s.opts.Quota.CheckTreeCreate(ctx, dst); err != nil {
			return nil, err
		}
	}
	if err := s.checkLicense(ctx, p, src, dst, branches, tags); err != nil {
		return nil, err
	}

	now := s.now()
	fork := &domain.Tree{
		ID:            newID(),
		TenantID:      dst.ID,
		Name:          name,
		Description:   desc,
		DefaultBranch: defaultBranch(src, branches),
		Visibility:    vis,
		CreatedAt:     now,
		UpdatedAt:     now,
		ForkOf:        src.ID,
	}
	if err := s.d.Store.CreateTree(ctx, fork); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return nil, fmt.Errorf("tree %s/%s already exists: %w", dst.Slug, name, domain.ErrAlreadyExists)
		}
		return nil, fmt.Errorf("fork: create tree: %w", err)
	}
	names, err := s.populate(ctx, src, fork, branches, tags, now)
	if err != nil {
		if derr := s.d.Store.DeleteTree(context.WithoutCancel(ctx), fork.ID); derr != nil {
			s.d.Logger.Warn("fork: roll back failed fork", slog.String("tree_id", fork.ID), slog.Any("error", derr))
		}
		return nil, err
	}
	rec := Record{
		UpstreamTreeID: src.ID, UpstreamTenantID: src.TenantID, TreeID: fork.ID, TenantID: dst.ID,
		Branches: names, CreatedBy: p.Username, CreatedAt: now,
	}
	if err := s.d.Store.PutDoc(ctx, Coll, domain.DocKey(src.ID, fork.ID), rec); err != nil {
		if derr := s.d.Store.DeleteTree(context.WithoutCancel(ctx), fork.ID); derr != nil {
			s.d.Logger.Warn("fork: roll back failed fork", slog.String("tree_id", fork.ID), slog.Any("error", derr))
		}
		return nil, fmt.Errorf("fork: record fork: %w", err)
	}

	upSlug := s.tenantSlug(ctx, src.TenantID)
	s.publish(domain.Event{
		Type: domain.EventTreeForked, TenantID: src.TenantID, TreeID: src.ID, Actor: p.Username,
		Payload: map[string]any{
			"fork_tree_id": fork.ID, "fork_tenant": dst.Slug, "fork_name": fork.Name,
			"upstream_tenant": upSlug, "upstream_name": src.Name, "branches": names,
		},
	})
	s.publish(domain.Event{
		Type: domain.EventTreeCreated, TenantID: dst.ID, TreeID: fork.ID, Actor: p.Username,
		Payload: map[string]any{"name": fork.Name, "tenant": dst.Slug, "fork_of": src.ID},
	})
	details := map[string]string{
		"name": fork.Name, "upstream": upSlug + "/" + src.Name, "upstream_tree_id": src.ID,
		"branches": fmt.Sprint(len(names)), "tags": fmt.Sprint(len(tags)), "visibility": vis,
	}
	s.audit(ctx, p, domain.AuditEvent{TenantID: dst.ID, Action: "tree.fork", ResourceType: "tree", ResourceID: fork.ID, Details: details})
	if dst.ID != src.TenantID {
		s.audit(ctx, p, domain.AuditEvent{TenantID: src.TenantID, Action: "tree.fork", ResourceType: "tree", ResourceID: src.ID,
			Details: map[string]string{"fork": dst.Slug + "/" + fork.Name, "fork_tree_id": fork.ID}})
	}
	return fork, nil
}

func orPrivate(v string) string {
	if v == "" {
		return domain.VisibilityPrivate
	}
	return v
}

// pickBranches returns the branches of src to copy, sorted by name.
func (s *Service) pickBranches(ctx context.Context, src *domain.Tree, want []string) ([]domain.Branch, error) {
	all, err := s.d.Store.ListBranches(ctx, src.ID)
	if err != nil {
		return nil, fmt.Errorf("fork: list branches: %w", err)
	}
	sort.Slice(all, func(i, j int) bool { return all[i].Name < all[j].Name })
	if len(want) == 0 {
		return all, nil
	}
	var out []domain.Branch
	for _, name := range want {
		name = strings.TrimSpace(name)
		i := slices.IndexFunc(all, func(b domain.Branch) bool { return b.Name == name })
		if i < 0 {
			return nil, fmt.Errorf("branch '%s' not found in the upstream: %w", name, domain.ErrNotFound)
		}
		if !slices.ContainsFunc(out, func(b domain.Branch) bool { return b.Name == name }) {
			out = append(out, all[i])
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Name < out[j].Name })
	return out, nil
}

// defaultBranch keeps src's default branch when it is copied, else the
// first copied branch.
func defaultBranch(src *domain.Tree, branches []domain.Branch) string {
	if len(branches) == 0 || slices.ContainsFunc(branches, func(b domain.Branch) bool { return b.Name == src.DefaultBranch }) {
		if src.DefaultBranch == "" {
			return "main"
		}
		return src.DefaultBranch
	}
	return branches[0].Name
}

// populate copies the branches and tags of src into fork and returns the
// copied branch names.
func (s *Service) populate(ctx context.Context, src, fork *domain.Tree, branches []domain.Branch, tags []domain.Tag, now time.Time) ([]string, error) {
	names := make([]string, 0, len(branches))
	for _, b := range branches {
		tip, err := snapcopy.Copy(ctx, s.d.Store, src.ID, fork.ID, b.Tip)
		if err != nil {
			return nil, fmt.Errorf("fork: copy branch '%s': %w", b.Name, err)
		}
		nb := &domain.Branch{TreeID: fork.ID, Name: b.Name, Tip: tip, CreatedAt: now, UpdatedAt: now, Description: b.Description}
		if err := s.d.Store.PutBranch(ctx, nb); err != nil {
			return nil, fmt.Errorf("fork: create branch '%s': %w", b.Name, err)
		}
		names = append(names, b.Name)
	}
	if len(branches) == 0 {
		// An empty upstream: the fork starts with an empty default branch.
		if err := s.d.Store.PutBranch(ctx, &domain.Branch{TreeID: fork.ID, Name: fork.DefaultBranch, CreatedAt: now, UpdatedAt: now}); err != nil {
			return nil, fmt.Errorf("fork: create branch '%s': %w", fork.DefaultBranch, err)
		}
	}
	for _, t := range tags {
		target, err := snapcopy.Copy(ctx, s.d.Store, src.ID, fork.ID, t.Target)
		if err != nil {
			return nil, fmt.Errorf("fork: copy tag '%s': %w", t.Name, err)
		}
		// Tag signatures cover the target id, which changes: copies are
		// unsigned (specs/tree/Forks.md §3).
		nt := &domain.Tag{TreeID: fork.ID, Name: t.Name, Target: target, Message: t.Message, Tagger: t.Tagger, CreatedAt: t.CreatedAt}
		if err := s.d.Store.PutTag(ctx, nt); err != nil {
			return nil, fmt.Errorf("fork: create tag '%s': %w", t.Name, err)
		}
	}
	return names, nil
}

// checkLicense applies the redistribution rule when the fork leaves src's
// tenant: the caller and the destination tenant (as a non-member of the
// upstream's tenant) must both be allowed to export every path of the
// copied branch and tag tips.
func (s *Service) checkLicense(ctx context.Context, p *domain.Principal, src *domain.Tree, dst *domain.Tenant, branches []domain.Branch, tags []domain.Tag) error {
	if s.opts.License == nil || dst.ID == src.TenantID {
		return nil
	}
	rules, err := s.d.Store.ListLicenseRules(ctx, src.ID)
	if err != nil && !errors.Is(err, domain.ErrNotFound) {
		return fmt.Errorf("fork: list license rules: %w", err)
	}
	if len(rules) == 0 {
		return nil
	}
	tips := map[string]bool{}
	for _, b := range branches {
		if b.Tip != "" {
			tips[b.Tip] = true
		}
	}
	for _, t := range tags {
		tips[t.Target] = true
	}
	paths, err := s.tipPaths(ctx, tips)
	if err != nil {
		return err
	}
	if len(paths) == 0 {
		return nil
	}
	// The destination tenant, as an outside tenant of the upstream: grantee
	// rules naming it apply, membership of the caller does not.
	outside := &domain.Principal{Username: "tenant:" + dst.Slug, TenantID: dst.ID}
	for _, who := range []*domain.Principal{p, outside} {
		if err := s.opts.License.CheckExport(ctx, who, src, paths); err != nil {
			var denied *domain.PushDenied
			if errors.As(err, &denied) {
				return &domain.PushDenied{
					Kind: denied.Kind, Path: denied.Path, License: denied.License,
					Reason: fmt.Sprintf("cannot fork into tenant %q: %s", dst.Slug, denied.Reason),
				}
			}
			return err
		}
	}
	return nil
}

// tipPaths returns the sorted union of the manifest paths of tips.
func (s *Service) tipPaths(ctx context.Context, tips map[string]bool) ([]string, error) {
	if len(tips) == 0 {
		return nil, nil
	}
	if s.d.Objects == nil {
		return nil, errors.New("fork: no object store configured for the license check")
	}
	seen := map[hash.Hash]bool{}
	set := map[string]bool{}
	for id := range tips {
		snap, err := s.d.Store.GetSnapshot(ctx, id)
		if err != nil {
			return nil, fmt.Errorf("fork: snapshot %s: %w", id, err)
		}
		if seen[snap.ManifestHash] {
			continue
		}
		seen[snap.ManifestHash] = true
		data, err := s.d.Objects.Get(ctx, snap.ManifestHash)
		if err != nil {
			return nil, fmt.Errorf("fork: read manifest %s: %w", snap.ManifestHash.Hex(), err)
		}
		m := &syncpb.ManifestRecord{}
		if err := proto.Unmarshal(data, m); err != nil {
			return nil, fmt.Errorf("fork: decode manifest %s: %w", snap.ManifestHash.Hex(), err)
		}
		for _, e := range m.GetEntries() {
			set[e.GetPath()] = true
		}
	}
	out := make([]string, 0, len(set))
	for p := range set {
		out = append(out, p)
	}
	sort.Strings(out)
	return out, nil
}

// ---- helpers ----

func (s *Service) authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	if s.d.Authz == nil {
		return fmt.Errorf("fork: no authorizer configured: %w", domain.ErrForbidden)
	}
	return s.d.Authz.Authorize(ctx, p, act, tenantID, tree)
}

func (s *Service) canRead(ctx context.Context, p *domain.Principal, t *domain.Tree) bool {
	return s.authorize(ctx, p, domain.ActTreeRead, t.TenantID, t) == nil
}

func (s *Service) tenantSlug(ctx context.Context, id string) string {
	if t, err := s.d.Store.GetTenant(ctx, id); err == nil {
		return t.Slug
	}
	return id
}

func (s *Service) publish(e domain.Event) {
	if s.d.Bus == nil {
		return
	}
	e.At = s.now()
	s.d.Bus.Publish(e)
}

func (s *Service) audit(ctx context.Context, p *domain.Principal, e domain.AuditEvent) {
	if s.d.Audit == nil {
		return
	}
	e.ID = newID()
	e.Actor = "system"
	if p != nil {
		e.ActorID, e.Actor = p.AccountID, p.Username
	}
	e.At = s.now()
	s.d.Audit.Record(context.WithoutCancel(ctx), e)
}
