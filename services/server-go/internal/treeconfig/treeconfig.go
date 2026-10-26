// Package treeconfig validates and applies the .wt-tree configuration of
// pushed snapshots: config-as-code (docs/ENGINEERING_PLAN.md §16.3,
// specs/dot-wt-tree/ConfigAsCode.md).
//
// When a push to a tree's default branch changes `.wt-tree/config.toml`,
// `.wt-tree/branches.toml`, `.wt-tree/protections.toml` or
// `.wt-tree/access/**`, CheckPushPlan (a domain.PushChecker) parses and
// validates the files of the new tip and rejects the push with located
// errors (file:line:col, codes shared with the Rust validator). Changes to
// `access/` additionally need iam:admin on the tenant and must pass a dry
// run of accesscfg.
//
// After the branch moved (the branch.pushed event, published by pushes and
// server-side merges alike), the changed files are applied:
//
//   - branches.toml: Branch.Description and Branch.Owner of the declared
//     branches (branches that no longer appear are cleared when they still
//     hold the previously declared values); such branches are
//     config-managed and refuse PATCH .../branches/{name};
//   - protections.toml: the declared protections replace the protections
//     the configuration created before; protections set through the REST
//     API with the same pattern are never overwritten (warning W3101);
//   - access/policies.toml: applied by accesscfg as the tree's access
//     document (removing the file removes the tree's config-managed
//     policies).
//
// The applied state and the outcome of the last attempt are stored in the
// DocStore collection `tree_config` (key: tree id); every apply publishes
// config.applied and is audited (`config.apply`, `config.rejected`).
package treeconfig

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"
	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/accesscfg"
	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Collection is the DocStore collection of applied configurations, keyed
// by tree id.
const Collection = "tree_config"

// DenyKind is the PushDenied kind of an invalid configuration (sent to
// clients as an access_denied rejection with the located errors).
const DenyKind = "invalid_config"

// pendingTTL bounds how long the pusher of an accepted push is remembered
// for the post-push apply.
const pendingTTL = 10 * time.Minute

// Deps are the collaborators of the treeconfig package. Store is required;
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

// Service is the treeconfig component.
type Service struct {
	d      Deps
	access *accesscfg.Service
	now    func() time.Time

	mu      sync.Mutex
	pending map[string]pendingPush // DocKey(treeID, newTip) → pusher
	applyMu sync.Mutex             // serializes applies
}

type pendingPush struct {
	p  *domain.Principal
	at time.Time
}

// New returns a Service.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, now: func() time.Time { return time.Now().UTC() }, pending: map[string]pendingPush{}}
}

// SetAccessConfig wires the declarative access service used for
// `.wt-tree/access/` (nil = access files are validated structurally only
// and not applied).
func (s *Service) SetAccessConfig(a *accesscfg.Service) { s.access = a }

// ---- stored state ----

// FileInfo describes one config file of the applied snapshot.
type FileInfo struct {
	Path string `json:"path"`
	Hash string `json:"hash"` // hex BLAKE3 of the content
	Size int    `json:"size"`
}

// Applied is the configuration currently in effect.
type Applied struct {
	SnapshotID string     `json:"snapshot_id"`
	Branch     string     `json:"branch"`
	AppliedBy  string     `json:"applied_by"`
	AppliedAt  time.Time  `json:"applied_at"`
	Files      []FileInfo `json:"files"`
	// Branches are the declared branch metadata (branches.toml).
	Branches []BranchDecl `json:"branches"`
	// Protections are the declared protections (protections.toml).
	Protections []domain.BranchProtection `json:"protections"`
	// ManagedPatterns are the protection patterns this configuration owns
	// (declared ones minus those managed through the API).
	ManagedPatterns []string `json:"managed_patterns"`
	// AccessHash is the hex BLAKE3 of the applied access document ("" = none).
	AccessHash string `json:"access_hash,omitempty"`
	// Changes summarizes what the last apply changed.
	Changes  []string     `json:"changes"`
	Warnings []Diagnostic `json:"warnings"`
}

// Attempt is the outcome of the last validation of a pushed configuration.
type Attempt struct {
	SnapshotID string       `json:"snapshot_id"`
	Branch     string       `json:"branch"`
	By         string       `json:"by"`
	At         time.Time    `json:"at"`
	Accepted   bool         `json:"accepted"`
	Errors     []Diagnostic `json:"errors"`
	Warnings   []Diagnostic `json:"warnings"`
}

