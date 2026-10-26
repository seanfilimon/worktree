package httpapi

import (
	"context"
	"errors"
	"net/http"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// v0.7 deeper review (docs/ENGINEERING_PLAN.md §15.7,
// specs/server/ReviewWorkflow.md): path owners, review requests, draft
// merge requests and suggested changes. Served by internal/review.Service
// behind Deps.Review; other implementations answer 501.

// deepReviewer is the v0.7 surface of internal/review.Service.
type deepReviewer interface {
	OpenWith(ctx context.Context, p *domain.Principal, tree *domain.Tree, in review.OpenInput) (*domain.MergeRequest, error)
	Ready(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*domain.MergeRequest, error)
	RequestReviewers(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, refs []string) (*domain.MergeRequest, error)
	RemoveReviewers(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, refs []string) (*domain.MergeRequest, error)
	Owners(ctx context.Context, p *domain.Principal, tree *domain.Tree) (*review.OwnersConfig, error)
	PutOwners(ctx context.Context, p *domain.Principal, tree *domain.Tree, rules []domain.PathOwnerRule) (*review.OwnersConfig, error)
	MROwners(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*review.MROwners, error)
	ApplySuggestion(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, id, message string) (*review.AppliedSuggestion, error)
}

var _ deepReviewer = (*review.Service)(nil)

// deepReviewSvc returns Deps.Review as a deepReviewer, or a 501 error.
func (a *api) deepReviewSvc() (deepReviewer, error) {
	if r, ok := a.d.Review.(deepReviewer); ok {
		return r, nil
	}
	return nil, &apiError{status: http.StatusNotImplemented, code: "not_implemented",
		msg: "path owners, review requests, drafts and suggestions are not available on this server"}
}

// registerOwnerRoutes registers the v0.7 routes for path owners, review
// requests, drafts and suggestions (§15.7).
func (a *api) registerOwnerRoutes(tree string) {
	mr := tree + "/merge-requests/{n}"
	a.handle("GET "+tree+"/owners", a.handleGetOwners)
	a.handle("PUT "+tree+"/owners", a.handlePutOwners)
	a.handle("GET "+mr+"/owners", a.handleMROwners)
	a.handle("POST "+mr+"/ready", a.handleMRReady)
	a.handle("POST "+mr+"/reviewers", a.handleReviewers(true))
	a.handle("DELETE "+mr+"/reviewers", a.handleReviewers(false))
	a.handle("POST "+mr+"/comments/{id}/apply", a.handleApplySuggestion)
}

// deepTarget resolves the v0.7 service, the caller-readable tree and the
// merge request number ({n} = 0 when the route has none).
func (a *api) deepTarget(r *http.Request, withNumber bool) (deepReviewer, *domain.Tree, int, error) {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return nil, nil, 0, err
	}
	rv, err := a.deepReviewSvc()
	if err != nil {
		return nil, nil, 0, err
	}
	n := 0
	if withNumber {
		if n, err = mrNumber(r); err != nil {
			return nil, nil, 0, err
		}
	}
	return rv, tree, n, nil
}

// deepReviewError maps the v0.7 errors onto 409 with their codes.
func deepReviewError(err error) error {
	var oe *review.OutdatedError
	if errors.As(err, &oe) {
		return &apiError{status: http.StatusConflict, code: review.CodeSuggestionOutdated, msg: oe.Message}
	}
	var qe *review.QuotaError
	if errors.As(err, &qe) {
		return &apiError{status: http.StatusConflict, code: review.CodeQuotaExceeded, msg: qe.Message}
	}
	return err
}

// handleGetOwners answers GET {tree}/owners.
func (a *api) handleGetOwners(w http.ResponseWriter, r *http.Request) error {
	rv, tree, _, err := a.deepTarget(r, false)
	if err != nil {
		return err
	}
	cfg, err := rv.Owners(r.Context(), principal(r), tree)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, cfg)
	return nil
}

