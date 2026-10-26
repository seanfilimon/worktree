// Package review implements domain.ReviewService: server-side branch
// protection enforcement and the built-in merge request workflow.
//
// Branch protection follows the spec's ceiling model: every rule whose
// pattern matches a branch applies, and the rules are merged into their most
// restrictive combination (see Resolve). Protection is never bypassed, not
// even for server admins.
//
// Merge requests merge with one of three methods (v0.6, merge.go):
// fast-forward moves the target branch tip to the source tip; merge writes
// a server-side merge snapshot (parents [target tip, source tip]) whose
// manifest is the three-way merge of internal/merge3; squash writes one
// snapshot (parent [target tip]) with the combined changes. The target
// always moves with a compare-and-swap. Branch protection may restrict the
// methods; conflicts refuse the merge (*ConflictError).
//
// v0.5 (docs/ENGINEERING_PLAN.md §13.4) adds review decisions with stale
// approvals (decisions.go), merge readiness checks: CI, TODOs, linear
// history (checks.go), merge request comments (comments.go), linked
// groups that merge atomically across trees (groups.go), tag protections
// (tags.go) and the RequireLinear push check (pushcheck.go). Their state
// lives in DocStore collections mr_comments, linked_groups and
// tag_protections; CI results and TODOs are read from ci_status and todos.
//
// Concurrency: the MetaStore's UpdateMergeRequest is a plain overwrite, so
// every read-modify-write of a merge request is serialized per tree with an
// in-process lock. Branch tips are additionally guarded by the store's
// CompareAndSwapBranchTip, which also protects against concurrent pushes and
// other server processes.
package review

