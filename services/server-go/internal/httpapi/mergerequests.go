package httpapi

import (
	"errors"
	"net/http"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
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
}

func normalizeMR(mr *domain.MergeRequest) { mr.Approvals = nonNil(mr.Approvals) }

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
	if req.SourceBranch == req.TargetBranch {
		return badRequestf("source_branch and target_branch must differ")
	}
	mr, err := a.d.Review.Open(r.Context(), p, tree, req.Title, req.Description, req.SourceBranch, req.TargetBranch)
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
			mr, err = a.d.Review.Merge(ctx, p, tree, n)
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
