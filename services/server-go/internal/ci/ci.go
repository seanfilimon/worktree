// Package ci stores CI check results reported for snapshots and evaluates
// their combined state (docs/ENGINEERING_PLAN.md §13.4).
//
// Every result is one domain.CIStatus document in the `ci_status`
// collection, keyed by DocKey(treeID, snapshotID, context): a newer report
// for the same check replaces the older one. Reporting requires the
// `ci:report` action and publishes a `ci.status` event. The combined state
// of a snapshot is `failure` when any check failed or errored, else
// `pending` when any check is pending, else `success` ("" without checks).
//
// Tree deletion removes the tree's documents (Start subscribes to
// `tree.deleted`).
package ci

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"sort"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Collection is the document collection holding CI results.
const Collection = "ci_status"

// Field limits.
const (
	MaxContextLen     = 255
	MaxDescriptionLen = 1024
	MaxTargetURLLen   = 2048
)

// Deps are the collaborators of the ci package. Store is required;
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

// Service is the ci component. It is safe for concurrent use.
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

// Report is the input of Service.Report.
type Report struct {
	Snapshot    string `json:"snapshot"`
	Context     string `json:"context"`
	State       string `json:"state"`
	Description string `json:"description,omitempty"`
	TargetURL   string `json:"target_url,omitempty"`
}

// Status is the combined CI view of one snapshot.
type Status struct {
	Snapshot string            `json:"snapshot"`
	State    string            `json:"state"`
	Checks   []domain.CIStatus `json:"checks"`
}

// ValidState reports whether s is one of the CI states.
func ValidState(s string) bool {
	switch s {
	case domain.CIPending, domain.CISuccess, domain.CIFailure, domain.CIError:
		return true
	}
	return false
}

// ValidateContext checks a check name: 1-255 bytes of printable UTF-8
// without NUL (it is a document key component).
func ValidateContext(c string) error {
	switch {
	case strings.TrimSpace(c) == "" || len(c) > MaxContextLen:
		return fmt.Errorf("context must be 1-%d bytes: %w", MaxContextLen, domain.ErrInvalid)
	case !utf8.ValidString(c):
		return fmt.Errorf("context must be valid UTF-8: %w", domain.ErrInvalid)
	case strings.ContainsFunc(c, unicode.IsControl):
		return fmt.Errorf("context must not contain control characters: %w", domain.ErrInvalid)
	}
	return nil
}

// Combine returns the combined state of checks: failure if any failed or
// errored, else pending if any is pending, else success; "" for none.
func Combine(checks []domain.CIStatus) string {
	if len(checks) == 0 {
		return ""
	}
	pending := false
	for _, c := range checks {
		switch c.State {
		case domain.CIFailure, domain.CIError:
			return domain.CIFailure
		case domain.CIPending:
			pending = true
		}
	}
	if pending {
		return domain.CIPending
	}
	return domain.CISuccess
}

// List returns the checks reported for a snapshot of a tree, sorted by
// context. It reads the store directly (no authorization) so other
// packages (merge readiness) can use it.
func List(ctx context.Context, docs domain.DocStore, treeID, snapshotID string) ([]domain.CIStatus, error) {
	if treeID == "" || snapshotID == "" {
		return []domain.CIStatus{}, nil
	}
	out, err := meta.ListDocsT[domain.CIStatus](ctx, docs, Collection, meta.DocPrefix(treeID, snapshotID))
	if err != nil {
		return nil, fmt.Errorf("ci: list statuses: %w", err)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Context < out[j].Context })
	return out, nil
}

