package review

import (
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/ci"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// TodoCollection is the document collection of TODOs (owned by the todo
// package, §13.5); merge readiness reads it for BlockOnTodos.
const TodoCollection = "todos"

// Blocker kinds reported by Checks.
const (
	BlockerState            = "state"             // the merge request is not open
	BlockerSource           = "source"            // the source branch is missing or empty
	BlockerLinkedGroup      = "linked_group"      // member of an open linked group
	BlockerApprovals        = "approvals"         // not enough current approvals
	BlockerChangesRequested = "changes_requested" // a reviewer requested changes
	BlockerCI               = "ci"                // required CI checks are not successful
	BlockerTodos            = "todos"             // open blocking TODOs target the source branch
	BlockerLinear           = "linear"            // the source brings merge snapshots
	BlockerDiverged         = "diverged"          // the target is not an ancestor of the source
)

// Error codes of *BlockedError.
const (
	CodeMergeBlocked = "merge_blocked"
	CodeLinkedGroup  = "linked_group"
)

// Blocker is one reason a merge request cannot be merged.
type Blocker struct {
	Kind    string `json:"kind"`
	Message string `json:"message"`
}

// ApprovalSummary counts the approvals of a merge request.
type ApprovalSummary struct {
	Required int `json:"required"`
	Current  int `json:"current"` // approvals of the current source tip
	Stale    int `json:"stale"`   // approvals of an older source tip
	// ChangesRequested lists the reviewers whose change request is open.
	ChangesRequested []string `json:"changes_requested"`
}

// CISummary is the CI state of the source tip.
type CISummary struct {
	State    string   `json:"state"`    // combined state of every reported check ("" = none)
	Required []string `json:"required"` // required contexts (RequireCIPass)
	Missing  []string `json:"missing"`  // required contexts without a report
}

// TodoSummary counts the open blocking TODOs of the source branch.
type TodoSummary struct {
	BlockingOpen int `json:"blocking_open"`
}

// MergeChecks is the merge readiness of a merge request.
type MergeChecks struct {
	Mergeable bool            `json:"mergeable"`
	Blockers  []Blocker       `json:"blockers"`
	Approvals ApprovalSummary `json:"approvals"`
	CI        CISummary       `json:"ci"`
	Todos     TodoSummary     `json:"todos"`
}

// BlockedError reports a merge refused by merge readiness checks. It wraps
// domain.ErrConflict, so callers that do not know it answer 409.
type BlockedError struct {
	Code     string    // CodeMergeBlocked or CodeLinkedGroup
	Message  string    // human-readable, every blocker
	Blockers []Blocker // the blockers that refused the merge
}

func (e *BlockedError) Error() string { return e.Message }

// Unwrap makes errors.Is(err, domain.ErrConflict) hold.
func (e *BlockedError) Unwrap() error { return domain.ErrConflict }

// evaluation is the merge readiness of one merge request plus the data a
// merge needs.
type evaluation struct {
	mr        *domain.MergeRequest
	eff       Effective
	sourceTip string
	targetTip string
	upToDate  bool

	sourceErr   error // missing/empty source branch
	divergedErr error // classifyMerge's conflict
	checks      MergeChecks
}

func (ev *evaluation) block(kind, format string, args ...any) {
	ev.checks.Blockers = append(ev.checks.Blockers, Blocker{Kind: kind, Message: fmt.Sprintf(format, args...)})
}

func (ev *evaluation) blockers(kinds ...string) []Blocker {
	var out []Blocker
	for _, b := range ev.checks.Blockers {
		if slices.Contains(kinds, b.Kind) {
			out = append(out, b)
		}
	}
	return out
}

// mergeError converts the blockers into the error Merge returns, nil when
// the merge request is mergeable. Priority: source branch problems, linked
// group, approvals (review_required), the merge_blocked kinds, divergence.
func (ev *evaluation) mergeError() error {
	if ev.sourceErr != nil {
		return ev.sourceErr
	}
	if ev.mr.State != domain.MRStateOpen {
		return requireOpen(ev.mr)
	}
	if bs := ev.blockers(BlockerLinkedGroup); len(bs) > 0 {
		return &BlockedError{Code: CodeLinkedGroup, Message: bs[0].Message, Blockers: bs}
	}
	if bs := ev.blockers(BlockerApprovals); len(bs) > 0 {
		return &domain.PushDenied{Kind: "review_required", Reason: fmt.Sprintf("%d of %d approvals",
			ev.checks.Approvals.Current, ev.checks.Approvals.Required)}
	}
	if bs := ev.blockers(BlockerChangesRequested, BlockerCI, BlockerTodos, BlockerLinear); len(bs) > 0 {
		msgs := make([]string, len(bs))
		for i, b := range bs {
			msgs[i] = b.Message
		}
		return &BlockedError{
			Code:     CodeMergeBlocked,
			Message:  fmt.Sprintf("merge request !%d cannot be merged: %s", ev.mr.Number, strings.Join(msgs, "; ")),
			Blockers: bs,
		}
	}
	return ev.divergedErr
}

// Checks reports the merge readiness of merge request number. It requires
// domain.ActTreeRead.
func (s *Service) Checks(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*MergeChecks, error) {
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
	ev, err := s.evaluate(ctx, tree, mr, false)
	if err != nil {
		return nil, err
	}
	return &ev.checks, nil
}

// evaluate computes the merge readiness of mr. inGroup skips the linked
// group blocker (used when the group itself is merged). Errors are
// operational failures only; policy problems become blockers.
func (s *Service) evaluate(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest, inGroup bool) (*evaluation, error) {
	eff, err := s.ProtectionFor(ctx, tree.ID, mr.TargetBranch)
	if err != nil {
		return nil, err
	}
	ev := &evaluation{mr: mr, eff: eff}
	ev.checks = MergeChecks{
		Blockers:  []Blocker{},
		Approvals: ApprovalSummary{Required: eff.RequiredApprovals, ChangesRequested: []string{}},
		CI:        CISummary{Required: slices.Clone(eff.RequiredChecks), Missing: []string{}},
	}
	if !eff.RequireCIPass {
		ev.checks.CI.Required = []string{}
	}

	if mr.State != domain.MRStateOpen {
		ev.block(BlockerState, "merge request !%d is %s", mr.Number, mr.State)
	}

	// Source and target tips.
	ev.sourceTip, err = s.branchTip(ctx, tree.ID, mr.SourceBranch)
	if err != nil {
		if !errors.Is(err, domain.ErrNotFound) && !errors.Is(err, domain.ErrInvalid) {
			return nil, err
		}
		ev.sourceErr = fmt.Errorf("merge request !%d: %w", mr.Number, err)
		ev.block(BlockerSource, "%v", err)
	}
	switch tb, err := s.store.GetBranch(ctx, tree.ID, mr.TargetBranch); {
	case err == nil:
		ev.targetTip = tb.Tip
	case isNotFound(err):
		// The target was deleted after the merge request was opened; a
		// merge recreates it at the source tip.
	default:
		return nil, fmt.Errorf("review: get branch '%s': %w", mr.TargetBranch, err)
	}

	// Linked group.
	if !inGroup && mr.LinkedGroup != "" && mr.State == domain.MRStateOpen {
		if g, err := s.groupOfMR(ctx, tree, mr); err != nil {
			return nil, err
		} else if g != nil && g.State == domain.MRStateOpen {
			ev.block(BlockerLinkedGroup, "merge request !%d belongs to linked group %q (%s); merge the group instead",
				mr.Number, g.Name, g.ID)
		}
	}

	// Approvals and change requests.
	cur, stale := tallyApprovals(mr, ev.sourceTip)
	ev.checks.Approvals.Current, ev.checks.Approvals.Stale = cur, stale
	if cur < eff.RequiredApprovals {
		msg := fmt.Sprintf("%d of %d approvals", cur, eff.RequiredApprovals)
		if stale > 0 {
			msg += fmt.Sprintf(" (%d stale: the source changed after approval)", stale)
		}
		ev.block(BlockerApprovals, "%s", msg)
	}
	for _, a := range mr.Approvals {
		if a.Decision == domain.ReviewChangesRequested {
			ev.checks.Approvals.ChangesRequested = append(ev.checks.Approvals.ChangesRequested, a.Username)
		}
	}
	if n := len(ev.checks.Approvals.ChangesRequested); n > 0 {
		ev.block(BlockerChangesRequested, "changes requested by %s", strings.Join(ev.checks.Approvals.ChangesRequested, ", "))
	}

	if ev.sourceTip != "" {
		if err := s.evaluateCI(ctx, tree, ev); err != nil {
			return nil, err
		}
	}
	if err := s.evaluateTodos(ctx, tree, ev); err != nil {
		return nil, err
	}

	if ev.sourceTip != "" {
		// Linear history: the snapshots the merge brings must not be merges.
		if eff.RequireLinear {
			var haves []string
			if ev.targetTip != "" {
				haves = []string{ev.targetTip}
			}
			incoming, err := s.hist.Missing(ctx, ev.sourceTip, haves)
			if err != nil {
				return nil, fmt.Errorf("review: list incoming snapshots: %w", err)
			}
			for _, snap := range incoming {
				if len(snap.Parents) > 1 {
					ev.block(BlockerLinear, "branch '%s' requires linear history but snapshot %s is a merge snapshot",
						mr.TargetBranch, shortID(snap.ID))
					break
				}
			}
		}
		// Fast-forward classification.
		up, err := s.classifyMerge(ctx, mr, ev.sourceTip, ev.targetTip)
		switch {
		case errors.Is(err, domain.ErrConflict):
			ev.divergedErr = err
			ev.block(BlockerDiverged, "target '%s' has diverged from '%s'; merge the target into the source and push",
				mr.TargetBranch, mr.SourceBranch)
		case err != nil:
			return nil, err
		default:
			ev.upToDate = up
		}
	}

	ev.checks.Mergeable = len(ev.checks.Blockers) == 0
	return ev, nil
}

// evaluateCI fills the CI summary and blocker.
func (s *Service) evaluateCI(ctx context.Context, tree *domain.Tree, ev *evaluation) error {
	checks, err := ci.List(ctx, s.store, tree.ID, ev.sourceTip)
	if err != nil {
		return err
	}
	ev.checks.CI.State = ci.Combine(checks)
	if !ev.eff.RequireCIPass {
		return nil
	}
	states := make(map[string]string, len(checks))
	for _, c := range checks {
		states[c.Context] = c.State
	}
	var failing []string
	if len(ev.eff.RequiredChecks) == 0 {
		if len(checks) == 0 {
			ev.block(BlockerCI, "no CI checks have been reported for the source tip %s", shortID(ev.sourceTip))
			return nil
		}
		for _, c := range checks {
			if c.State != domain.CISuccess {
				failing = append(failing, c.Context+" is "+c.State)
			}
		}
	} else {
		for _, name := range ev.eff.RequiredChecks {
			st, ok := states[name]
			switch {
			case !ok:
				ev.checks.CI.Missing = append(ev.checks.CI.Missing, name)
			case st != domain.CISuccess:
				failing = append(failing, name+" is "+st)
			}
		}
	}
	if len(ev.checks.CI.Missing) > 0 {
		failing = append(failing, "missing "+strings.Join(ev.checks.CI.Missing, ", "))
	}
	if len(failing) > 0 {
		ev.block(BlockerCI, "required CI checks have not passed: %s", strings.Join(failing, "; "))
	}
	return nil
}

// evaluateTodos counts open blocking TODOs of the source branch (doc
// collection `todos`) and blocks when the target requires it.
func (s *Service) evaluateTodos(ctx context.Context, tree *domain.Tree, ev *evaluation) error {
	todos, err := meta.ListDocsT[domain.Todo](ctx, s.store, TodoCollection, meta.DocPrefix(tree.ID))
	if err != nil {
		return fmt.Errorf("review: list todos: %w", err)
	}
	var numbers []string
	for _, t := range todos {
		if !t.Blocking || (t.Status != domain.TodoOpen && t.Status != domain.TodoInProgress) {
			continue
		}
		branch := t.TargetBranch
		if branch == "" {
			branch = tree.DefaultBranch
		}
		if branch != ev.mr.SourceBranch {
			continue
		}
		numbers = append(numbers, fmt.Sprintf("#%d", t.Number))
	}
	ev.checks.Todos.BlockingOpen = len(numbers)
	if ev.eff.BlockOnTodos && len(numbers) > 0 {
		ev.block(BlockerTodos, "%d open blocking TODO(s) on '%s': %s", len(numbers), ev.mr.SourceBranch, strings.Join(numbers, ", "))
	}
	return nil
}

func shortID(id string) string {
	if len(id) > 12 {
		return id[:12]
	}
	return id
}