// State is the tree_config document (GET {tree}/config).
type State struct {
	TreeID        string   `json:"tree_id"`
	DefaultBranch string   `json:"default_branch"`
	Applied       *Applied `json:"applied"`
	LastAttempt   *Attempt `json:"last_attempt"`
}

func (s *Service) load(ctx context.Context, tree *domain.Tree) (*State, error) {
	var st State
	err := s.d.Store.GetDoc(ctx, Collection, tree.ID, &st)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		st = State{TreeID: tree.ID}
	case err != nil:
		return nil, err
	}
	st.DefaultBranch = tree.DefaultBranch
	return &st, nil
}

// Get returns the configuration state of tree (tree:read). A tree without
// config-as-code has Applied and LastAttempt nil.
func (s *Service) Get(ctx context.Context, p *domain.Principal, tree *domain.Tree) (*State, error) {
	if err := s.authorize(ctx, p, domain.ActTreeRead, tree); err != nil {
		return nil, err
	}
	return s.load(ctx, tree)
}

func (s *Service) authorize(ctx context.Context, p *domain.Principal, act domain.Action, tree *domain.Tree) error {
	if s.d.Authz == nil {
		return nil
	}
	return s.d.Authz.Authorize(ctx, p, act, tree.TenantID, tree)
}

// ---- reading files from snapshots ----

// snapshotFiles returns the config files of a snapshot (path → content)
// and their FileInfo, sorted by path.
func (s *Service) snapshotFiles(ctx context.Context, snap *domain.Snapshot) (map[string][]byte, []FileInfo, error) {
	m, err := s.manifest(ctx, snap)
	if err != nil {
		return nil, nil, err
	}
	files := map[string][]byte{}
	var infos []FileInfo
	for _, e := range m.GetEntries() {
		if !IsConfigPath(e.GetPath()) {
			continue
		}
		data, err := content.ReadEntry(ctx, s.d.Objects, e)
		if err != nil {
			return nil, nil, fmt.Errorf("treeconfig: read %s: %w", e.GetPath(), err)
		}
		files[e.GetPath()] = data
		infos = append(infos, FileInfo{Path: e.GetPath(), Hash: hash.Sum(data).Hex(), Size: len(data)})
	}
	sort.Slice(infos, func(i, j int) bool { return infos[i].Path < infos[j].Path })
	return files, infos, nil
}

// configHashes maps the config paths of a snapshot to their blob hashes
// (empty for "").
func (s *Service) configHashes(ctx context.Context, snapshotID string) (map[string]string, error) {
	out := map[string]string{}
	if snapshotID == "" {
		return out, nil
	}
	snap, err := s.d.Store.GetSnapshot(ctx, snapshotID)
	if err != nil {
		return nil, err
	}
	m, err := s.manifest(ctx, snap)
	if err != nil {
		return nil, err
	}
	for _, e := range m.GetEntries() {
		if IsConfigPath(e.GetPath()) {
			out[e.GetPath()] = fmt.Sprintf("%x/%v", e.GetBlobHash().GetValue(), e.GetChunked())
		}
	}
	return out, nil
}

func (s *Service) manifest(ctx context.Context, snap *domain.Snapshot) (*syncpb.ManifestRecord, error) {
	data, err := s.d.Objects.Get(ctx, snap.ManifestHash)
	if err != nil {
		return nil, fmt.Errorf("treeconfig: read manifest of %s: %w", snap.ID, err)
	}
	m := &syncpb.ManifestRecord{}
	if err := proto.Unmarshal(data, m); err != nil {
		return nil, fmt.Errorf("treeconfig: decode manifest of %s: %w", snap.ID, err)
	}
	return m, nil
}

// ---- server-side checks ----

// emptyAccessDocument removes a tree's config-managed policies.
var emptyAccessDocument = []byte(`{"roles":[],"policies":[]}`)

// serverChecks adds the checks that need server state to parsed: owners
// must exist; when accessChanged, the caller needs iam:admin and the
// access document must pass accesscfg (a dry run unless apply). It returns
// the access plan (changes).
func (s *Service) serverChecks(ctx context.Context, p *domain.Principal, tree *domain.Tree, parsed *Parsed, accessChanged, apply bool) ([]string, error) {
	var extra []Diagnostic
	for _, b := range parsed.Branches {
		if b.Owner == "" || CheckOwner(b.Owner) != "" {
			continue
		}
		ok, err := s.ownerExists(ctx, tree, b.Owner)
		if err != nil {
			return nil, err
		}
		if !ok {
			extra = append(extra, newDiag(EOwner, BranchesFile, parsed.ownerPos[b.Name], "owner %s does not exist in this tenant", b.Owner))
		}
	}
	var changes []string
	if accessChanged && s.access != nil && !parsed.HasErrors() {
		d, ch, err := s.checkAccess(ctx, p, tree, parsed, apply)
		if err != nil {
			return nil, err
		}
		extra = append(extra, d...)
		changes = ch
	}
	parsed.Diagnostics = append(parsed.Diagnostics, extra...)
	sortDiagnostics(parsed.Diagnostics)
	return changes, nil
}