// Report records a check result for a snapshot of tree. It requires
// domain.ActCIReport; the snapshot must belong to the tree.
func (s *Service) Report(ctx context.Context, p *domain.Principal, tree *domain.Tree, r Report) (*domain.CIStatus, error) {
	if tree == nil || tree.ID == "" {
		return nil, fmt.Errorf("ci: tree is required: %w", domain.ErrInvalid)
	}
	if s.d.Authz != nil {
		if err := s.d.Authz.Authorize(ctx, p, domain.ActCIReport, tree.TenantID, tree); err != nil {
			return nil, err
		}
	}
	if p == nil {
		return nil, fmt.Errorf("ci: sign-in required: %w", domain.ErrUnauthenticated)
	}
	r.Snapshot = strings.TrimSpace(r.Snapshot)
	if r.Snapshot == "" || strings.ContainsRune(r.Snapshot, 0) {
		return nil, fmt.Errorf("snapshot is required: %w", domain.ErrInvalid)
	}
	if err := ValidateContext(r.Context); err != nil {
		return nil, err
	}
	if !ValidState(r.State) {
		return nil, fmt.Errorf("state must be one of pending, success, failure, error: %w", domain.ErrInvalid)
	}
	if len(r.Description) > MaxDescriptionLen || !utf8.ValidString(r.Description) {
		return nil, fmt.Errorf("description must be at most %d bytes of UTF-8: %w", MaxDescriptionLen, domain.ErrInvalid)
	}
	if err := validateURL(r.TargetURL); err != nil {
		return nil, err
	}
	snap, err := s.d.Store.GetSnapshot(ctx, r.Snapshot)
	switch {
	case errors.Is(err, domain.ErrNotFound), err == nil && snap.TreeID != tree.ID:
		return nil, fmt.Errorf("snapshot %s not found in tree %s: %w", r.Snapshot, tree.Name, domain.ErrNotFound)
	case err != nil:
		return nil, fmt.Errorf("ci: get snapshot: %w", err)
	}
	st := &domain.CIStatus{
		TreeID:      tree.ID,
		SnapshotID:  snap.ID,
		Context:     r.Context,
		State:       r.State,
		Description: r.Description,
		TargetURL:   r.TargetURL,
		Reporter:    p.Username,
		UpdatedAt:   s.now(),
	}
	if err := s.d.Store.PutDoc(ctx, Collection, domain.DocKey(tree.ID, snap.ID, r.Context), st); err != nil {
		return nil, fmt.Errorf("ci: store status: %w", err)
	}
	if s.d.Bus != nil {
		s.d.Bus.Publish(domain.Event{
			Type:     domain.EventCIStatus,
			TenantID: tree.TenantID,
			TreeID:   tree.ID,
			Actor:    p.Username,
			Payload:  *st,
			At:       st.UpdatedAt,
		})
	}
	if s.d.Audit != nil {
		s.d.Audit.Record(ctx, domain.AuditEvent{
			TenantID:     tree.TenantID,
			ActorID:      p.AccountID,
			Actor:        p.Username,
			Action:       "ci.report",
			ResourceType: "snapshot",
			ResourceID:   snap.ID,
			Details: map[string]string{
				"tree": tree.Name, "tree_id": tree.ID, "context": st.Context, "state": st.State,
			},
			At: st.UpdatedAt,
		})
	}
	return st, nil
}

// Status returns the combined CI status of a snapshot of tree. It requires
// domain.ActTreeRead.
func (s *Service) Status(ctx context.Context, p *domain.Principal, tree *domain.Tree, snapshotID string) (*Status, error) {
	if tree == nil || tree.ID == "" {
		return nil, fmt.Errorf("ci: tree is required: %w", domain.ErrInvalid)
	}
	if s.d.Authz != nil {
		if err := s.d.Authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
			return nil, err
		}
	}
	checks, err := List(ctx, s.d.Store, tree.ID, snapshotID)
	if err != nil {
		return nil, err
	}
	return &Status{Snapshot: snapshotID, State: Combine(checks), Checks: checks}, nil
}

// Start subscribes to tree.deleted and removes the deleted tree's
// statuses until ctx ends. It is a no-op without a bus.
func (s *Service) Start(ctx context.Context) {
	if s.d.Bus == nil {
		return
	}
	ch, cancel := s.d.Bus.Subscribe(domain.EventFilter{Types: []string{domain.EventTreeDeleted}})
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
				s.DeleteTree(context.WithoutCancel(ctx), e.TreeID)
			}
		}
	}()
}

// DeleteTree removes every status of a tree.
func (s *Service) DeleteTree(ctx context.Context, treeID string) {
	if treeID == "" {
		return
	}
	if _, err := s.d.Store.DeleteDocs(ctx, Collection, meta.DocPrefix(treeID)); err != nil {
		s.d.Logger.Warn("ci: delete statuses of deleted tree", slog.String("tree_id", treeID), slog.Any("error", err))
	}
}

func validateURL(u string) error {
	if u == "" {
		return nil
	}
	if len(u) > MaxTargetURLLen || !utf8.ValidString(u) || strings.ContainsFunc(u, unicode.IsSpace) {
		return fmt.Errorf("target_url must be at most %d bytes without whitespace: %w", MaxTargetURLLen, domain.ErrInvalid)
	}
	if !strings.HasPrefix(u, "https://") && !strings.HasPrefix(u, "http://") {
		return fmt.Errorf("target_url must be an http(s) URL: %w", domain.ErrInvalid)
	}
	return nil
}
