// Package todo manages TODOs created from snapshot dependencies
// (docs/ENGINEERING_PLAN.md §13.5).
//
// A snapshot may declare dependencies on other trees ("this change needs
// work in acme/api"). When such a snapshot is pushed, the Service — a
// subscriber of the event bus — creates one TODO per dependency in the
// target tree. A snapshot whose message contains a line
//
//	Completes: #N            (TODO N of the tree the snapshot is pushed to)
//	Completes: tree#N        (a tree of the same tenant)
//	Completes: tenant/tree#N (any tree)
//
// marks those TODOs done. The Service also answers the TODO REST API
// (manual TODOs, claim/complete/cancel, the dependency report).
//
// Storage (§13.14): collection "todos", key DocKey(treeID, %010d number),
// value domain.Todo; collection "todo_sources", key DocKey(sourceSnapshotID,
// targetTreeID, title), value sourceRef — the deduplication index that
// makes re-processing a snapshot harmless. Numbers come from
// NextSeq("todo/" + treeID).
package todo

import (
	"context"
	"crypto/rand"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Document collections owned by this package.
const (
	CollTodos   = "todos"
	CollSources = "todo_sources"
)

// Field limits.
const (
	MaxTitle   = 200   // bytes
	MaxDetails = 10000 // bytes
)

// Deps are the collaborators of the todo package. Store is required;
// the others may be nil unless documented otherwise. History and Bus are
// needed for TODOs created from pushes (Start); Authz for the access
// checks of those TODOs and of the dependency report.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Service is the todo component.
type Service struct {
	d   Deps
	now func() time.Time
}

// New returns a Service.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, now: func() time.Time { return time.Now().UTC() }}
}

// Error is a domain error with a client-facing message. errors.Is matches
// its Kind (domain.ErrNotFound, ErrInvalid, ErrConflict, ...).
type Error struct {
	Kind error
	Msg  string
}

func (e *Error) Error() string { return e.Msg }
func (e *Error) Unwrap() error { return e.Kind }

func errorf(kind error, format string, args ...any) error {
	return &Error{Kind: kind, Msg: fmt.Sprintf(format, args...)}
}

// sourceRef is the value of a todo_sources index document.
type sourceRef struct {
	TreeID string `json:"tree_id"`
	Number int    `json:"number"`
	// Key components, kept so the index can be cleaned up by tree.
	SourceSnapshotID string `json:"source_snapshot_id,omitempty"`
	Title            string `json:"title,omitempty"`
}

// TodoKey is the "todos" document key of TODO number n of treeID.
func TodoKey(treeID string, n int) string {
	return domain.DocKey(treeID, fmt.Sprintf("%010d", n))
}

// SourceKey is the "todo_sources" document key of a dependency.
func SourceKey(sourceSnapshotID, targetTreeID, title string) string {
	return domain.DocKey(sourceSnapshotID, targetTreeID, title)
}

func seqName(treeID string) string { return "todo/" + treeID }

// ---- event subscriber ---------------------------------------------------

// Start subscribes to branch.pushed (creating and completing TODOs) and
// tree.deleted (removing a deleted tree's TODOs) and processes those
// events in a background goroutine until ctx is cancelled. It returns
// once subscribed, so no event published afterwards is missed (unless the
// bus drops it because the subscriber fell behind). Without a Bus it does
// nothing.
func (s *Service) Start(ctx context.Context) {
	if s.d.Bus == nil {
		return
	}
	ch, cancel := s.d.Bus.Subscribe(domain.EventFilter{
		Types: []string{domain.EventBranchPushed, domain.EventTreeDeleted},
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
					s.d.Logger.WarnContext(ctx, "todo: handling event failed",
						"type", e.Type, "tree", e.TreeID, "err", err)
				}
			}
		}
	}()
}

// HandleEvent processes one bus event synchronously (Start calls it for
// every subscribed event; tests call it directly). Other event types are
// ignored.
func (s *Service) HandleEvent(ctx context.Context, e domain.Event) error {
	switch e.Type {
	case domain.EventBranchPushed:
		return s.handlePush(ctx, e)
	case domain.EventTreeDeleted:
		if e.TreeID == "" {
			return nil
		}
		return s.DeleteTree(ctx, e.TreeID)
	}
	return nil
}

// pushPayload is the payload of branch.pushed (syncsvc and review publish
// map[string]any with these keys).
type pushPayload struct {
	Branch string `json:"branch"`
	OldTip string `json:"old_tip"`
	NewTip string `json:"new_tip"`
}

func decodePayload(v any, dst any) error {
	raw, err := json.Marshal(v)
	if err != nil {
		return err
	}
	return json.Unmarshal(raw, dst)
}

