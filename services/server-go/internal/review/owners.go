package review

// v0.7 deeper review (docs/ENGINEERING_PLAN.md §15.7,
// specs/server/ReviewWorkflow.md): path owners.
//
// A tree's owners come from its server configuration (doc collection
// path_owners, key = tree id) or, when there is none, from an OWNERS file at
// the root of the tree's default branch. Owners are "user:<username>" or
// "team:<slug>" (a team of the tree's tenant). Rules use the "**" glob syntax
// of license rules (license.Match); the last matching rule wins, and a rule
// without owners un-owns the paths it matches.
//
// Owners of the paths a merge request changes are requested as reviewers
// when it is opened (or marked ready) and when its source branch moves.
// BranchProtection.RequireOwnerApproval makes merging wait until every
// owned changed path has a current approval from one of its owners.

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"sort"
	"strings"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/license"
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// OwnersCollection holds the server owner configuration of trees (key =
// tree id, value domain.PathOwners).
const OwnersCollection = "path_owners"

// OwnersFile is the path of the owners file read when a tree has no server
// configuration (at the root of the default branch).
const OwnersFile = "OWNERS"

// Owner configuration sources (OwnersConfig.Source, MROwners.Source).
const (
	OwnersSourceConfig = "config" // server configuration (PUT {tree}/owners)
	OwnersSourceFile   = "file"   // the OWNERS file of the default branch
	OwnersSourceNone   = "none"   // no owners
)

// Bounds of an owner configuration.
const (
	MaxOwnerRules     = 1000
	MaxOwnersPerRule  = 100
	MaxOwnerPattern   = 512
	MaxOwnerName      = 100
	MaxOwnersFileSize = 256 << 10
)

// BlockerOwners is the blocker kind of missing owner approvals.
const BlockerOwners = "owners"

// OwnersConfig is the owner configuration of a tree as served by GET
// {tree}/owners.
type OwnersConfig struct {
	domain.PathOwners
	// Source is "config", "file" or "none".
	Source string `json:"source"`
	// Errors lists OWNERS file lines that could not be parsed.
	Errors []string `json:"errors"`
}

// PathOwnership is one path a merge request changes and its owners.
type PathOwnership struct {
	Path   string   `json:"path"`
	Owners []string `json:"owners"`
	// Approved is set when one of Owners approved the current source tip.
	Approved   bool     `json:"approved"`
	ApprovedBy []string `json:"approved_by"`
}

// MROwners reports the owners of the paths a merge request changes.
type MROwners struct {
	Source string `json:"source"`
	// Required is set when the target's protection requires owner approval.
	Required bool `json:"required"`
	// Satisfied is set when every owned path has an owner approval.
	Satisfied bool            `json:"satisfied"`
	Paths     []PathOwnership `json:"paths"`
	// Missing lists the owned paths without an owner approval.
	Missing []string `json:"missing"`
}

// OwnersSummary is the owner part of MergeChecks.
type OwnersSummary struct {
	Required bool     `json:"required"`
	Missing  []string `json:"missing"`
}

// NormalizeOwner returns the canonical form ("user:<name>" or
// "team:<slug>") of an owner or reviewer reference. It accepts "user:x",
// "team:y", "@x" / "x" (users) and "@tenant/slug" / "tenant/slug" (teams).
func NormalizeOwner(ref string) (string, error) {
	s := strings.TrimSpace(ref)
	kind, name := "", ""
	switch {
	case strings.HasPrefix(s, "user:"):
		kind, name = "user", s[len("user:"):]
	case strings.HasPrefix(s, "team:"):
		kind, name = "team", s[len("team:"):]
	default:
		s = strings.TrimPrefix(s, "@")
		if i := strings.LastIndexByte(s, '/'); i >= 0 {
			kind, name = "team", s[i+1:]
		} else {
			kind, name = "user", s
		}
	}
	if name == "" || len(name) > MaxOwnerName || !validOwnerName(name) {
		return "", fmt.Errorf("%q is not a valid owner (use user:NAME, team:SLUG, @NAME or @TENANT/TEAM): %w", ref, domain.ErrInvalid)
	}
	if kind == "team" {
		name = strings.ToLower(name)
	}
	return kind + ":" + name, nil
}