func (s *Service) checkAccess(ctx context.Context, p *domain.Principal, tree *domain.Tree, parsed *Parsed, apply bool) ([]Diagnostic, []string, error) {
	file := PoliciesFile
	tenant, err := s.d.Store.GetTenant(ctx, tree.TenantID)
	if err != nil {
		return nil, nil, err
	}
	denied := func() []Diagnostic {
		return []Diagnostic{newDiag(EPermission, file, Pos{},
			"changing %s needs iam:admin in tenant %s; ask a tenant admin to push this change (or revert the files under %s)",
			AccessDir, tenant.Slug, AccessDir)}
	}
	if p == nil {
		return denied(), nil, nil
	}
	if err := s.d.Authz.Authorize(ctx, p, domain.ActIAMAdmin, tree.TenantID, nil); err != nil {
		if errors.Is(err, domain.ErrForbidden) || errors.Is(err, domain.ErrUnauthenticated) {
			return denied(), nil, nil
		}
		return nil, nil, err
	}
	doc := parsed.Access
	if !parsed.hasAccess {
		doc = emptyAccessDocument
	}
	res, err := s.access.Apply(ctx, p, tenant, tree, doc, !apply)
	if err != nil {
		if errors.Is(err, domain.ErrForbidden) || errors.Is(err, domain.ErrUnauthenticated) {
			return denied(), nil, nil
		}
		return nil, nil, err
	}
	if !res.Accepted {
		var out []Diagnostic
		for _, e := range res.Errors {
			out = append(out, newDiag(EAccess, file, parsed.locateAccessError(e), "%s", e))
		}
		return out, nil, nil
	}
	return nil, res.Changes, nil
}

func (s *Service) ownerExists(ctx context.Context, tree *domain.Tree, owner string) (bool, error) {
	kind, name, _ := strings.Cut(owner, ":")
	var err error
	switch kind {
	case "user":
		_, err = s.d.Store.GetAccountByUsername(ctx, name)
	case "team":
		_, err = s.d.Store.GetTeamBySlug(ctx, tree.TenantID, name)
	default:
		return false, nil
	}
	if errors.Is(err, domain.ErrNotFound) {
		return false, nil
	}
	return err == nil, err
}

// ---- push check ----

// CheckPushPlan implements domain.PushChecker: pushes to the default
// branch that change config-as-code files must carry a valid
// configuration.
func (s *Service) CheckPushPlan(ctx context.Context, p *domain.Principal, tree *domain.Tree, plan *domain.PushPlan) error {
	if tree == nil || plan == nil || plan.Branch != tree.DefaultBranch {
		return nil
	}
	accessChanged, relevant := false, false
	for _, path := range plan.ChangedPaths {
		if IsConfigPath(path) {
			relevant = true
			accessChanged = accessChanged || isAccessPath(path)
		}
	}
	if !relevant {
		return nil
	}
	snap := s.tipSnapshot(ctx, plan)
	if snap == nil {
		return nil // stored elsewhere and unreadable: validated on apply
	}
	files, _, err := s.snapshotFiles(ctx, snap)
	if err != nil {
		return err
	}
	parsed := Validate(files)
	if _, err := s.serverChecks(ctx, p, tree, parsed, accessChanged, false); err != nil {
		return err
	}
	errs := Errors(parsed.Diagnostics)
	if len(errs) > 0 {
		s.recordAttempt(ctx, tree, &Attempt{SnapshotID: plan.NewTip, Branch: plan.Branch, By: username(p), At: s.now(),
			Accepted: false, Errors: errs, Warnings: nonNil(Warnings(parsed.Diagnostics))})
		s.audit(ctx, p, tree, "config.rejected", map[string]string{"snapshot": plan.NewTip, "branch": plan.Branch,
			"errors": fmt.Sprint(len(errs)), "first": errs[0].String()})
		return &domain.PushDenied{Kind: DenyKind, Reason: "invalid .wt-tree configuration: " + summarize(errs)}
	}
	s.remember(tree.ID, plan.NewTip, p)
	return nil
}