// pushCtx carries what processing one push needs.
type pushCtx struct {
	tree   *domain.Tree
	tenant *domain.Tenant
	branch string
	actor  string
	// principals caches pushers by account id ("" = the event actor).
	principals map[string]*domain.Principal
	trees      map[[2]string]*domain.Tree // (tenant id, name) -> tree; nil entry = missing
	tenants    map[string]*domain.Tenant  // slug -> tenant; nil entry = missing
}

func (s *Service) handlePush(ctx context.Context, e domain.Event) error {
	if s.d.History == nil || e.TreeID == "" {
		return nil
	}
	var pl pushPayload
	if err := decodePayload(e.Payload, &pl); err != nil {
		return fmt.Errorf("todo: decode branch.pushed payload: %w", err)
	}
	if pl.NewTip == "" {
		return nil
	}
	tree, err := s.d.Store.GetTree(ctx, e.TreeID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil // deleted meanwhile
		}
		return err
	}
	tenant, err := s.d.Store.GetTenant(ctx, tree.TenantID)
	if err != nil {
		return err
	}
	snaps, err := s.d.History.Missing(ctx, pl.NewTip, []string{pl.OldTip})
	if err != nil {
		return fmt.Errorf("todo: history of %s: %w", pl.NewTip, err)
	}
	pc := &pushCtx{
		tree: tree, tenant: tenant, branch: pl.Branch, actor: e.Actor,
		principals: map[string]*domain.Principal{},
		trees:      map[[2]string]*domain.Tree{},
		tenants:    map[string]*domain.Tenant{tenant.Slug: tenant},
	}
	var errs []error
	for i := range snaps {
		snap := &snaps[i]
		refs := ParseCompletes(snap.Message)
		if len(snap.Dependencies) == 0 && len(refs) == 0 {
			continue
		}
		p := s.pusher(ctx, pc, snap.PushedBy)
		for _, dep := range snap.Dependencies {
			if err := s.createFromDependency(ctx, pc, p, snap, dep); err != nil {
				errs = append(errs, err)
			}
		}
		for _, ref := range refs {
			if err := s.completeRef(ctx, pc, p, snap, ref); err != nil {
				errs = append(errs, err)
			}
		}
	}
	return errors.Join(errs...)
}

// pusher resolves the principal that pushed a snapshot: its PushedBy
// account, else the event actor (a username or "sa:<tenant>/<name>").
// nil = unknown (treated as anonymous).
func (s *Service) pusher(ctx context.Context, pc *pushCtx, accountID string) *domain.Principal {
	if p, ok := pc.principals[accountID]; ok {
		return p
	}
	var p *domain.Principal
	if accountID != "" {
		if a, err := s.d.Store.GetAccount(ctx, accountID); err == nil {
			p = accountPrincipal(a)
		}
	}
	if p == nil {
		p = s.actorPrincipal(ctx, pc.actor)
	}
	pc.principals[accountID] = p
	return p
}

func accountPrincipal(a *domain.Account) *domain.Principal {
	if a.Disabled {
		return nil
	}
	return &domain.Principal{AccountID: a.ID, Username: a.Username, TenantID: a.TenantID, IsAdmin: a.IsAdmin}
}

func (s *Service) actorPrincipal(ctx context.Context, actor string) *domain.Principal {
	if actor == "" {
		return nil
	}
	if rest, ok := strings.CutPrefix(actor, "sa:"); ok {
		slug, name, ok := strings.Cut(rest, "/")
		if !ok {
			return nil
		}
		ten, err := s.d.Store.GetTenantBySlug(ctx, slug)
		if err != nil {
			return nil
		}
		sa, err := s.d.Store.GetServiceAccountByName(ctx, ten.ID, name)
		if err != nil || sa.Disabled {
			return nil
		}
		return &domain.Principal{Username: actor, TenantID: sa.TenantID,
			Kind: domain.PrincipalServiceAccount, ServiceAccountID: sa.ID}
	}
	a, err := s.d.Store.GetAccountByUsername(ctx, actor)
	if err != nil {
		return nil
	}
	return accountPrincipal(a)
}

func username(p *domain.Principal, fallback string) string {
	if p != nil {
		return p.Username
	}
	return fallback
}

func accountID(p *domain.Principal) string {
	if p != nil {
		return p.AccountID
	}
	return ""
}