import (
	"context"
	"errors"
	"fmt"
	"hash/fnv"
	"log/slog"
	"slices"
	"strconv"
	"sync"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// lockStripes is the number of mutexes tree IDs are hashed onto. Striping
// bounds memory regardless of how many trees exist; unrelated trees that
// share a stripe merely serialize their merge-request writes.
const lockStripes = 256

// Service enforces branch protection and manages merge requests. It is safe
// for concurrent use.
type Service struct {
	store domain.MetaStore
	hist  domain.History
	authz domain.Authorizer
	bus   domain.EventBus
	audit domain.AuditLog

	now   func() time.Time
	locks [lockStripes]sync.Mutex
	// groupMu serializes linked-group operations (taken before any tree
	// lock).
	groupMu sync.Mutex
	log     *slog.Logger

	// v0.6 server-side merges (§14.1, merge.go). objects is nil until
	// SetObjects: merge requests can then only be fast-forwarded.
	objects objstore.Store
	newID   func() string
	merges  mergeCache

	// v0.7 (§15.7, suggest.go): quota checks server-side merge and squash
	// snapshots (nil = unchecked); pushChecks run for suggestion snapshots
	// on the source branch (nil = quota only).
	quota      domain.PushChecker
	pushChecks []domain.PushChecker
}

// SetObjects gives the service the object store, enabling the server-side
// merge methods "merge" and "squash" (§14.1). Without it merge requests can
// only be fast-forwarded. Call before serving requests.
func (s *Service) SetObjects(objs objstore.Store) { s.objects = objs }

var (
	_ domain.ReviewService = (*Service)(nil)
	_ domain.PushChecker   = (*Service)(nil)
)

// SetLogger sets the logger for background failures (default slog.Default).
func (s *Service) SetLogger(l *slog.Logger) { s.log = l }

func (s *Service) logger() *slog.Logger {
	if s.log == nil {
		return slog.Default()
	}
	return s.log
}

// New returns a review service. bus and audit may be nil, in which case
// events and audit records are not emitted.
func New(store domain.MetaStore, hist domain.History, authz domain.Authorizer, bus domain.EventBus, audit domain.AuditLog) *Service {
	return &Service{
		store: store,
		hist:  hist,
		authz: authz,
		bus:   bus,
		audit: audit,
		now:   func() time.Time { return time.Now().UTC() },
		newID: uuid.NewString,
	}
}

// lockTree acquires the per-tree lock and returns its unlock function.
func (s *Service) lockTree(treeID string) func() {
	h := fnv.New32a()
	_, _ = h.Write([]byte(treeID))
	mu := &s.locks[h.Sum32()%lockStripes]
	mu.Lock()
	return mu.Unlock
}

// ProtectionFor returns the effective protection of branch in the tree.
func (s *Service) ProtectionFor(ctx context.Context, treeID, branch string) (Effective, error) {
	rules, err := s.store.ListProtections(ctx, treeID)
	if err != nil {
		return Effective{}, fmt.Errorf("review: list protections: %w", err)
	}
	return Resolve(rules, branch), nil
}

// CheckPush enforces branch protection for a direct push of branch. It
// first requires domain.ActTreeWrite (returning the authorizer's error
// unchanged), then rejects with *domain.PushDenied{Kind:
// "branch_protection"} when the branch forbids direct pushes or the caller
// is not among the allowed pushers.
func (s *Service) CheckPush(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) error {
	if err := checkTree(tree); err != nil {
		return err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeWrite, tree.TenantID, tree); err != nil {
		return err
	}
	eff, err := s.ProtectionFor(ctx, tree.ID, branch)
	if err != nil {
		return err
	}
	if eff.NoDirectPush {
		return &domain.PushDenied{
			Kind:   "branch_protection",
			Reason: fmt.Sprintf("branch '%s' is protected: changes must go through a merge request", branch),
		}
	}
	if !eff.MayPush(username(p)) {
		return &domain.PushDenied{
			Kind:   "branch_protection",
			Reason: fmt.Sprintf("branch '%s' is protected: %s is not an allowed pusher", branch, displayName(p)),
		}
	}
	return nil
}

// CheckDeleteBranch requires domain.ActBranchDelete and rejects deleting the
// tree's default branch or a branch protected with NoDelete with
// *domain.PushDenied{Kind: "branch_protection"}.
func (s *Service) CheckDeleteBranch(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch string) error {
	if err := checkTree(tree); err != nil {
		return err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActBranchDelete, tree.TenantID, tree); err != nil {
		return err
	}
	if branch != "" && branch == tree.DefaultBranch {
		return &domain.PushDenied{
			Kind:   "branch_protection",
			Reason: fmt.Sprintf("branch '%s' is the default branch and cannot be deleted", branch),
		}
	}
	eff, err := s.ProtectionFor(ctx, tree.ID, branch)
	if err != nil {
		return err
	}
	if eff.NoDelete {
		return &domain.PushDenied{
			Kind:   "branch_protection",
			Reason: fmt.Sprintf("branch '%s' is protected and cannot be deleted", branch),
		}
	}
	return nil
}

// checkTree guards against a nil tree, which would otherwise panic.
func checkTree(tree *domain.Tree) error {
	if tree == nil || tree.ID == "" {
		return fmt.Errorf("review: tree is required: %w", domain.ErrInvalid)
	}
	return nil
}

// requirePrincipal returns ErrUnauthenticated for anonymous callers. It is
// a defensive backstop after authorization for operations that record the
// caller's identity.
func requirePrincipal(p *domain.Principal) error {
	if p.Anonymous() || p.AccountID == "" {
		return fmt.Errorf("review: sign-in required: %w", domain.ErrUnauthenticated)
	}
	return nil
}

func username(p *domain.Principal) string {
	if p.Anonymous() {
		return ""
	}
	return p.Username
}

func displayName(p *domain.Principal) string {
	if p.Anonymous() || p.Username == "" {
		return "anonymous"
	}
	return "user '" + p.Username + "'"
}

// publish emits an event if a bus is configured.
func (s *Service) publish(typ string, p *domain.Principal, tree *domain.Tree, payload any) {
	if s.bus == nil {
		return
	}
	s.bus.Publish(domain.Event{
		Type:     typ,
		TenantID: tree.TenantID,
		TreeID:   tree.ID,
		Actor:    username(p),
		Payload:  payload,
		At:       s.now(),
	})
}

// record writes an audit event about a merge request if an audit log is
// configured.
func (s *Service) record(ctx context.Context, action string, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, extra map[string]string) {
	if s.audit == nil {
		return
	}
	details := map[string]string{
		"tree":          tree.Name,
		"tree_id":       tree.ID,
		"number":        strconv.Itoa(mr.Number),
		"source_branch": mr.SourceBranch,
		"target_branch": mr.TargetBranch,
	}
	for k, v := range extra {
		details[k] = v
	}
	ev := domain.AuditEvent{
		TenantID:     tree.TenantID,
		Actor:        "anonymous",
		Action:       action,
		ResourceType: "merge_request",
		ResourceID:   mr.ID,
		Details:      details,
		At:           s.now(),
	}
	if !p.Anonymous() {
		ev.ActorID, ev.Actor = p.AccountID, p.Username
	}
	s.audit.Record(ctx, ev)
}

// snapshotMR returns a deep copy of mr for use as an event payload, so
// subscribers never alias the value returned to the caller.
func snapshotMR(mr *domain.MergeRequest) domain.MergeRequest {
	c := *mr
	c.Approvals = slices.Clone(mr.Approvals)
	c.Reviewers = slices.Clone(mr.Reviewers) // v0.7 (§15.7)
	if mr.MergedAt != nil {
		t := *mr.MergedAt
		c.MergedAt = &t
	}
	return c
}

// isNotFound reports whether err wraps domain.ErrNotFound.
func isNotFound(err error) bool { return errors.Is(err, domain.ErrNotFound) }
