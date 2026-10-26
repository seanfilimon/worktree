package httpapi

import (
	"context"
	"encoding/json"
	"errors"
	"io"
	"mime"
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// v0.8 conflict resolution in the browser (docs/ENGINEERING_PLAN.md §16.9,
// specs/server/Merging.md §5): GET {tree}/merge-requests/{n}/conflicts and
// POST {tree}/merge-requests/{n}/resolve, served by internal/review.Service
// behind Deps.Review (other implementations answer 501).

// maxResolveBody bounds the body of POST .../resolve (resolved file
// contents; each at most review.MaxResolvedFile).
const maxResolveBody = 32 << 20

// conflictResolver is the v0.8 conflict surface of internal/review.Service.
type conflictResolver interface {
	Conflicts(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*review.ConflictReport, error)
	ResolveConflicts(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, in review.ResolveInput) (*review.Resolved, error)
}

var _ conflictResolver = (*review.Service)(nil)

// registerResolveRoutes registers the v0.8 routes for conflict previews and
// browser conflict resolution (§16.9).
func (a *api) registerResolveRoutes(tree string) {
	mr := tree + "/merge-requests/{n}"
	a.handle("GET "+mr+"/conflicts", a.handleMRConflicts)
	a.handle("POST "+mr+"/resolve", a.handleMRResolve)
}

// resolveTarget resolves the service, the caller-readable tree and the
// merge request number.
func (a *api) resolveTarget(r *http.Request) (conflictResolver, *domain.Tree, int, error) {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return nil, nil, 0, err
	}
	rv, ok := a.d.Review.(conflictResolver)
	if !ok {
		return nil, nil, 0, &apiError{status: http.StatusNotImplemented, code: "not_implemented",
			msg: "conflict resolution is not available on this server"}
	}
	n, err := mrNumber(r)
	if err != nil {
		return nil, nil, 0, err
	}
	return rv, tree, n, nil
}

// handleMRConflicts answers GET {tree}/merge-requests/{n}/conflicts with a
// review.ConflictReport. Paths the caller may not read (path policies,
// license rules) are listed without their content (restricted).
func (a *api) handleMRConflicts(w http.ResponseWriter, r *http.Request) error {
	rv, tree, n, err := a.resolveTarget(r)
	if err != nil {
		return err
	}
	ctx := r.Context()
	p := principal(r)
	rep, err := rv.Conflicts(ctx, p, tree, n)
	if err != nil {
		return err
	}
	paths := make([]string, len(rep.Files))
	for i, f := range rep.Files {
		paths[i] = f.Path
	}
	rs, err := a.readRestrictions(ctx, p, tree, paths)
	if err != nil {
		return err
	}
	for i := range rep.Files {
		f := &rep.Files[i]
		if rs.check(f.Path) != nil {
			f.Restricted = true
			f.Base.Text, f.Ours.Text, f.Theirs.Text, f.Markers = "", "", "", ""
			f.Regions = []merge3.Region{}
		}
	}
	writeJSON(w, http.StatusOK, rep)
	return nil
}

// resolveRequest is the body of POST .../merge-requests/{n}/resolve.
type resolveRequest = review.ResolveInput

// handleMRResolve answers POST {tree}/merge-requests/{n}/resolve: a
// merge-resolution snapshot on the source branch (review.Resolved).
func (a *api) handleMRResolve(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, n, err := a.resolveTarget(r)
	if err != nil {
		return err
	}
	var req resolveRequest
	if err := decodeJSONLimit(w, r, &req, maxResolveBody); err != nil {
		return err
	}
	if err := validateText("message", req.Message, review.MaxMergeMessage, false); err != nil {
		return err
	}
	res, err := rv.ResolveConflicts(r.Context(), p, tree, n, req)
	if err != nil {
		return resolveError(err)
	}
	normalizeMR(res.MergeRequest)
	writeJSON(w, http.StatusOK, res)
	return nil
}

// resolveError maps the conflict resolution errors onto 409 with their
// codes (resolve_outdated, quota_exceeded); others pass through.
func resolveError(err error) error {
	var oe *review.OutdatedResolveError
	if errors.As(err, &oe) {
		return &apiError{status: http.StatusConflict, code: review.CodeResolveOutdated, msg: oe.Message}
	}
	return deepReviewError(err)
}

// decodeJSONLimit is decodeJSON with a custom body limit.
func decodeJSONLimit(w http.ResponseWriter, r *http.Request, dst any, limit int64) error {
	if ct := r.Header.Get("Content-Type"); ct != "" {
		mt, _, err := mime.ParseMediaType(ct)
		if err != nil || (mt != "application/json" && !strings.HasSuffix(mt, "+json")) {
			return &apiError{status: http.StatusUnsupportedMediaType, code: "unsupported_media_type",
				msg: "request body must be application/json"}
		}
	}
	r.Body = http.MaxBytesReader(w, r.Body, limit)
	dec := json.NewDecoder(r.Body)
	dec.DisallowUnknownFields()
	if err := dec.Decode(dst); err != nil {
		return jsonDecodeError(err)
	}
	if err := dec.Decode(&struct{}{}); !errors.Is(err, io.EOF) {
		var mbe *http.MaxBytesError
		if errors.As(err, &mbe) {
			return jsonDecodeError(err)
		}
		return badRequestf("request body must contain a single JSON value")
	}
	return nil
}