// lookupTree resolves (tenant slug, tree name); slug "" = the source
// tenant. nil without error = no such tenant or tree.
func (s *Service) lookupTree(ctx context.Context, pc *pushCtx, slug, name string) (*domain.Tree, error) {
	ten := pc.tenant
	if slug != "" && slug != pc.tenant.Slug {
		var ok bool
		if ten, ok = pc.tenants[slug]; !ok {
			t, err := s.d.Store.GetTenantBySlug(ctx, slug)
			if err != nil && !errors.Is(err, domain.ErrNotFound) {
				return nil, err
			}
			ten = t
			pc.tenants[slug] = t
		}
		if ten == nil {
			return nil, nil
		}
	}
	k := [2]string{ten.ID, name}
	if t, ok := pc.trees[k]; ok {
		return t, nil
	}
	t, err := s.d.Store.GetTreeByName(ctx, ten.ID, name)
	if err != nil {
		if !errors.Is(err, domain.ErrNotFound) {
			return nil, err
		}
		t = nil
	}
	pc.trees[k] = t
	return t, nil
}

// DependencyTitle is the title of the TODO created for dep of snap: the
// dependency's title, else the first line of the snapshot message, else a
// generated one; at most MaxTitle bytes.
func DependencyTitle(dep domain.Dependency, snap *domain.Snapshot) string {
	title := strings.TrimSpace(dep.Title)
	if title == "" {
		title = strings.TrimSpace(firstLine(snap.Message))
	}
	if title == "" {
		title = "Dependency of snapshot " + shortID(snap.ID)
	}
	return truncate(title, MaxTitle)
}

func firstLine(s string) string {
	line, _, _ := strings.Cut(s, "\n")
	return strings.TrimRight(line, "\r")
}

func shortID(id string) string {
	if len(id) > 12 {
		return id[:12]
	}
	return id
}

// truncate cuts s to at most n bytes on a rune boundary.
func truncate(s string, n int) string {
	if len(s) <= n {
		return s
	}
	for n > 0 && !utf8.RuneStart(s[n]) {
		n--
	}
	return s[:n]
}

// NormalizePriority maps "" to medium and validates the others.
func NormalizePriority(p string) (string, error) {
	switch strings.ToLower(strings.TrimSpace(p)) {
	case "", domain.TodoPriorityMedium:
		return domain.TodoPriorityMedium, nil
	case domain.TodoPriorityLow:
		return domain.TodoPriorityLow, nil
	case domain.TodoPriorityHigh:
		return domain.TodoPriorityHigh, nil
	case domain.TodoPriorityCritical:
		return domain.TodoPriorityCritical, nil
	}
	return "", errorf(domain.ErrInvalid, "priority must be one of low, medium, high, critical")
}

func validStatus(st string) bool {
	switch st {
	case domain.TodoOpen, domain.TodoInProgress, domain.TodoDone, domain.TodoCancelled:
		return true
	}
	return false
}

func closed(st string) bool { return st == domain.TodoDone || st == domain.TodoCancelled }

func (s *Service) createFromDependency(ctx context.Context, pc *pushCtx, p *domain.Principal, snap *domain.Snapshot, dep domain.Dependency) error {
	target, err := s.lookupTree(ctx, pc, dep.TenantSlug, dep.Tree)
	if err != nil {
		return err
	}
	if target == nil {
		s.skipped(ctx, pc, p, snap, dep, "target tree not found")
		return nil
	}
	if s.d.Authz != nil {
		if err := s.d.Authz.Authorize(ctx, p, domain.ActTreeRead, target.TenantID, target); err != nil {
			s.skipped(ctx, pc, p, snap, dep, "pusher may not read the target tree")
			return nil
		}
	}
	title := DependencyTitle(dep, snap)
	key := SourceKey(snap.ID, target.ID, title)
	var existing sourceRef
	switch err := s.d.Store.GetDoc(ctx, CollSources, key, &existing); {
	case err == nil:
		return nil // already created (re-processed snapshot)
	case !errors.Is(err, domain.ErrNotFound):
		return err
	}
	priority, err := NormalizePriority(dep.Priority)
	if err != nil {
		priority = domain.TodoPriorityMedium
	}
	n, err := s.d.Store.NextSeq(ctx, seqName(target.ID))
	if err != nil {
		return err
	}
	if err := s.d.Store.CreateDoc(ctx, CollSources, key, sourceRef{
		TreeID: target.ID, Number: int(n), SourceSnapshotID: snap.ID, Title: title,
	}); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return nil
		}
		return err
	}
	branch := dep.Branch
	if branch == "" {
		branch = target.DefaultBranch
	}
	now := s.now()
	t := domain.Todo{
		ID:               newID(),
		TenantID:         target.TenantID,
		TreeID:           target.ID,
		Number:           int(n),
		TargetBranch:     branch,
		Title:            title,
		Details:          truncate(dep.Details, MaxDetails),
		Priority:         priority,
		Blocking:         dep.Blocking,
		Linked:           dep.Linked,
		Status:           domain.TodoOpen,
		SourceTenantID:   pc.tree.TenantID,
		SourceTreeID:     pc.tree.ID,
		SourceBranch:     pc.branch,
		SourceSnapshotID: snap.ID,
		CreatedBy:        accountID(p),
		Author:           username(p, pc.actor),
		CreatedAt:        now,
		UpdatedAt:        now,
	}
	if err := s.d.Store.CreateDoc(ctx, CollTodos, TodoKey(t.TreeID, t.Number), t); err != nil {
		return err
	}
	s.publish(domain.EventTodoCreated, &t, t.Author)
	s.record(ctx, p, pc.actor, "todo.create", &t, map[string]string{
		"title":           t.Title,
		"source_tree":     pc.tenant.Slug + "/" + pc.tree.Name,
		"source_snapshot": snap.ID,
		"source_branch":   pc.branch,
	})
	return nil
}