func validOwnerName(name string) bool {
	for _, r := range name {
		switch {
		case r >= 'a' && r <= 'z', r >= 'A' && r <= 'Z', r >= '0' && r <= '9', r == '-', r == '_', r == '.':
		default:
			return false
		}
	}
	return true
}

// ParseOwnersFile parses the OWNERS file format: one rule per line,
// "pattern owner..." separated by whitespace; '#' starts a comment (at the
// start of a line or of a word); blank lines are ignored. Lines that cannot
// be parsed are skipped and reported in errs ("OWNERS:3: ...").
func ParseOwnersFile(data []byte) (rules []domain.PathOwnerRule, errs []string) {
	rules, errs = []domain.PathOwnerRule{}, []string{}
	sc := bufio.NewScanner(bytes.NewReader(data))
	sc.Buffer(make([]byte, 0, 4096), MaxOwnersFileSize)
	n := 0
	for sc.Scan() {
		n++
		fields := strings.Fields(sc.Text())
		for i, f := range fields {
			if strings.HasPrefix(f, "#") {
				fields = fields[:i]
				break
			}
		}
		if len(fields) == 0 {
			continue
		}
		bad := func(format string, args ...any) {
			errs = append(errs, fmt.Sprintf("%s:%d: %s", OwnersFile, n, fmt.Sprintf(format, args...)))
		}
		pattern := fields[0]
		if err := validateOwnerPattern(pattern); err != nil {
			bad("%v", errors.Unwrap(err))
			continue
		}
		if len(fields) == 1 {
			bad("no owners for %q", pattern)
			continue
		}
		rule := domain.PathOwnerRule{Pattern: pattern, Owners: []string{}}
		ok := true
		for _, f := range fields[1:] {
			o, err := NormalizeOwner(f)
			if err != nil {
				bad("%q is not a valid owner", f)
				ok = false
				break
			}
			if !slices.Contains(rule.Owners, o) {
				rule.Owners = append(rule.Owners, o)
			}
		}
		if !ok {
			continue
		}
		if len(rules) == MaxOwnerRules {
			bad("too many rules (at most %d)", MaxOwnerRules)
			break
		}
		rules = append(rules, rule)
	}
	if err := sc.Err(); err != nil {
		errs = append(errs, fmt.Sprintf("%s: %v", OwnersFile, err))
	}
	return rules, errs
}

type invalidPattern struct{ msg string }

func (e invalidPattern) Error() string { return e.msg }

func validateOwnerPattern(p string) error {
	switch {
	case strings.TrimSpace(p) == "":
		return fmt.Errorf("owner pattern: %w", invalidPattern{"empty pattern"})
	case len(p) > MaxOwnerPattern || !utf8.ValidString(p) || strings.ContainsAny(p, "\x00\n\r"):
		return fmt.Errorf("owner pattern: %w", invalidPattern{fmt.Sprintf("pattern %q is malformed", p)})
	}
	return nil
}

// NormalizeOwnerRules validates rules for storage: patterns are non-empty
// globs, owners are normalized and deduplicated.
func NormalizeOwnerRules(rules []domain.PathOwnerRule) ([]domain.PathOwnerRule, error) {
	if len(rules) > MaxOwnerRules {
		return nil, fmt.Errorf("at most %d owner rules are allowed: %w", MaxOwnerRules, domain.ErrInvalid)
	}
	out := make([]domain.PathOwnerRule, 0, len(rules))
	for i, r := range rules {
		r.Pattern = strings.TrimSpace(r.Pattern)
		if err := validateOwnerPattern(r.Pattern); err != nil {
			return nil, fmt.Errorf("rule %d: %s: %w", i+1, errors.Unwrap(err), domain.ErrInvalid)
		}
		if len(r.Owners) > MaxOwnersPerRule {
			return nil, fmt.Errorf("rule %d: at most %d owners per rule: %w", i+1, MaxOwnersPerRule, domain.ErrInvalid)
		}
		owners := []string{}
		for _, o := range r.Owners {
			n, err := NormalizeOwner(o)
			if err != nil {
				return nil, fmt.Errorf("rule %d: %w", i+1, err)
			}
			if !slices.Contains(owners, n) {
				owners = append(owners, n)
			}
		}
		out = append(out, domain.PathOwnerRule{Pattern: r.Pattern, Owners: owners})
	}
	return out, nil
}