func (s *Service) tipSnapshot(ctx context.Context, plan *domain.PushPlan) *domain.Snapshot {
	for _, snap := range plan.NewSnapshots {
		if snap.ID == plan.NewTip {
			return snap
		}
	}
	snap, err := s.d.Store.GetSnapshot(ctx, plan.NewTip)
	if err != nil {
		return nil
	}
	return snap
}

func (s *Service) remember(treeID, tip string, p *domain.Principal) {
	s.mu.Lock()
	defer s.mu.Unlock()
	now := s.now()
	for k, v := range s.pending {
		if now.Sub(v.at) > pendingTTL {
			delete(s.pending, k)
		}
	}
	if p != nil && len(s.pending) < 10_000 {
		s.pending[domain.DocKey(treeID, tip)] = pendingPush{p: p, at: now}
	}
}

func (s *Service) takePending(treeID, tip string) *domain.Principal {
	s.mu.Lock()
	defer s.mu.Unlock()
	k := domain.DocKey(treeID, tip)
	v, ok := s.pending[k]
	if !ok {
		return nil
	}
	delete(s.pending, k)
	return v.p
}

// ---- events ----

// Start subscribes to branch pushes, branch creation and tree deletion.
// Without a Bus it does nothing.
func (s *Service) Start(ctx context.Context) {
	if s.d.Bus == nil {
		return
	}
	ch, cancel := s.d.Bus.Subscribe(domain.EventFilter{
		Types: []string{domain.EventBranchPushed, domain.EventBranchCreated, domain.EventTreeDeleted},
	})
	go func() {
		defer cancel()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-ch:
				if !ok {
					return
				}
				if err := s.HandleEvent(context.WithoutCancel(ctx), e); err != nil {
					s.d.Logger.WarnContext(ctx, "treeconfig: handling event failed", "type", e.Type, "tree", e.TreeID, "err", err)
				}
			}
		}
	}()
}

type pushPayload struct {
	Branch string `json:"branch"`
	Name   string `json:"name"` // branch.created (domain.Branch)
	OldTip string `json:"old_tip"`
	NewTip string `json:"new_tip"`
}

// HandleEvent processes one bus event synchronously (Start calls it; tests
// call it directly).
func (s *Service) HandleEvent(ctx context.Context, e domain.Event) error {
	if e.TreeID == "" {
		return nil
	}
	if e.Type == domain.EventTreeDeleted {
		if err := s.d.Store.DeleteDoc(ctx, Collection, e.TreeID); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
		return nil
	}
	var pl pushPayload
	raw, err := json.Marshal(e.Payload)
	if err != nil {
		return err
	}
	if err := json.Unmarshal(raw, &pl); err != nil {
		return nil // not a payload we understand
	}
	tree, err := s.d.Store.GetTree(ctx, e.TreeID)
	if errors.Is(err, domain.ErrNotFound) {
		return nil
	}
	if err != nil {
		return err
	}
	switch e.Type {
	case domain.EventBranchCreated:
		return s.applyNewBranch(ctx, tree, pl.Name)
	case domain.EventBranchPushed:
		if pl.Branch == tree.DefaultBranch && pl.NewTip != "" {
			return s.Apply(ctx, tree, pl.Branch, pl.OldTip, pl.NewTip, e.Actor)
		}
		if pl.OldTip == "" && pl.Branch != "" {
			return s.applyNewBranch(ctx, tree, pl.Branch)
		}
	}
	return nil
}

// principalFor returns the pusher remembered by CheckPushPlan, else the
// account named actor (server-side merges).
func (s *Service) principalFor(ctx context.Context, tree *domain.Tree, tip, actor string) *domain.Principal {
	if p := s.takePending(tree.ID, tip); p != nil {
		return p
	}
	if actor == "" || strings.HasPrefix(actor, "sa:") {
		return nil
	}
	acct, err := s.d.Store.GetAccountByUsername(ctx, actor)
	if err != nil || acct.Disabled {
		return nil
	}
	return &domain.Principal{AccountID: acct.ID, Username: acct.Username, TenantID: acct.TenantID, IsAdmin: acct.IsAdmin}
}