// skipped audits a dependency that did not produce a TODO.
func (s *Service) skipped(ctx context.Context, pc *pushCtx, p *domain.Principal, snap *domain.Snapshot, dep domain.Dependency, reason string) {
	target := dep.Tree
	if dep.TenantSlug != "" {
		target = dep.TenantSlug + "/" + dep.Tree
	}
	s.d.Logger.InfoContext(ctx, "todo: dependency skipped", "snapshot", snap.ID, "target", target, "reason", reason)
	if s.d.Audit == nil {
		return
	}
	s.d.Audit.Record(ctx, domain.AuditEvent{
		ID:           newID(),
		TenantID:     pc.tree.TenantID,
		ActorID:      accountID(p),
		Actor:        username(p, actorOr(pc.actor)),
		Action:       "todo.skipped",
		ResourceType: "snapshot",
		ResourceID:   snap.ID,
		Details: map[string]string{
			"tree":   pc.tenant.Slug + "/" + pc.tree.Name,
			"target": target,
			"title":  DependencyTitle(dep, snap),
			"reason": reason,
		},
		At: s.now(),
	})
}

func actorOr(actor string) string {
	if actor == "" {
		return "system"
	}
	return actor
}

// ---- Completes: references ---------------------------------------------

// CompletesRef is one reference of a "Completes:" line. Tenant and Tree
// are empty for "#N" (the tree the snapshot is pushed to); Tenant is
// empty for "tree#N" (the same tenant).
type CompletesRef struct {
	Tenant string
	Tree   string
	Number int
}

// ParseCompletes extracts the references of every "Completes:" line of a
// snapshot message (case-insensitive keyword; several references may be
// separated by commas or spaces). Malformed references are ignored.
func ParseCompletes(message string) []CompletesRef {
	var out []CompletesRef
	for _, line := range strings.Split(message, "\n") {
		line = strings.TrimSpace(line)
		if len(line) < len("completes:") || !strings.EqualFold(line[:len("completes:")], "completes:") {
			continue
		}
		rest := line[len("completes:"):]
		for _, f := range strings.FieldsFunc(rest, func(r rune) bool { return r == ',' || r == ' ' || r == '\t' }) {
			if ref, ok := parseRef(f); ok {
				out = append(out, ref)
			}
		}
	}
	return out
}

func parseRef(s string) (CompletesRef, bool) {
	scope, num, ok := strings.Cut(s, "#")
	if !ok || num == "" {
		return CompletesRef{}, false
	}
	n := 0
	for _, c := range num {
		if c < '0' || c > '9' || n > 1e8 {
			return CompletesRef{}, false
		}
		n = n*10 + int(c-'0')
	}
	if n < 1 {
		return CompletesRef{}, false
	}
	ref := CompletesRef{Number: n}
	if scope == "" {
		return ref, true
	}
	if tenant, tree, ok := strings.Cut(scope, "/"); ok {
		if tenant == "" || tree == "" || strings.Contains(tree, "/") {
			return CompletesRef{}, false
		}
		ref.Tenant, ref.Tree = tenant, tree
		return ref, true
	}
	ref.Tree = scope
	return ref, true
}

func (s *Service) completeRef(ctx context.Context, pc *pushCtx, p *domain.Principal, snap *domain.Snapshot, ref CompletesRef) error {
	target := pc.tree
	if ref.Tree != "" {
		t, err := s.lookupTree(ctx, pc, ref.Tenant, ref.Tree)
		if err != nil {
			return err
		}
		if t == nil {
			return nil
		}
		target = t
	}
	if target.ID != pc.tree.ID && s.d.Authz != nil {
		if err := s.d.Authz.Authorize(ctx, p, domain.ActTreeWrite, target.TenantID, target); err != nil {
			s.d.Logger.InfoContext(ctx, "todo: completion skipped", "snapshot", snap.ID, "tree", target.ID, "number", ref.Number)
			return nil
		}
	}
	var done *domain.Todo
	err := meta.UpdateDocT(ctx, s.d.Store, CollTodos, TodoKey(target.ID, ref.Number), func(cur *domain.Todo) (*domain.Todo, error) {
		if cur == nil || closed(cur.Status) {
			done = nil
			return cur, nil
		}
		now := s.now()
		cur.Status = domain.TodoDone
		cur.ResolvedBy = snap.ID
		cur.ClosedAt = &now
		cur.UpdatedAt = now
		done = cur
		return cur, nil
	})
	if err != nil || done == nil {
		return err
	}
	s.publish(domain.EventTodoUpdated, done, username(p, pc.actor))
	s.record(ctx, p, pc.actor, "todo.complete", done, map[string]string{
		"resolved_by": snap.ID,
		"via":         "push",
	})
	return nil
}