// OwnersOf returns the owners of path under rules (the last matching rule
// wins; nil when no rule matches or the rule has no owners).
func OwnersOf(rules []domain.PathOwnerRule, path string) []string {
	for i := len(rules) - 1; i >= 0; i-- {
		if license.Match(rules[i].Pattern, path) {
			if len(rules[i].Owners) == 0 {
				return nil
			}
			return rules[i].Owners
		}
	}
	return nil
}

// Owners returns the owner configuration of tree. It requires
// domain.ActTreeRead.
func (s *Service) Owners(ctx context.Context, p *domain.Principal, tree *domain.Tree) (*OwnersConfig, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	return s.ownersConfig(ctx, tree)
}

// PutOwners replaces the server owner configuration of tree (an empty list
// removes it, so the OWNERS file applies again). It requires
// domain.ActTreeAdmin; user and team owners must exist.
func (s *Service) PutOwners(ctx context.Context, p *domain.Principal, tree *domain.Tree, rules []domain.PathOwnerRule) (*OwnersConfig, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeAdmin, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	rules, err := NormalizeOwnerRules(rules)
	if err != nil {
		return nil, err
	}
	checked := map[string]bool{}
	for _, r := range rules {
		for _, o := range r.Owners {
			if checked[o] {
				continue
			}
			checked[o] = true
			if err := s.checkPrincipalRef(ctx, tree, o); err != nil {
				return nil, err
			}
		}
	}
	if len(rules) == 0 {
		if err := s.store.DeleteDoc(ctx, OwnersCollection, tree.ID); err != nil && !isNotFound(err) {
			return nil, fmt.Errorf("review: delete owners: %w", err)
		}
	} else {
		doc := domain.PathOwners{TreeID: tree.ID, Rules: rules, UpdatedBy: p.Username, UpdatedAt: s.now()}
		if err := s.store.PutDoc(ctx, OwnersCollection, tree.ID, doc); err != nil {
			return nil, fmt.Errorf("review: store owners: %w", err)
		}
	}
	s.recordTree(ctx, "owners.update", p, tree, map[string]string{"rules": fmt.Sprint(len(rules))})
	return s.ownersConfig(ctx, tree)
}

// checkPrincipalRef verifies that a normalized "user:x" / "team:y" exists.
func (s *Service) checkPrincipalRef(ctx context.Context, tree *domain.Tree, ref string) error {
	kind, name, _ := strings.Cut(ref, ":")
	var err error
	if kind == "team" {
		_, err = s.store.GetTeamBySlug(ctx, tree.TenantID, name)
	} else {
		_, err = s.store.GetAccountByUsername(ctx, name)
	}
	switch {
	case err == nil:
		return nil
	case isNotFound(err):
		what := "user"
		if kind == "team" {
			what = "team"
		}
		return fmt.Errorf("%s %q does not exist: %w", what, name, domain.ErrInvalid)
	default:
		return fmt.Errorf("review: look up %s: %w", ref, err)
	}
}

