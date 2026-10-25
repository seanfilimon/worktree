package httpapi

import (
	"errors"
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/ci"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// registerCIRoutes registers the v0.5 routes for CI status reporting and
// queries (§13.4). Without Deps.CI they answer 501 not_implemented.
func (a *api) registerCIRoutes(tree string) {
	a.handle("POST "+tree+"/ci/status", a.handleReportCI)
	a.handle("GET "+tree+"/ci/status", a.handleCIStatus)
}

// maxRequiredChecks bounds BranchProtection.RequiredChecks.
const maxRequiredChecks = 50

// validateRequiredChecks validates and normalizes (trims, deduplicates,
// keeps order) the CI contexts of a branch protection rule.
func validateRequiredChecks(p *domain.BranchProtection) error {
	if len(p.RequiredChecks) > maxRequiredChecks {
		return badRequestf("required_checks lists at most %d contexts", maxRequiredChecks)
	}
	var out []string
	seen := map[string]bool{}
	for _, c := range p.RequiredChecks {
		if ci.ValidateContext(c) != nil {
			return badRequestf("required_checks entries must be 1-%d bytes of printable UTF-8", ci.MaxContextLen)
		}
		c = strings.TrimSpace(c)
		if !seen[c] {
			seen[c] = true
			out = append(out, c)
		}
	}
	p.RequiredChecks = out
	return nil
}

func (a *api) ciSvc() (*ci.Service, error) {
	if a.d.CI == nil {
		return nil, &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "CI status reporting is not available on this server"}
	}
	return a.d.CI, nil
}

// handleReportCI answers POST {tree}/ci/status {"snapshot","context",
// "state","description?","target_url?"} (requires ci:report) with 201 and
// the stored CIStatus.
func (a *api) handleReportCI(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.ciSvc()
	if err != nil {
		return err
	}
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActCIReport)
	if err != nil {
		return err
	}
	var req ci.Report
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if req.Snapshot == "" {
		return badRequestf("snapshot is required")
	}
	if !ci.ValidState(req.State) {
		return badRequestf("state must be one of %q, %q, %q, %q", domain.CIPending, domain.CISuccess, domain.CIFailure, domain.CIError)
	}
	// A revision (branch or tag) is accepted for convenience and resolved
	// to its snapshot.
	snap, err := a.snapshotForRev(r.Context(), tree, req.Snapshot)
	if err != nil {
		return err
	}
	req.Snapshot = snap.ID
	st, err := svc.Report(r.Context(), p, tree, req)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusCreated, st)
	return nil
}

// handleCIStatus answers GET {tree}/ci/status?snapshot=ID|branch=NAME
// (default: the tree's default branch) with {"snapshot","state","checks"}.
func (a *api) handleCIStatus(w http.ResponseWriter, r *http.Request) error {
	svc, err := a.ciSvc()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	snapshot, branch := q.Get("snapshot"), q.Get("branch")
	if snapshot != "" && branch != "" {
		return badRequestf("pass at most one of snapshot, branch")
	}
	ctx := r.Context()
	if snapshot != "" {
		s, err := a.snapshotForRev(ctx, tree, snapshot)
		if err != nil {
			return err
		}
		snapshot = s.ID
	} else {
		if branch == "" {
			branch = tree.DefaultBranch
		}
		b, err := a.d.Store.GetBranch(ctx, tree.ID, branch)
		if err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				return notFoundf("branch %q not found", branch)
			}
			return err
		}
		snapshot = b.Tip
	}
	st, err := svc.Status(ctx, principal(r), tree, snapshot)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}