// Apply validates and applies the configuration of newTip on the default
// branch of tree when it changed since oldTip. actor names the account
// that moved the branch (used when the push was not seen by
// CheckPushPlan, e.g. a merge request merge).
func (s *Service) Apply(ctx context.Context, tree *domain.Tree, branch, oldTip, newTip, actor string) error {
	s.applyMu.Lock()
	defer s.applyMu.Unlock()
	before, err := s.configHashes(ctx, oldTip)
	if err != nil && !errors.Is(err, domain.ErrNotFound) && !errors.Is(err, objstore.ErrNotFound) {
		return err
	}
	after, err := s.configHashes(ctx, newTip)
	if err != nil {
		return err
	}
	changed := map[string]bool{}
	for p, h := range after {
		if before[p] != h {
			changed[p] = true
		}
	}
	for p := range before {
		if _, ok := after[p]; !ok {
			changed[p] = true
		}
	}
	p := s.principalFor(ctx, tree, newTip, actor)
	if len(changed) == 0 {
		return nil
	}
	snap, err := s.d.Store.GetSnapshot(ctx, newTip)
	if err != nil {
		return err
	}
	files, infos, err := s.snapshotFiles(ctx, snap)
	if err != nil {
		return err
	}
	accessChanged := false
	for path := range changed {
		accessChanged = accessChanged || isAccessPath(path)
	}
	parsed := Validate(files)
	by := username(p)
	if by == "" {
		by = actor
	}
	if _, err := s.serverChecks(ctx, p, tree, parsed, accessChanged, false); err != nil {
		return err
	}
	if errs := Errors(parsed.Diagnostics); len(errs) > 0 {
		s.recordAttempt(ctx, tree, &Attempt{SnapshotID: newTip, Branch: branch, By: by, At: s.now(), Errors: errs, Warnings: nonNil(Warnings(parsed.Diagnostics))})
		s.audit(ctx, p, tree, "config.rejected", map[string]string{"snapshot": newTip, "branch": branch, "errors": fmt.Sprint(len(errs)), "first": errs[0].String()})
		return nil
	}

	st, err := s.load(ctx, tree)
	if err != nil {
		return err
	}
	prev := st.Applied
	if prev == nil {
		prev = &Applied{}
	}
	next := &Applied{
		SnapshotID: newTip, Branch: branch, AppliedBy: by, AppliedAt: s.now(), Files: nonNil(infos),
		Branches: prev.Branches, Protections: prev.Protections, ManagedPatterns: prev.ManagedPatterns, AccessHash: prev.AccessHash,
	}
	var changes []string
	var warnings []Diagnostic
	if changed[BranchesFile] {
		decls := parsed.Branches
		if !parsed.hasBranches {
			decls = nil
		}
		ch, w, err := s.applyBranches(ctx, tree, prev.Branches, decls, parsed.branchPos)
		if err != nil {
			return err
		}
		changes, warnings = append(changes, ch...), append(warnings, w...)
		next.Branches = decls
	}
	if changed[ProtectionsFile] {
		decls := parsed.Protections
		if !parsed.hasProtections {
			decls = nil
		}
		managed, ch, w, err := s.applyProtections(ctx, tree, prev.ManagedPatterns, decls, parsed.protectionPos)
		if err != nil {
			return err
		}
		changes, warnings = append(changes, ch...), append(warnings, w...)
		next.Protections, next.ManagedPatterns = decls, managed
	}
	if accessChanged && s.access != nil {
		ch, err := s.serverChecks(ctx, p, tree, parsed, true, true)
		if err != nil {
			return err
		}
		if errs := Errors(parsed.Diagnostics); len(errs) > 0 {
			// Refused between the dry run and the apply (state changed).
			warnings = append(warnings, errs...)
		} else {
			for _, c := range ch {
				changes = append(changes, "access "+c)
			}
			next.AccessHash = ""
			if parsed.hasAccess {
				next.AccessHash = hash.Sum(parsed.Access).Hex()
			}
		}
	}
	warnings = append(warnings, Warnings(parsed.Diagnostics)...)
	sortDiagnostics(warnings)
	warnings = slices.CompactFunc(warnings, func(a, b Diagnostic) bool { return a == b })
	next.Changes, next.Warnings = nonNil(changes), nonNil(warnings)
	next.Branches, next.Protections, next.ManagedPatterns = nonNil(next.Branches), nonNil(next.Protections), nonNil(next.ManagedPatterns)
	st.Applied = next
	st.LastAttempt = &Attempt{SnapshotID: newTip, Branch: branch, By: by, At: next.AppliedAt, Accepted: true, Errors: []Diagnostic{}, Warnings: next.Warnings}
	if err := s.d.Store.PutDoc(ctx, Collection, tree.ID, st); err != nil {
		return err
	}
	paths := make([]string, 0, len(changed))
	for path := range changed {
		paths = append(paths, path)
	}
	sort.Strings(paths)
	if s.d.Bus != nil {
		s.d.Bus.Publish(domain.Event{
			Type: domain.EventConfigApplied, TenantID: tree.TenantID, TreeID: tree.ID, Actor: by, At: s.now(),
			Payload: map[string]any{"snapshot_id": newTip, "branch": branch, "files": paths, "changes": next.Changes, "warnings": len(next.Warnings)},
		})
	}
	s.audit(ctx, p, tree, "config.apply", map[string]string{
		"snapshot": newTip, "branch": branch, "files": strings.Join(paths, ","), "changes": fmt.Sprint(len(next.Changes)), "actor": by,
	})
	return nil
}