// ownersConfig loads the effective owner configuration of tree.
func (s *Service) ownersConfig(ctx context.Context, tree *domain.Tree) (*OwnersConfig, error) {
	doc, err := meta.GetDocT[domain.PathOwners](ctx, s.store, OwnersCollection, tree.ID)
	switch {
	case err == nil && len(doc.Rules) > 0:
		doc.TreeID = tree.ID
		return &OwnersConfig{PathOwners: *doc, Source: OwnersSourceConfig, Errors: []string{}}, nil
	case err != nil && !errors.Is(err, domain.ErrNotFound):
		return nil, fmt.Errorf("review: get owners: %w", err)
	}
	cfg := &OwnersConfig{
		PathOwners: domain.PathOwners{TreeID: tree.ID, Rules: []domain.PathOwnerRule{}},
		Source:     OwnersSourceNone,
		Errors:     []string{},
	}
	data, found, err := s.ownersFile(ctx, tree)
	if err != nil {
		return nil, err
	}
	if found {
		cfg.Source = OwnersSourceFile
		cfg.Rules, cfg.Errors = ParseOwnersFile(data)
	}
	return cfg, nil
}

// ownersFile reads OWNERS at the root of the default branch.
func (s *Service) ownersFile(ctx context.Context, tree *domain.Tree) ([]byte, bool, error) {
	if s.objects == nil || tree.DefaultBranch == "" {
		return nil, false, nil
	}
	b, err := s.store.GetBranch(ctx, tree.ID, tree.DefaultBranch)
	if err != nil || b.Tip == "" {
		if err != nil && !isNotFound(err) {
			return nil, false, fmt.Errorf("review: get branch '%s': %w", tree.DefaultBranch, err)
		}
		return nil, false, nil
	}
	m, err := s.snapshotManifest(ctx, b.Tip)
	if errors.Is(err, objstore.ErrNotFound) {
		// A tip without its manifest (pruned or not uploaded): no file.
		s.logger().Warn("review: owners: manifest of the default branch is missing",
			slog.String("tree_id", tree.ID), slog.String("tip", b.Tip))
		return nil, false, nil
	}
	if err != nil {
		return nil, false, err
	}
	for _, e := range m.GetEntries() {
		if e.GetPath() != OwnersFile {
			continue
		}
		if e.GetSize() > MaxOwnersFileSize {
			return nil, false, nil
		}
		data, err := content.ReadEntry(ctx, s.objects, e)
		if err != nil {
			return nil, false, fmt.Errorf("review: read %s: %w", OwnersFile, err)
		}
		return data, true, nil
	}
	return nil, false, nil
}

// snapshotManifest loads the manifest of snapshot id ("" = empty).
func (s *Service) snapshotManifest(ctx context.Context, id string) (*syncpb.ManifestRecord, error) {
	if id == "" {
		return &syncpb.ManifestRecord{}, nil
	}
	sn, err := s.store.GetSnapshot(ctx, id)
	if err != nil {
		return nil, fmt.Errorf("review: snapshot %s: %w", shortID(id), err)
	}
	return s.loadManifest(ctx, sn.ManifestHash)
}

// mergeBaseOf returns the merge base of a and b ("" when unrelated or b is
// empty).
func (s *Service) mergeBaseOf(ctx context.Context, a, b string) (string, error) {
	if a == "" || b == "" {
		return "", nil
	}
	base, ok, err := merge3.MergeBase(a, b, func(id string) ([]string, error) {
		sn, err := s.store.GetSnapshot(ctx, id)
		if err != nil {
			return nil, fmt.Errorf("review: snapshot %s: %w", shortID(id), err)
		}
		return sn.Parents, nil
	})
	if err != nil {
		return "", fmt.Errorf("review: merge base: %w", err)
	}
	if !ok {
		return "", nil
	}
	return base, nil
}

// changedPaths returns the sorted paths the source brings relative to its
// merge base with the target (nil without an object store).
func (s *Service) changedPaths(ctx context.Context, sourceTip, targetTip string) ([]string, error) {
	if s.objects == nil || sourceTip == "" {
		return nil, nil
	}
	base, err := s.mergeBaseOf(ctx, targetTip, sourceTip)
	if err != nil {
		return nil, err
	}
	if base == sourceTip {
		return nil, nil
	}
	bm, err := s.snapshotManifest(ctx, base)
	if err != nil {
		return nil, err
	}
	sm, err := s.snapshotManifest(ctx, sourceTip)
	if err != nil {
		return nil, err
	}
	return diffPaths(bm, sm), nil
}