// ---- queries --------------------------------------------------------------

// Filter selects TODOs of a tree. Empty fields match anything.
type Filter struct {
	// Statuses to include (nil = all).
	Statuses []string
	// Branch matches TargetBranch.
	Branch string
	// Assignee matches the assignee's username.
	Assignee string
}

// ParseStatuses parses a comma-separated status filter. "" and "all" mean
// every status, "active" means open and in_progress.
func ParseStatuses(spec string) ([]string, error) {
	var out []string
	for _, part := range strings.Split(spec, ",") {
		switch part = strings.TrimSpace(part); part {
		case "":
		case "all":
			return nil, nil
		case "active":
			out = append(out, domain.TodoOpen, domain.TodoInProgress)
		default:
			if !validStatus(part) {
				return nil, errorf(domain.ErrInvalid, "status must be open, in_progress, done, cancelled, active or all")
			}
			out = append(out, part)
		}
	}
	return out, nil
}

func (f Filter) match(t *domain.Todo) bool {
	if len(f.Statuses) > 0 && !containsStr(f.Statuses, t.Status) {
		return false
	}
	if f.Branch != "" && t.TargetBranch != f.Branch {
		return false
	}
	if f.Assignee != "" && t.Assignee != f.Assignee {
		return false
	}
	return true
}

func containsStr(list []string, s string) bool {
	for _, x := range list {
		if x == s {
			return true
		}
	}
	return false
}

// List returns the TODOs of treeID matching f, by number.
func (s *Service) List(ctx context.Context, treeID string, f Filter) ([]domain.Todo, error) {
	all, err := meta.ListDocsT[domain.Todo](ctx, s.d.Store, CollTodos, meta.DocPrefix(treeID))
	if err != nil {
		return nil, err
	}
	out := make([]domain.Todo, 0, len(all))
	for i := range all {
		if f.match(&all[i]) {
			out = append(out, all[i])
		}
	}
	return out, nil
}

// Get returns TODO n of treeID.
func (s *Service) Get(ctx context.Context, treeID string, n int) (*domain.Todo, error) {
	t, err := meta.GetDocT[domain.Todo](ctx, s.d.Store, CollTodos, TodoKey(treeID, n))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, errorf(domain.ErrNotFound, "todo #%d not found", n)
		}
		return nil, err
	}
	return t, nil
}

// OpenBlocking returns the open or in-progress blocking TODOs created by
// dependencies declared on sourceBranch of sourceTreeID (for merge
// protection rules with BlockOnTodos). It scans every TODO.
func (s *Service) OpenBlocking(ctx context.Context, sourceTreeID, sourceBranch string) ([]domain.Todo, error) {
	all, err := meta.ListDocsT[domain.Todo](ctx, s.d.Store, CollTodos, "")
	if err != nil {
		return nil, err
	}
	out := []domain.Todo{}
	for _, t := range all {
		if t.Blocking && !closed(t.Status) && t.SourceTreeID == sourceTreeID &&
			t.SourceSnapshotID != "" && (sourceBranch == "" || t.SourceBranch == sourceBranch) {
			out = append(out, t)
		}
	}
	return out, nil
}

// ---- manual changes --------------------------------------------------------

// CreateInput is a manually created TODO.
type CreateInput struct {
	Title        string `json:"title"`
	Details      string `json:"details"`
	Priority     string `json:"priority"`
	Blocking     bool   `json:"blocking"`
	TargetBranch string `json:"target_branch"`
}

func validateText(field, v string, max int, required bool) error {
	if required && strings.TrimSpace(v) == "" {
		return errorf(domain.ErrInvalid, "%s is required", field)
	}
	if len(v) > max {
		return errorf(domain.ErrInvalid, "%s must be at most %d bytes", field, max)
	}
	if !utf8.ValidString(v) {
		return errorf(domain.ErrInvalid, "%s must be valid UTF-8", field)
	}
	return nil
}

