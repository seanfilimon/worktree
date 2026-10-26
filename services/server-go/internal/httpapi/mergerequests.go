package httpapi

import (
	"context"
	"errors"
	"net/http"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// Merge-request mutations are delegated to Deps.Review, which owns their
// authorization (mr:create / mr:approve / mr:merge), events and audit
// records. The HTTP layer requires a logged-in caller who can read the tree
// so that the existence of private trees is not revealed to others.

type createMRRequest struct {
	Title        string `json:"title"`
	Description  string `json:"description"`
	SourceBranch string `json:"source_branch"`
	TargetBranch string `json:"target_branch"`

	// v0.7 (§15.7): open as a draft; request reviewers ("user:x", "team:y").
	Draft     bool     `json:"draft,omitempty"`
	Reviewers []string `json:"reviewers,omitempty"`

	// v0.8 forks (§16.2, forks.go): the tree the source branch lives in (a
	// fork in the same fork network); both or neither.
	SourceTenant string `json:"source_tenant,omitempty"`
	SourceTree   string `json:"source_tree,omitempty"`
}

func normalizeMR(mr *domain.MergeRequest) { mr.Approvals = nonNil(mr.Approvals) }

// ---- v0.6 server-side merges (§14.1, specs/server/Merging.md) ----

// merger is the v0.6 merge surface of internal/review.Service.
type merger interface {
	MergeWith(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, opts review.MergeOptions) (*domain.MergeRequest, error)
	Mergeability(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*review.Mergeability, error)
	MergeGroupWith(ctx context.Context, p *domain.Principal, tenantID, id, method string) (*domain.LinkedGroup, error)
}

var _ merger = (*review.Service)(nil)

// mergeMRRequest is the optional body of POST .../merge-requests/{n}/merge.
type mergeMRRequest struct {
	Method  string `json:"method"`
	Message string `json:"message"`
	// v0.8 (§16.9): delete the source branch after the merge.
	DeleteSourceBranch bool `json:"delete_source_branch,omitempty"`
	// v0.8 (§16.10): merge past the review gates of the target's protection
	// (needs branch:bypass_protection; audited).
	Bypass bool `json:"bypass,omitempty"`
}

func (req *mergeMRRequest) validate() error {
	req.Method = strings.TrimSpace(req.Method)
	if req.Method != "" && !review.ValidMergeMethod(req.Method) {
		return badRequestf("method must be one of %s", strings.Join(review.AllMergeMethods, ", "))
	}
	return validateText("message", req.Message, review.MaxMergeMessage, false)
}

// mergeConflictBody is the 409 merge_conflict response: the standard error
// envelope plus the conflicting paths, both at the top level and inside
// "error".
type mergeConflictBody struct {
	Error     mergeConflictDetail `json:"error"`
	Conflicts []review.Conflict   `json:"conflicts"`
}

type mergeConflictDetail struct {
	Code      string            `json:"code"`
	Message   string            `json:"message"`
	Conflicts []review.Conflict `json:"conflicts"`
}

// writeMergeConflict answers a *review.ConflictError with 409
// merge_conflict and reports whether it did.
func writeMergeConflict(w http.ResponseWriter, err error) bool {
	var ce *review.ConflictError
	if !errors.As(err, &ce) {
		return false
	}
	cs := ce.Conflicts
	if cs == nil {
		cs = []review.Conflict{}
	}
	writeJSON(w, http.StatusConflict, mergeConflictBody{
		Error:     mergeConflictDetail{Code: review.CodeMergeConflict, Message: ce.Message, Conflicts: cs},
		Conflicts: cs,
	})
	return true
}

// handleMergeability answers GET {tree}/merge-requests/{n}/mergeability →
// {"fast_forward","up_to_date","methods","default","allowed","merge_base",
// "conflicts","reasons"}.
func (a *api) handleMergeability(w http.ResponseWriter, r *http.Request) error {
	// Authorize first, like the other review routes, so 501 is never an
	// oracle for trees the caller cannot read.
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	m, ok := a.d.Review.(merger)
	if !ok {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "server-side merge methods are not available on this server"}
	}
	n, err := mrNumber(r)
	if err != nil {
		return err
	}
	res, err := m.Mergeability(r.Context(), principal(r), tree, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// validateMergeMethods validates and normalizes BranchProtection.MergeMethods
// (called by the protections PUT handler).
func validateMergeMethods(p *domain.BranchProtection) error {
	ms, err := review.NormalizeMergeMethods(p.MergeMethods)
	if err != nil {
		return badRequestf("merge_methods entries must be one of %s", strings.Join(review.AllMergeMethods, ", "))
	}
	p.MergeMethods = ms
	return nil
}

// handleListMRs answers GET /api/v1/trees/{tenant}/{tree}/merge-requests?state=.
func (a *api) handleListMRs(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	state := r.URL.Query().Get("state")
	switch state {
	case "", "all":
		state = ""
	case domain.MRStateOpen, domain.MRStateMerged, domain.MRStateClosed:
	default:
		return badRequestf("state must be one of open, merged, closed")
	}
	mrs, err := a.d.Store.ListMergeRequests(r.Context(), tree.ID, state)
	if err != nil {
		return err
	}
	mrs = nonNil(mrs)
	for i := range mrs {
		normalizeMR(&mrs[i])
	}
	writeJSON(w, http.StatusOK, mrs)
	return nil
}

// handleCreateMR answers POST /api/v1/trees/{tenant}/{tree}/merge-requests.
func (a *api) handleCreateMR(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	var req createMRRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Title = strings.TrimSpace(req.Title)
	if err := validateText("title", req.Title, 300, true); err != nil {
		return err
	}
	if err := validateText("description", req.Description, 64<<10, false); err != nil {
		return err
	}
	if err := validateBranchName(req.SourceBranch); err != nil {
		return badRequestf("source_branch: %v", err)
	}
	if err := validateBranchName(req.TargetBranch); err != nil {
		return badRequestf("target_branch: %v", err)
	}
	srcTree, err := a.mrSourceTree(r, &req) // forks (§16.2)
	if err != nil {
		return err
	}
	if req.SourceBranch == req.TargetBranch && (srcTree == nil || srcTree.ID == tree.ID) {
		return badRequestf("source_branch and target_branch must differ")
	}
	var mr *domain.MergeRequest
	if dr, ok := a.d.Review.(deepReviewer); ok { // v0.7 (§15.7)
		if len(req.Reviewers) > review.MaxReviewers {
			return badRequestf("at most %d reviewers can be requested", review.MaxReviewers)
		}
		mr, err = dr.OpenWith(r.Context(), p, tree, review.OpenInput{
			Title: req.Title, Description: req.Description, Source: req.SourceBranch, Target: req.TargetBranch,
			Draft: req.Draft, Reviewers: req.Reviewers, SourceTree: srcTree,
		})
	} else if req.Draft || len(req.Reviewers) > 0 || srcTree != nil {
		return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "draft merge requests, review requests and merge requests from forks are not available on this server"}
	} else {
		mr, err = a.d.Review.Open(r.Context(), p, tree, req.Title, req.Description, req.SourceBranch, req.TargetBranch)
	}
	if err != nil {
		return err
	}
	normalizeMR(mr)
	writeJSON(w, http.StatusCreated, mr)
	return nil
}

func mrNumber(r *http.Request) (int, error) {
	n, err := strconv.Atoi(r.PathValue("n"))
	if err != nil || n < 1 {
		return 0, badRequestf("merge request number must be a positive integer")
	}
	return n, nil
}

// handleGetMR answers GET /api/v1/trees/{tenant}/{tree}/merge-requests/{n}.
func (a *api) handleGetMR(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	n, err := mrNumber(r)
	if err != nil {
		return err
	}
	mr, err := a.d.Store.GetMergeRequest(r.Context(), tree.ID, n)
	if errors.Is(err, domain.ErrNotFound) {
		return notFoundf("merge request !%d not found", n)
	}
	if err != nil {
		return err
	}
	normalizeMR(mr)
	writeJSON(w, http.StatusOK, mr)
	return nil
}

// mrAction returns the handler for POST .../merge-requests/{n}/{action}.
func (a *api) mrAction(action string) handlerFunc {
	return func(w http.ResponseWriter, r *http.Request) error {
		p, err := requireLogin(r)
		if err != nil {
			return err
		}
		_, tree, err := a.treeFor(r, domain.ActTreeRead)
		if err != nil {
			return err
		}
		n, err := mrNumber(r)
		if err != nil {
			return err
		}
		var mr *domain.MergeRequest
		ctx := r.Context()
		switch action {
		case "approve":
			mr, err = a.d.Review.Approve(ctx, p, tree, n)
		case "merge":
			// Readiness failures (change requests, CI, TODOs, linear
			// history, linked groups) answer 409 merge_blocked /
			// linked_group; missing approvals stay 403 review_required.
			// v0.6 (§14.1): optional body {"method","message"}; conflicts
			// answer 409 merge_conflict with the conflicting paths,
			// refused methods 409 merge_method_not_allowed.
			var req mergeMRRequest
			if err := decodeOptionalJSON(w, r, &req); err != nil {
				return err
			}
			if err := req.validate(); err != nil {
				return err
			}
			if m, ok := a.d.Review.(merger); ok {
				mr, err = m.MergeWith(ctx, p, tree, n, review.MergeOptions{Method: req.Method, Message: req.Message, DeleteSourceBranch: req.DeleteSourceBranch, Bypass: req.Bypass})
			} else if (req.Method == "" || req.Method == domain.MergeFastForward) && !req.DeleteSourceBranch {
				mr, err = a.d.Review.Merge(ctx, p, tree, n)
			} else {
				return &apiError{status: http.StatusNotImplemented, code: "not_implemented",
					msg: "server-side merge methods are not available on this server"}
			}
			if writeMergeConflict(w, err) {
				return nil
			}
			err = reviewError(err)
		case "close":
			mr, err = a.d.Review.Close(ctx, p, tree, n)
		}
		if err != nil {
			return err
		}
		normalizeMR(mr)
		writeJSON(w, http.StatusOK, mr)
		return nil
	}
}