// diffPaths lists the paths whose entries differ between a and b.
func diffPaths(a, b *syncpb.ManifestRecord) []string {
	key := func(e *syncpb.ManifestEntry) string {
		return fmt.Sprintf("%x|%t|%t", e.GetBlobHash().GetValue(), e.GetExecutable(), e.GetChunked())
	}
	am := make(map[string]string, len(a.GetEntries()))
	for _, e := range a.GetEntries() {
		am[e.GetPath()] = key(e)
	}
	var out []string
	seen := make(map[string]bool, len(b.GetEntries()))
	for _, e := range b.GetEntries() {
		seen[e.GetPath()] = true
		if k, ok := am[e.GetPath()]; !ok || k != key(e) {
			out = append(out, e.GetPath())
		}
	}
	for p := range am {
		if !seen[p] {
			out = append(out, p)
		}
	}
	sort.Strings(out)
	return out
}

// ownerApprovers resolves which current approvers match an owner list.
type ownerApprovers struct {
	s         *Service
	tenantID  string
	approvals []domain.Approval          // current, non-author approvals
	teams     map[string]map[string]bool // slug → member account ids
}

func (s *Service) newOwnerApprovers(tree *domain.Tree, mr *domain.MergeRequest, sourceTip string) *ownerApprovers {
	oa := &ownerApprovers{s: s, tenantID: tree.TenantID, teams: map[string]map[string]bool{}}
	seen := map[string]bool{}
	for i := len(mr.Approvals) - 1; i >= 0; i-- {
		a := mr.Approvals[i]
		if a.AccountID == "" || a.AccountID == mr.AuthorID || seen[a.AccountID] {
			continue
		}
		seen[a.AccountID] = true // the latest decision of a reviewer counts
		if isApproval(a) && isCurrent(a, sourceTip) {
			oa.approvals = append(oa.approvals, a)
		}
	}
	slices.SortFunc(oa.approvals, func(a, b domain.Approval) int { return strings.Compare(a.Username, b.Username) })
	return oa
}

func (oa *ownerApprovers) members(ctx context.Context, slug string) (map[string]bool, error) {
	if m, ok := oa.teams[slug]; ok {
		return m, nil
	}
	m := map[string]bool{}
	team, err := oa.s.store.GetTeamBySlug(ctx, oa.tenantID, slug)
	switch {
	case err == nil:
		ms, err := oa.s.store.ListTeamMembers(ctx, team.ID)
		if err != nil {
			return nil, fmt.Errorf("review: list team members: %w", err)
		}
		for _, x := range ms {
			m[x.AccountID] = true
		}
	case !isNotFound(err):
		return nil, fmt.Errorf("review: get team %q: %w", slug, err)
	}
	oa.teams[slug] = m
	return m, nil
}

// approvedBy returns the usernames of current approvers among owners.
func (oa *ownerApprovers) approvedBy(ctx context.Context, owners []string) ([]string, error) {
	out := []string{}
	for _, a := range oa.approvals {
		for _, o := range owners {
			kind, name, _ := strings.Cut(o, ":")
			match := false
			if kind == "team" {
				m, err := oa.members(ctx, name)
				if err != nil {
					return nil, err
				}
				match = m[a.AccountID]
			} else {
				match = strings.EqualFold(name, a.Username)
			}
			if match {
				out = append(out, a.Username)
				break
			}
		}
	}
	return out, nil
}