// Create adds a manual TODO to tree (the caller is its author; the source
// is the tree itself, with no snapshot).
func (s *Service) Create(ctx context.Context, p *domain.Principal, tree *domain.Tree, in CreateInput) (*domain.Todo, error) {
	title := strings.TrimSpace(in.Title)
	if err := validateText("title", title, MaxTitle, true); err != nil {
		return nil, err
	}
	if err := validateText("details", in.Details, MaxDetails, false); err != nil {
		return nil, err
	}
	if err := validateText("target_branch", in.TargetBranch, 255, false); err != nil {
		return nil, err
	}
	priority, err := NormalizePriority(in.Priority)
	if err != nil {
		return nil, err
	}
	branch := in.TargetBranch
	if branch == "" {
		branch = tree.DefaultBranch
	}
	n, err := s.d.Store.NextSeq(ctx, seqName(tree.ID))
	if err != nil {
		return nil, err
	}
	now := s.now()
	t := domain.Todo{
		ID:             newID(),
		TenantID:       tree.TenantID,
		TreeID:         tree.ID,
		Number:         int(n),
		TargetBranch:   branch,
		Title:          title,
		Details:        in.Details,
		Priority:       priority,
		Blocking:       in.Blocking,
		Status:         domain.TodoOpen,
		SourceTenantID: tree.TenantID,
		SourceTreeID:   tree.ID,
		CreatedBy:      accountID(p),
		Author:         username(p, "anonymous"),
		CreatedAt:      now,
		UpdatedAt:      now,
	}
	if err := s.d.Store.CreateDoc(ctx, CollTodos, TodoKey(tree.ID, t.Number), t); err != nil {
		return nil, err
	}
	s.publish(domain.EventTodoCreated, &t, t.Author)
	s.record(ctx, p, "", "todo.create", &t, map[string]string{"title": t.Title, "manual": "true"})
	return &t, nil
}

// Patch is a partial update; nil fields are unchanged.
type Patch struct {
	Title    *string `json:"title"`
	Details  *string `json:"details"`
	Priority *string `json:"priority"`
	Blocking *bool   `json:"blocking"`
	Status   *string `json:"status"`
}

// Update applies patch to TODO n of tree. Closing (done/cancelled) sets
// ClosedAt; reopening clears ClosedAt and ResolvedBy.
func (s *Service) Update(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int, patch Patch) (*domain.Todo, error) {
	if patch.Title != nil {
		*patch.Title = strings.TrimSpace(*patch.Title)
		if err := validateText("title", *patch.Title, MaxTitle, true); err != nil {
			return nil, err
		}
	}
	if patch.Details != nil {
		if err := validateText("details", *patch.Details, MaxDetails, false); err != nil {
			return nil, err
		}
	}
	var priority string
	if patch.Priority != nil {
		var err error
		if priority, err = NormalizePriority(*patch.Priority); err != nil {
			return nil, err
		}
	}
	if patch.Status != nil && !validStatus(*patch.Status) {
		return nil, errorf(domain.ErrInvalid, "status must be open, in_progress, done or cancelled")
	}
	details := map[string]string{}
	return s.mutate(ctx, p, tree, n, "todo.update", details, func(t *domain.Todo, now time.Time) (bool, error) {
		changed := false
		if patch.Title != nil && *patch.Title != t.Title {
			t.Title, changed = *patch.Title, true
			details["title"] = t.Title
		}
		if patch.Details != nil && *patch.Details != t.Details {
			t.Details, changed = *patch.Details, true
			details["details"] = "changed"
		}
		if patch.Priority != nil && priority != t.Priority {
			t.Priority, changed = priority, true
			details["priority"] = priority
		}
		if patch.Blocking != nil && *patch.Blocking != t.Blocking {
			t.Blocking, changed = *patch.Blocking, true
			details["blocking"] = fmt.Sprint(t.Blocking)
		}
		if patch.Status != nil && *patch.Status != t.Status {
			setStatus(t, *patch.Status, now)
			changed = true
			details["status"] = t.Status
		}
		return changed, nil
	})
}

func setStatus(t *domain.Todo, st string, now time.Time) {
	t.Status = st
	if closed(st) {
		t.ClosedAt = &now
	} else {
		t.ClosedAt = nil
		t.ResolvedBy = ""
	}
}

// Claim assigns TODO n to the caller and marks it in progress.
func (s *Service) Claim(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int) (*domain.Todo, error) {
	if p == nil {
		return nil, errorf(domain.ErrUnauthenticated, "authentication required")
	}
	return s.mutate(ctx, p, tree, n, "todo.claim", map[string]string{"assignee": p.Username}, func(t *domain.Todo, now time.Time) (bool, error) {
		if closed(t.Status) {
			return false, errorf(domain.ErrConflict, "todo #%d is %s", n, t.Status)
		}
		if t.Assignee == p.Username && t.Status == domain.TodoInProgress {
			return false, nil
		}
		t.AssigneeID, t.Assignee = p.AccountID, p.Username
		setStatus(t, domain.TodoInProgress, now)
		return true, nil
	})
}