// putOwnersRequest is the body of PUT {tree}/owners.
type putOwnersRequest struct {
	Rules []domain.PathOwnerRule `json:"rules"`
}

// handlePutOwners answers PUT {tree}/owners {"rules"} (tree:admin).
func (a *api) handlePutOwners(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, _, err := a.deepTarget(r, false)
	if err != nil {
		return err
	}
	var req putOwnersRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if len(req.Rules) > review.MaxOwnerRules {
		return badRequestf("at most %d rules are allowed", review.MaxOwnerRules)
	}
	for _, rule := range req.Rules {
		if err := validateText("pattern", rule.Pattern, review.MaxOwnerPattern, true); err != nil {
			return err
		}
	}
	cfg, err := rv.PutOwners(r.Context(), p, tree, req.Rules)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, cfg)
	return nil
}

// handleMROwners answers GET {tree}/merge-requests/{n}/owners.
func (a *api) handleMROwners(w http.ResponseWriter, r *http.Request) error {
	rv, tree, n, err := a.deepTarget(r, true)
	if err != nil {
		return err
	}
	res, err := rv.MROwners(r.Context(), principal(r), tree, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// handleMRReady answers POST {tree}/merge-requests/{n}/ready.
func (a *api) handleMRReady(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, n, err := a.deepTarget(r, true)
	if err != nil {
		return err
	}
	mr, err := rv.Ready(r.Context(), p, tree, n)
	if err != nil {
		return err
	}
	normalizeMR(mr)
	writeJSON(w, http.StatusOK, mr)
	return nil
}

// reviewersRequest is the body of POST/DELETE .../reviewers.
type reviewersRequest struct {
	Reviewers []string `json:"reviewers"`
}

// handleReviewers answers POST (add) and DELETE (remove)
// {tree}/merge-requests/{n}/reviewers {"reviewers"}; DELETE also accepts
// ?reviewer= (repeatable) instead of a body.
func (a *api) handleReviewers(add bool) handlerFunc {
	return func(w http.ResponseWriter, r *http.Request) error {
		p, err := requireLogin(r)
		if err != nil {
			return err
		}
		rv, tree, n, err := a.deepTarget(r, true)
		if err != nil {
			return err
		}
		var req reviewersRequest
		if add {
			err = decodeJSON(w, r, &req)
		} else {
			err = decodeOptionalJSON(w, r, &req)
			req.Reviewers = append(req.Reviewers, r.URL.Query()["reviewer"]...)
		}
		if err != nil {
			return err
		}
		if len(req.Reviewers) == 0 {
			return badRequestf("reviewers must name at least one user:NAME or team:SLUG")
		}
		if len(req.Reviewers) > review.MaxReviewers {
			return badRequestf("at most %d reviewers per request", review.MaxReviewers)
		}
		var mr *domain.MergeRequest
		if add {
			mr, err = rv.RequestReviewers(r.Context(), p, tree, n, req.Reviewers)
		} else {
			mr, err = rv.RemoveReviewers(r.Context(), p, tree, n, req.Reviewers)
		}
		if err != nil {
			return err
		}
		normalizeMR(mr)
		writeJSON(w, http.StatusOK, mr)
		return nil
	}
}

// applySuggestionRequest is the optional body of POST .../comments/{id}/apply.
type applySuggestionRequest struct {
	Message string `json:"message"`
}

// handleApplySuggestion answers POST {tree}/merge-requests/{n}/comments/{id}/apply.
func (a *api) handleApplySuggestion(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, n, err := a.deepTarget(r, true)
	if err != nil {
		return err
	}
	var req applySuggestionRequest
	if err := decodeOptionalJSON(w, r, &req); err != nil {
		return err
	}
	if err := validateText("message", req.Message, review.MaxMergeMessage, false); err != nil {
		return err
	}
	res, err := rv.ApplySuggestion(r.Context(), p, tree, n, r.PathValue("id"), req.Message)
	if err != nil {
		return deepReviewError(err)
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}