// applyBranches sets the metadata of the declared branches and clears it
// on branches that are no longer declared (when unchanged since).
func (s *Service) applyBranches(ctx context.Context, tree *domain.Tree, prev, decls []BranchDecl, namePos map[string]Pos) ([]string, []Diagnostic, error) {
	var changes []string
	var warnings []Diagnostic
	declared := map[string]bool{}
	for _, d := range decls {
		declared[d.Name] = true
		b, err := s.d.Store.GetBranch(ctx, tree.ID, d.Name)
		if errors.Is(err, domain.ErrNotFound) {
			warnings = append(warnings, newDiag(WNoSuchBranch, BranchesFile, namePos[d.Name], "branch %s does not exist yet; its metadata is applied when it is created", d.Name))
			continue
		}
		if err != nil {
			return nil, nil, err
		}
		if b.Description == d.Description && b.Owner == d.Owner {
			continue
		}
		if _, err := s.setBranchMeta(ctx, tree.ID, d.Name, d.Description, d.Owner); err != nil {
			return nil, nil, err
		}
		changes = append(changes, "branch "+d.Name+": "+describeMeta(d.Description, d.Owner))
	}
	for _, old := range prev {
		if declared[old.Name] {
			continue
		}
		b, err := s.d.Store.GetBranch(ctx, tree.ID, old.Name)
		if errors.Is(err, domain.ErrNotFound) {
			continue
		}
		if err != nil {
			return nil, nil, err
		}
		if b.Description != old.Description || b.Owner != old.Owner || (b.Description == "" && b.Owner == "") {
			continue
		}
		if _, err := s.setBranchMeta(ctx, tree.ID, old.Name, "", ""); err != nil {
			return nil, nil, err
		}
		changes = append(changes, "branch "+old.Name+": metadata cleared")
	}
	return changes, warnings, nil
}

func describeMeta(desc, owner string) string {
	var parts []string
	if desc != "" {
		parts = append(parts, "description set")
	} else {
		parts = append(parts, "no description")
	}
	if owner != "" {
		parts = append(parts, "owner "+owner)
	} else {
		parts = append(parts, "no owner")
	}
	return strings.Join(parts, ", ")
}

// setBranchMeta updates description and owner without touching the tip.
func (s *Service) setBranchMeta(ctx context.Context, treeID, name, desc, owner string) (*domain.Branch, error) {
	if u, ok := s.d.Store.(meta.BranchMetaUpdater); ok {
		return u.UpdateBranchMeta(ctx, treeID, name, desc, owner)
	}
	b, err := s.d.Store.GetBranch(ctx, treeID, name)
	if err != nil {
		return nil, err
	}
	b.Description, b.Owner = desc, owner
	if err := s.d.Store.PutBranch(ctx, b); err != nil {
		return nil, err
	}
	return b, nil
}