// Complete marks TODO n done, optionally resolved by snapshotID.
func (s *Service) Complete(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int, snapshotID string) (*domain.Todo, error) {
	return s.mutate(ctx, p, tree, n, "todo.complete", map[string]string{"resolved_by": snapshotID}, func(t *domain.Todo, now time.Time) (bool, error) {
		if t.Status == domain.TodoDone && (snapshotID == "" || t.ResolvedBy == snapshotID) {
			return false, nil
		}
		setStatus(t, domain.TodoDone, now)
		t.ResolvedBy = snapshotID
		return true, nil
	})
}

// Cancel marks TODO n cancelled.
func (s *Service) Cancel(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int) (*domain.Todo, error) {
	return s.mutate(ctx, p, tree, n, "todo.cancel", nil, func(t *domain.Todo, now time.Time) (bool, error) {
		switch t.Status {
		case domain.TodoCancelled:
			return false, nil
		case domain.TodoDone:
			return false, errorf(domain.ErrConflict, "todo #%d is already done", n)
		}
		setStatus(t, domain.TodoCancelled, now)
		return true, nil
	})
}

// mutate atomically applies fn to TODO n; when fn reports a change the
// TODO is saved, todo.updated is published and action audited.
func (s *Service) mutate(ctx context.Context, p *domain.Principal, tree *domain.Tree, n int, action string, details map[string]string,
	fn func(t *domain.Todo, now time.Time) (bool, error)) (*domain.Todo, error) {
	var out *domain.Todo
	changed := false
	err := meta.UpdateDocT(ctx, s.d.Store, CollTodos, TodoKey(tree.ID, n), func(cur *domain.Todo) (*domain.Todo, error) {
		if cur == nil {
			return nil, errorf(domain.ErrNotFound, "todo #%d not found", n)
		}
		now := s.now()
		c, err := fn(cur, now)
		if err != nil {
			return nil, err
		}
		changed = c
		if c {
			cur.UpdatedAt = now
		}
		out = cur
		return cur, nil
	})
	if err != nil {
		return nil, err
	}
	if changed {
		s.publish(domain.EventTodoUpdated, out, username(p, "anonymous"))
		s.record(ctx, p, "", action, out, details)
	}
	return out, nil
}

// ---- dependency report -----------------------------------------------------

// TodoRef identifies the TODO created for an outgoing dependency.
type TodoRef struct {
	Tenant string `json:"tenant"`
	Tree   string `json:"tree"`
	Number int    `json:"number"`
	Status string `json:"status"`
	Title  string `json:"title,omitempty"`
}

// Outgoing is a dependency declared by a snapshot of the reported branch.
type Outgoing struct {
	SnapshotID string            `json:"snapshot_id"`
	Dependency domain.Dependency `json:"dependency"`
	Todo       *TodoRef          `json:"todo"`
}

// Report is the answer of GET {tree}/dependencies.
type Report struct {
	Branch   string        `json:"branch"`
	Outgoing []Outgoing    `json:"outgoing"`
	Incoming []domain.Todo `json:"incoming"`
	// Trees names the trees referenced by Incoming (source_tree_id) and
	// Outgoing as "tenant/tree", for the trees the caller may read.
	Trees map[string]string `json:"trees"`
}

// maxReportSnapshots bounds the history walked for outgoing dependencies.
const maxReportSnapshots = 1000