// ownership computes the owners of the paths mr changes and their approval
// state for sourceTip / targetTip.
func (s *Service) ownership(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest, sourceTip, targetTip string) (*MROwners, error) {
	out := &MROwners{Source: OwnersSourceNone, Paths: []PathOwnership{}, Missing: []string{}}
	cfg, err := s.ownersConfig(ctx, tree)
	if err != nil {
		return nil, err
	}
	out.Source = cfg.Source
	paths, err := s.changedPaths(ctx, sourceTip, targetTip)
	if err != nil {
		return nil, err
	}
	oa := s.newOwnerApprovers(tree, mr, sourceTip)
	for _, p := range paths {
		po := PathOwnership{Path: p, Owners: []string{}, ApprovedBy: []string{}}
		if owners := OwnersOf(cfg.Rules, p); len(owners) > 0 {
			po.Owners = slices.Clone(owners)
			if po.ApprovedBy, err = oa.approvedBy(ctx, owners); err != nil {
				return nil, err
			}
			po.Approved = len(po.ApprovedBy) > 0
			if !po.Approved {
				out.Missing = append(out.Missing, p)
			}
		}
		out.Paths = append(out.Paths, po)
	}
	out.Satisfied = len(out.Missing) == 0
	return out, nil
}

// MROwners reports the owners of the paths merge request number changes
// and whether each path's owners approved the current source tip. It
// requires domain.ActTreeRead.
func (s *Service) MROwners(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*MROwners, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	mr, err := s.getMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	sourceTip, targetTip := s.tips(ctx, tree, mr)
	out, err := s.ownership(ctx, tree, mr, sourceTip, targetTip)
	if err != nil {
		return nil, err
	}
	eff, err := s.ProtectionFor(ctx, tree.ID, mr.TargetBranch)
	if err != nil {
		return nil, err
	}
	out.Required = eff.RequireOwnerApproval
	return out, nil
}

// tips returns the current source and target tips of mr ("" when missing).
func (s *Service) tips(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest) (source, target string) {
	if b, err := s.store.GetBranch(ctx, tree.ID, mr.SourceBranch); err == nil {
		source = b.Tip
	}
	if b, err := s.store.GetBranch(ctx, tree.ID, mr.TargetBranch); err == nil {
		target = b.Tip
	}
	return source, target
}

// evaluateOwners fills the owner summary and blocks when the target
// requires owner approval and an owned changed path lacks one.
func (s *Service) evaluateOwners(ctx context.Context, tree *domain.Tree, ev *evaluation) error {
	ev.checks.Owners = OwnersSummary{Required: ev.eff.RequireOwnerApproval, Missing: []string{}}
	if !ev.eff.RequireOwnerApproval || ev.sourceTip == "" || ev.upToDate {
		return nil
	}
	if s.objects == nil {
		ev.block(BlockerOwners, "owner approval is required but cannot be checked on this server")
		return nil
	}
	own, err := s.ownership(ctx, tree, ev.mr, ev.sourceTip, ev.targetTip)
	if err != nil {
		return err
	}
	ev.checks.Owners.Missing = own.Missing
	if len(own.Missing) == 0 {
		return nil
	}
	parts := make([]string, 0, min(len(own.Missing), 10)+1)
	for i, po := range own.Paths {
		if po.Approved || len(po.Owners) == 0 {
			continue
		}
		if len(parts) == 10 {
			parts = append(parts, fmt.Sprintf("and %d more", len(own.Missing)-10))
			break
		}
		parts = append(parts, fmt.Sprintf("%s (%s)", own.Paths[i].Path, strings.Join(po.Owners, ", ")))
	}
	ev.block(BlockerOwners, "owner approval required for %s", strings.Join(parts, "; "))
	return nil
}

// recordTree writes an audit event about a tree-level review setting.
func (s *Service) recordTree(ctx context.Context, action string, p *domain.Principal, tree *domain.Tree, details map[string]string) {
	if s.audit == nil {
		return
	}
	d := map[string]string{"tree": tree.Name, "tree_id": tree.ID}
	for k, v := range details {
		d[k] = v
	}
	ev := domain.AuditEvent{
		TenantID: tree.TenantID, Actor: "anonymous", Action: action,
		ResourceType: "tree", ResourceID: tree.ID, Details: d, At: s.now(),
	}
	if !p.Anonymous() {
		ev.ActorID, ev.Actor = p.AccountID, p.Username
	}
	s.audit.Record(ctx, ev)
}