// applyProtections makes the config-managed protections equal to decls.
// It returns the new managed patterns.
func (s *Service) applyProtections(ctx context.Context, tree *domain.Tree, prevManaged []string, decls []domain.BranchProtection, patternPos map[string]Pos) ([]string, []string, []Diagnostic, error) {
	current, err := s.d.Store.ListProtections(ctx, tree.ID)
	if err != nil {
		return nil, nil, nil, err
	}
	byPattern := map[string]domain.BranchProtection{}
	for _, p := range current {
		byPattern[p.Pattern] = p
	}
	var managed, changes []string
	var warnings []Diagnostic
	declared := map[string]bool{}
	for _, d := range decls {
		declared[d.Pattern] = true
		cur, exists := byPattern[d.Pattern]
		if exists && !slices.Contains(prevManaged, d.Pattern) {
			warnings = append(warnings, newDiag(WManagedByAPI, ProtectionsFile, patternPos[d.Pattern],
				"a protection for %s is managed through the API; this entry is ignored (delete the API rule to let the configuration manage it)", d.Pattern))
			continue
		}
		managed = append(managed, d.Pattern)
		d.TreeID = tree.ID
		if exists && sameProtection(cur, d) {
			continue
		}
		if err := s.d.Store.PutProtection(ctx, &d); err != nil {
			return nil, nil, nil, err
		}
		op := "create"
		if exists {
			op = "update"
		}
		changes = append(changes, "protection "+d.Pattern+": "+op)
	}
	for _, old := range prevManaged {
		if declared[old] {
			continue
		}
		if _, exists := byPattern[old]; !exists {
			continue
		}
		if err := s.d.Store.DeleteProtection(ctx, tree.ID, old); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return nil, nil, nil, err
		}
		changes = append(changes, "protection "+old+": delete")
	}
	return managed, changes, warnings, nil
}

func sameProtection(a, b domain.BranchProtection) bool {
	norm := func(p domain.BranchProtection) string {
		p.TreeID = ""
		if len(p.AllowedPushers) == 0 {
			p.AllowedPushers = nil
		}
		if len(p.RequiredChecks) == 0 {
			p.RequiredChecks = nil
		}
		if len(p.MergeMethods) == 0 {
			p.MergeMethods = nil
		}
		raw, _ := json.Marshal(p)
		return string(raw)
	}
	return norm(a) == norm(b)
}

// applyNewBranch applies the declared metadata to a branch that was just
// created.
func (s *Service) applyNewBranch(ctx context.Context, tree *domain.Tree, name string) error {
	if name == "" {
		return nil
	}
	st, err := s.load(ctx, tree)
	if err != nil || st.Applied == nil {
		return err
	}
	for _, d := range st.Applied.Branches {
		if d.Name != name {
			continue
		}
		b, err := s.d.Store.GetBranch(ctx, tree.ID, name)
		if errors.Is(err, domain.ErrNotFound) {
			return nil
		}
		if err != nil {
			return err
		}
		if b.Description == d.Description && b.Owner == d.Owner {
			return nil
		}
		_, err = s.setBranchMeta(ctx, tree.ID, name, d.Description, d.Owner)
		return err
	}
	return nil
}

func (s *Service) recordAttempt(ctx context.Context, tree *domain.Tree, a *Attempt) {
	a.Errors, a.Warnings = nonNil(a.Errors), nonNil(a.Warnings)
	err := s.d.Store.UpdateDoc(ctx, Collection, tree.ID, func(raw []byte) ([]byte, error) {
		st := State{TreeID: tree.ID}
		if raw != nil {
			if err := json.Unmarshal(raw, &st); err != nil {
				return nil, err
			}
		}
		st.DefaultBranch = tree.DefaultBranch
		st.LastAttempt = a
		return json.Marshal(st)
	})
	if err != nil {
		s.d.Logger.WarnContext(ctx, "treeconfig: recording the attempt failed", "tree", tree.ID, "err", err)
	}
}

// ---- REST helpers ----

// ValidationResult is the answer of POST {tree}/config/validate.
type ValidationResult struct {
	Valid       bool                      `json:"valid"`
	Diagnostics []Diagnostic              `json:"diagnostics"`
	Branches    []BranchDecl              `json:"branches"`
	Protections []domain.BranchProtection `json:"protections"`
	// AccessChanges is the plan of the access document (dry run).
	AccessChanges []string `json:"access_changes"`
}

// MaxValidateFiles bounds the files of one validation request.
const MaxValidateFiles = 64

// ValidateFiles validates files (tree-relative paths → content) as if
// they were pushed to the default branch, without changing anything
// (tree:write). Access files are checked with a dry run (they need
// iam:admin, reported as E3010).
func (s *Service) ValidateFiles(ctx context.Context, p *domain.Principal, tree *domain.Tree, files map[string]string) (*ValidationResult, error) {
	if err := s.authorize(ctx, p, domain.ActTreeWrite, tree); err != nil {
		return nil, err
	}
	if len(files) > MaxValidateFiles {
		return nil, fmt.Errorf("at most %d files can be validated at once: %w", MaxValidateFiles, domain.ErrInvalid)
	}
	raw := map[string][]byte{}
	accessGiven := false
	for path, text := range files {
		if !IsConfigPath(path) {
			return nil, fmt.Errorf("%q is not a config-as-code file (%s, %s, %s or %s**): %w",
				path, ConfigFile, BranchesFile, ProtectionsFile, AccessDir, domain.ErrInvalid)
		}
		raw[path] = []byte(text)
		accessGiven = accessGiven || isAccessPath(path)
	}
	parsed := Validate(raw)
	changes, err := s.serverChecks(ctx, p, tree, parsed, accessGiven, false)
	if err != nil {
		return nil, err
	}
	return &ValidationResult{
		Valid: !parsed.HasErrors(), Diagnostics: parsed.Diagnostics,
		Branches: nonNil(parsed.Branches), Protections: nonNil(parsed.Protections), AccessChanges: nonNil(changes),
	}, nil
}