// Dependencies reports the dependencies declared on branch of tree ("" =
// the default branch; outgoing, newest snapshot first) and the TODOs other
// trees requested from it (incoming; all branches when branch is ""). TODO
// details of other trees are included only when p may read them.
func (s *Service) Dependencies(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) (*Report, error) {
	ten, err := s.d.Store.GetTenant(ctx, tree.TenantID)
	if err != nil {
		return nil, err
	}
	rep := &Report{Branch: branch, Outgoing: []Outgoing{}, Incoming: []domain.Todo{},
		Trees: map[string]string{tree.ID: ten.Slug + "/" + tree.Name}}
	pc := &pushCtx{tree: tree, tenant: ten, principals: map[string]*domain.Principal{},
		trees: map[[2]string]*domain.Tree{}, tenants: map[string]*domain.Tenant{ten.Slug: ten}}
	readable := map[string]bool{tree.ID: true}
	canRead := func(t *domain.Tree) bool {
		if ok, seen := readable[t.ID]; seen {
			return ok
		}
		ok := s.d.Authz == nil || s.d.Authz.Authorize(ctx, p, domain.ActTreeRead, t.TenantID, t) == nil
		readable[t.ID] = ok
		return ok
	}

	outBranch := branch
	if outBranch == "" {
		outBranch = tree.DefaultBranch
	}
	if rep.Branch == "" {
		rep.Branch = outBranch
	}
	b, err := s.d.Store.GetBranch(ctx, tree.ID, outBranch)
	switch {
	case err == nil && b.Tip != "" && s.d.History != nil:
		snaps, err := s.d.History.Log(ctx, b.Tip, maxReportSnapshots)
		if err != nil {
			return nil, err
		}
		for i := range snaps {
			snap := &snaps[i]
			for _, dep := range snap.Dependencies {
				o := Outgoing{SnapshotID: snap.ID, Dependency: dep}
				target, err := s.lookupTree(ctx, pc, dep.TenantSlug, dep.Tree)
				if err != nil {
					return nil, err
				}
				if target != nil && canRead(target) {
					if ref, err := s.todoRefFor(ctx, pc, snap, dep, target); err != nil {
						return nil, err
					} else if ref != nil {
						o.Todo = ref
						rep.Trees[target.ID] = ref.Tenant + "/" + ref.Tree
					}
				}
				rep.Outgoing = append(rep.Outgoing, o)
			}
		}
	case err != nil && !errors.Is(err, domain.ErrNotFound):
		return nil, err
	}

	todos, err := s.List(ctx, tree.ID, Filter{Branch: branch})
	if err != nil {
		return nil, err
	}
	for _, t := range todos {
		if t.SourceSnapshotID == "" {
			continue // manual
		}
		rep.Incoming = append(rep.Incoming, t)
		if _, ok := rep.Trees[t.SourceTreeID]; ok {
			continue
		}
		src, err := s.d.Store.GetTree(ctx, t.SourceTreeID)
		if err != nil || !canRead(src) {
			continue
		}
		if st, err := s.d.Store.GetTenant(ctx, src.TenantID); err == nil {
			rep.Trees[src.ID] = st.Slug + "/" + src.Name
		}
	}
	return rep, nil
}

func (s *Service) todoRefFor(ctx context.Context, pc *pushCtx, snap *domain.Snapshot, dep domain.Dependency, target *domain.Tree) (*TodoRef, error) {
	var ref sourceRef
	if err := s.d.Store.GetDoc(ctx, CollSources, SourceKey(snap.ID, target.ID, DependencyTitle(dep, snap)), &ref); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil
		}
		return nil, err
	}
	t, err := s.Get(ctx, ref.TreeID, ref.Number)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil
		}
		return nil, err
	}
	slug := pc.tenant.Slug
	if target.TenantID != pc.tenant.ID {
		ten, err := s.d.Store.GetTenant(ctx, target.TenantID)
		if err != nil {
			return nil, err
		}
		slug = ten.Slug
	}
	return &TodoRef{Tenant: slug, Tree: target.Name, Number: t.Number, Status: t.Status, Title: t.Title}, nil
}

// ---- cleanup ----------------------------------------------------------------

// DeleteTree removes the TODOs of treeID and their index entries.
func (s *Service) DeleteTree(ctx context.Context, treeID string) error {
	if _, err := s.d.Store.DeleteDocs(ctx, CollTodos, meta.DocPrefix(treeID)); err != nil {
		return err
	}
	refs, err := meta.ListDocsT[sourceRef](ctx, s.d.Store, CollSources, "")
	if err != nil {
		return err
	}
	for _, r := range refs {
		if r.TreeID != treeID || r.SourceSnapshotID == "" {
			continue
		}
		err := s.d.Store.DeleteDoc(ctx, CollSources, SourceKey(r.SourceSnapshotID, r.TreeID, r.Title))
		if err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	return nil
}

// ---- events and audit -------------------------------------------------------

func (s *Service) publish(typ string, t *domain.Todo, actor string) {
	if s.d.Bus == nil {
		return
	}
	cp := *t
	s.d.Bus.Publish(domain.Event{Type: typ, TenantID: t.TenantID, TreeID: t.TreeID, Actor: actor, Payload: &cp, At: s.now()})
}

func (s *Service) record(ctx context.Context, p *domain.Principal, actor, action string, t *domain.Todo, details map[string]string) {
	if s.d.Audit == nil {
		return
	}
	d := map[string]string{"number": fmt.Sprint(t.Number)}
	for k, v := range details {
		if v != "" {
			d[k] = v
		}
	}
	s.d.Audit.Record(context.WithoutCancel(ctx), domain.AuditEvent{
		ID:           newID(),
		TenantID:     t.TenantID,
		ActorID:      accountID(p),
		Actor:        username(p, actorOr(actor)),
		Action:       action,
		ResourceType: "todo",
		ResourceID:   fmt.Sprintf("%s#%d", t.TreeID, t.Number),
		Details:      d,
		At:           s.now(),
	})
}

// newID returns a random RFC 4122 version 4 UUID string.
func newID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	b[6] = b[6]&0x0f | 0x40
	b[8] = b[8]&0x3f | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}