// ManagedBranch reports whether branch is declared by the applied
// configuration of tree.
func (s *Service) ManagedBranch(ctx context.Context, tree *domain.Tree, branch string) (bool, error) {
	st, err := s.load(ctx, tree)
	if err != nil || st.Applied == nil {
		return false, err
	}
	return slices.ContainsFunc(st.Applied.Branches, func(b BranchDecl) bool { return b.Name == branch }), nil
}

// BranchUpdate is the body of PATCH {tree}/branches/{name}: absent fields
// stay unchanged, "" clears.
type BranchUpdate struct {
	Description *string `json:"description,omitempty"`
	Owner       *string `json:"owner,omitempty"`
}

// ErrConfigManaged is returned by UpdateBranch for branches declared in
// .wt-tree/branches.toml.
var ErrConfigManaged = errors.New("treeconfig: branch is managed by .wt-tree/branches.toml")

// UpdateBranch changes the description and/or owner of a branch.
// Descriptions need tree:write, owners tree:admin; branches declared in
// the applied branches.toml are refused with ErrConfigManaged.
func (s *Service) UpdateBranch(ctx context.Context, p *domain.Principal, tree *domain.Tree, name string, u BranchUpdate) (*domain.Branch, error) {
	if u.Description == nil && u.Owner == nil {
		return nil, fmt.Errorf("nothing to change: pass description and/or owner: %w", domain.ErrInvalid)
	}
	act := domain.ActTreeWrite
	if u.Owner != nil {
		act = domain.ActTreeAdmin
	}
	if err := s.authorize(ctx, p, act, tree); err != nil {
		return nil, err
	}
	b, err := s.d.Store.GetBranch(ctx, tree.ID, name)
	if err != nil {
		return nil, err
	}
	desc, owner := b.Description, b.Owner
	if u.Description != nil {
		desc = strings.TrimSpace(*u.Description)
		if charLen(desc) > MaxDescription {
			return nil, fmt.Errorf("description must be at most %d characters: %w", MaxDescription, domain.ErrInvalid)
		}
	}
	if u.Owner != nil {
		owner = strings.TrimSpace(*u.Owner)
		if owner != "" {
			if msg := CheckOwner(owner); msg != "" {
				return nil, fmt.Errorf("%s: %w", msg, domain.ErrInvalid)
			}
			ok, err := s.ownerExists(ctx, tree, owner)
			if err != nil {
				return nil, err
			}
			if !ok {
				return nil, fmt.Errorf("owner %s does not exist in this tenant: %w", owner, domain.ErrInvalid)
			}
		}
	}
	managed, err := s.ManagedBranch(ctx, tree, name)
	if err != nil {
		return nil, err
	}
	if managed {
		return nil, ErrConfigManaged
	}
	out, err := s.setBranchMeta(ctx, tree.ID, name, desc, owner)
	if err != nil {
		return nil, err
	}
	s.audit(ctx, p, tree, "branch.update", map[string]string{"branch": name, "description": fmt.Sprint(u.Description != nil), "owner": owner})
	return out, nil
}

// ---- helpers ----

func (s *Service) audit(ctx context.Context, p *domain.Principal, tree *domain.Tree, action string, details map[string]string) {
	if s.d.Audit == nil {
		return
	}
	actor, actorID := "system", ""
	if p != nil {
		actor, actorID = p.Username, p.AccountID
	}
	s.d.Audit.Record(context.WithoutCancel(ctx), domain.AuditEvent{
		ID: uuid.NewString(), TenantID: tree.TenantID, ActorID: actorID, Actor: actor, Action: action,
		ResourceType: "tree", ResourceID: tree.ID, Details: details, At: s.now(),
	})
}

func username(p *domain.Principal) string {
	if p == nil {
		return ""
	}
	return p.Username
}

func nonNil[T any](v []T) []T {
	if v == nil {
		return []T{}
	}
	return v
}
