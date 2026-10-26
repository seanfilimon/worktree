package httpapi

import (
	"context"
	"errors"
	"net/http"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// v0.5 review features (docs/ENGINEERING_PLAN.md §13.4): review decisions,
// merge request comments, merge readiness, linked groups and tag
// protections. They are served by the review service behind Deps.Review
// when it implements reviewer (internal/review.Service does); otherwise
// the endpoints answer 501 not_implemented.

// reviewer is the v0.5 surface of internal/review.Service.
type reviewer interface {
	domain.ReviewService
	Review(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, decision, body string) (*domain.MergeRequest, error)
	Checks(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*review.MergeChecks, error)

	Comments(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) ([]domain.ReviewComment, error)
	AddComment(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, in review.CommentInput) (*domain.ReviewComment, error)
	UpdateComment(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, id string, patch review.CommentPatch) (*domain.ReviewComment, error)
	DeleteComment(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, id string) error

	CreateGroup(ctx context.Context, p *domain.Principal, tenantID, name string, members []review.GroupMember) (*domain.LinkedGroup, error)
	GetGroup(ctx context.Context, tenantID, id string) (*domain.LinkedGroup, error)
	ListGroups(ctx context.Context, tenantID string) ([]domain.LinkedGroup, error)
	MergeGroup(ctx context.Context, p *domain.Principal, tenantID, id string) (*domain.LinkedGroup, error)
	CloseGroup(ctx context.Context, p *domain.Principal, tenantID, id string) (*domain.LinkedGroup, error)

	TagProtections(ctx context.Context, treeID string) ([]domain.TagProtection, error)
	PutTagProtection(ctx context.Context, p *domain.Principal, tree *domain.Tree, tp domain.TagProtection) (*domain.TagProtection, error)
	DeleteTagProtection(ctx context.Context, p *domain.Principal, tree *domain.Tree, pattern string) error
	CheckTag(ctx context.Context, p *domain.Principal, tree *domain.Tree, name string, op review.TagOp) error
}

var _ reviewer = (*review.Service)(nil)

// reviewSvc returns Deps.Review as a reviewer, or a 501 error.
func (a *api) reviewSvc() (reviewer, error) {
	if r, ok := a.d.Review.(reviewer); ok {
		return r, nil
	}
	return nil, &apiError{status: http.StatusNotImplemented, code: "not_implemented",
		msg: "reviews, comments, linked groups and tag protections are not available on this server"}
}

// reviewError maps a *review.BlockedError onto 409 with its code
// (merge_blocked / linked_group); other errors pass through.
func reviewError(err error) error {
	var be *review.BlockedError
	if errors.As(err, &be) {
		return &apiError{status: http.StatusConflict, code: be.Code, msg: be.Message}
	}
	// v0.6 (§14.1): a merge method refused by branch protection.
	var me *review.MethodError
	if errors.As(err, &me) {
		return &apiError{status: http.StatusConflict, code: review.CodeMergeMethodNotAllowed, msg: me.Message}
	}
	// v0.7 (§15.7): drafts answer 409 draft (a BlockedError above); the
	// tenant storage quota 409 quota_exceeded.
	return deepReviewError(err)
}

// registerReviewRoutes registers the v0.5 routes for review decisions,
// comments, merge readiness, linked groups and tag protections (§13.4).
func (a *api) registerReviewRoutes(tree string) {
	mr := tree + "/merge-requests/{n}"
	a.handle("POST "+mr+"/review", a.handleReviewMR)
	a.handle("GET "+mr+"/checks", a.handleMRChecks)
	a.handle("GET "+mr+"/mergeability", a.handleMergeability) // v0.6 (§14.1)
	a.handle("GET "+mr+"/comments", a.handleListComments)
	a.handle("POST "+mr+"/comments", a.handleAddComment)
	a.handle("PATCH "+mr+"/comments/{id}", a.handleUpdateComment)
	a.handle("DELETE "+mr+"/comments/{id}", a.handleDeleteComment)

	const groups = "/api/v1/tenants/{tenant}/linked-groups"
	a.handle("GET "+groups, a.handleListGroups)
	a.handle("POST "+groups, a.handleCreateGroup)
	a.handle("GET "+groups+"/{id}", a.handleGetGroup)
	a.handle("POST "+groups+"/{id}/merge", a.handleMergeGroup)
	a.handle("DELETE "+groups+"/{id}", a.handleCloseGroup)

	a.handle("GET "+tree+"/tag-protections", a.handleListTagProtections)
	a.handle("PUT "+tree+"/tag-protections", a.handlePutTagProtection)
	a.handle("DELETE "+tree+"/tag-protections", a.handleDeleteTagProtection)
}

// mrTarget resolves the reviewer, the caller-readable tree and the MR
// number of a .../merge-requests/{n}/... request.
func (a *api) mrTarget(r *http.Request) (reviewer, *domain.Tree, int, error) {
	rv, err := a.reviewSvc()
	if err != nil {
		return nil, nil, 0, err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return nil, nil, 0, err
	}
	n, err := mrNumber(r)
	if err != nil {
		return nil, nil, 0, err
	}
	return rv, tree, n, nil
}

type reviewRequest struct {
	Decision string `json:"decision"`
	Body     string `json:"body"`
}

// handleReviewMR answers POST {tree}/merge-requests/{n}/review
// {"decision","body"}.
func (a *api) handleReviewMR(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, n, err := a.mrTarget(r)
	if err != nil {
		return err
	}
	var req reviewRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	switch req.Decision {
	case domain.ReviewApproved, domain.ReviewChangesRequested, domain.ReviewCommented:
	default:
		return badRequestf("decision must be one of %q, %q, %q",
			domain.ReviewApproved, domain.ReviewChangesRequested, domain.ReviewCommented)
	}
	if err := validateText("body", req.Body, review.MaxBodyLen, req.Decision == domain.ReviewCommented); err != nil {
		return err
	}
	mr, err := rv.Review(r.Context(), p, tree, n, req.Decision, req.Body)
	if err != nil {
		return err
	}
	normalizeMR(mr)
	writeJSON(w, http.StatusOK, mr)
	return nil
}

// handleMRChecks answers GET {tree}/merge-requests/{n}/checks.
func (a *api) handleMRChecks(w http.ResponseWriter, r *http.Request) error {
	rv, tree, n, err := a.mrTarget(r)
	if err != nil {
		return err
	}
	c, err := rv.Checks(r.Context(), principal(r), tree, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, c)
	return nil
}

// ---- comments ----

// handleListComments answers GET {tree}/merge-requests/{n}/comments.
func (a *api) handleListComments(w http.ResponseWriter, r *http.Request) error {
	rv, tree, n, err := a.mrTarget(r)
	if err != nil {
		return err
	}
	cs, err := rv.Comments(r.Context(), principal(r), tree, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(cs))
	return nil
}

// handleAddComment answers POST {tree}/merge-requests/{n}/comments
// {"body","path?","line?","snapshot_id?"}.
func (a *api) handleAddComment(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, n, err := a.mrTarget(r)
	if err != nil {
		return err
	}
	var in review.CommentInput
	if err := decodeJSON(w, r, &in); err != nil {
		return err
	}
	// v0.7 (§15.7): a suggestion comment may have an empty body.
	if err := validateText("body", in.Body, review.MaxBodyLen, in.Suggestion == nil); err != nil {
		return err
	}
	if in.Suggestion != nil {
		if err := validateText("suggestion.replacement", in.Suggestion.Replacement, review.MaxSuggestionReplacement, false); err != nil {
			return err
		}
		if in.Suggestion.StartLine < 1 || in.Suggestion.EndLine < in.Suggestion.StartLine {
			return badRequestf("suggestion lines must satisfy 1 <= start_line <= end_line")
		}
	}
	if err := validateText("path", in.Path, review.MaxCommentPathLen, false); err != nil {
		return err
	}
	if in.Line < 0 {
		return badRequestf("line must not be negative")
	}
	c, err := rv.AddComment(r.Context(), p, tree, n, in)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusCreated, c)
	return nil
}

// handleUpdateComment answers PATCH {tree}/merge-requests/{n}/comments/{id}
// {"body?","resolved?"}.
func (a *api) handleUpdateComment(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, n, err := a.mrTarget(r)
	if err != nil {
		return err
	}
	var patch review.CommentPatch
	if err := decodeJSON(w, r, &patch); err != nil {
		return err
	}
	if patch.Body == nil && patch.Resolved == nil {
		return badRequestf("at least one of body, resolved is required")
	}
	if patch.Body != nil {
		if err := validateText("body", *patch.Body, review.MaxBodyLen, true); err != nil {
			return err
		}
	}
	c, err := rv.UpdateComment(r.Context(), p, tree, n, r.PathValue("id"), patch)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, c)
	return nil
}

// handleDeleteComment answers DELETE {tree}/merge-requests/{n}/comments/{id}.
func (a *api) handleDeleteComment(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, tree, n, err := a.mrTarget(r)
	if err != nil {
		return err
	}
	id := r.PathValue("id")
	if err := rv.DeleteComment(r.Context(), p, tree, n, id); err != nil {
		return err
	}
	a.audit(r, "mr.comment_delete", tree.TenantID, "merge_request", tree.ID, map[string]string{
		"tree": tree.Name, "number": r.PathValue("n"), "comment_id": id,
	})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// ---- linked groups ----

type groupMemberRequest struct {
	Tree   string `json:"tree"` // "tenant/tree"
	Number int    `json:"number"`
}

type createGroupRequest struct {
	Name    string               `json:"name"`
	Members []groupMemberRequest `json:"members"`
}

// groupMemberView is a linked group member enriched with its tree and
// merge request (when the caller may read the tree).
type groupMemberView struct {
	TreeID   string `json:"tree_id"`
	MRNumber int    `json:"mr_number"`
	Tree     string `json:"tree,omitempty"` // "tenant/tree"
	Title    string `json:"title,omitempty"`
	State    string `json:"state,omitempty"`
}

type linkedGroupView struct {
	domain.LinkedGroup
	Members []groupMemberView `json:"members"`
}

// groupView enriches g for the caller of r.
func (a *api) groupView(r *http.Request, g *domain.LinkedGroup) linkedGroupView {
	ctx := r.Context()
	v := linkedGroupView{LinkedGroup: *g, Members: []groupMemberView{}}
	slugs := map[string]string{}
	for _, m := range g.Members {
		mv := groupMemberView{TreeID: m.TreeID, MRNumber: m.MRNumber}
		if tree, err := a.d.Store.GetTree(ctx, m.TreeID); err == nil &&
			a.d.Authz.Authorize(ctx, principal(r), domain.ActTreeRead, tree.TenantID, tree) == nil {
			slug, ok := slugs[tree.TenantID]
			if !ok {
				if t, err := a.d.Store.GetTenant(ctx, tree.TenantID); err == nil {
					slug = t.Slug
				}
				slugs[tree.TenantID] = slug
			}
			mv.Tree = slug + "/" + tree.Name
			if mr, err := a.d.Store.GetMergeRequest(ctx, tree.ID, m.MRNumber); err == nil {
				mv.Title, mv.State = mr.Title, mr.State
			}
		}
		v.Members = append(v.Members, mv)
	}
	return v
}

// handleListGroups answers GET /tenants/{tenant}/linked-groups.
func (a *api) handleListGroups(w http.ResponseWriter, r *http.Request) error {
	rv, err := a.reviewSvc()
	if err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return err
	}
	gs, err := rv.ListGroups(r.Context(), t.ID)
	if err != nil {
		return err
	}
	out := make([]linkedGroupView, 0, len(gs))
	for i := range gs {
		out = append(out, a.groupView(r, &gs[i]))
	}
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleCreateGroup answers POST /tenants/{tenant}/linked-groups
// {"name","members":[{"tree":"tenant/tree","number"}]}.
func (a *api) handleCreateGroup(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, err := a.reviewSvc()
	if err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return err
	}
	var req createGroupRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Name = strings.TrimSpace(req.Name)
	if err := validateText("name", req.Name, review.MaxGroupName, true); err != nil {
		return err
	}
	if len(req.Members) < 2 || len(req.Members) > review.MaxGroupMembers {
		return badRequestf("members must list 2-%d merge requests", review.MaxGroupMembers)
	}
	ctx := r.Context()
	members := make([]review.GroupMember, 0, len(req.Members))
	for _, m := range req.Members {
		slug, name, ok := strings.Cut(m.Tree, "/")
		if !ok || slug == "" || name == "" || strings.Contains(name, "/") {
			return badRequestf("member tree %q must be of the form tenant/tree", m.Tree)
		}
		if m.Number < 1 {
			return badRequestf("member number must be a positive integer")
		}
		mt, err := a.tenantByRef(ctx, slug)
		if err != nil {
			return err
		}
		tree, err := a.d.Store.GetTreeByName(ctx, mt.ID, name)
		if err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				return notFoundf("tree %s not found", m.Tree)
			}
			return err
		}
		if err := a.d.Authz.Authorize(ctx, p, domain.ActTreeRead, mt.ID, tree); err != nil {
			if isDenied(err) {
				return notFoundf("tree %s not found", m.Tree)
			}
			return err
		}
		members = append(members, review.GroupMember{Tree: tree, Number: m.Number})
	}
	g, err := rv.CreateGroup(ctx, p, t.ID, req.Name, members)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusCreated, a.groupView(r, g))
	return nil
}

// groupTarget resolves the reviewer, tenant and group of a
// /linked-groups/{id} request (tenant:read required).
func (a *api) groupTarget(r *http.Request) (reviewer, *domain.Tenant, *domain.LinkedGroup, error) {
	rv, err := a.reviewSvc()
	if err != nil {
		return nil, nil, nil, err
	}
	t, err := a.tenantFor(r, domain.ActTenantRead)
	if err != nil {
		return nil, nil, nil, err
	}
	g, err := rv.GetGroup(r.Context(), t.ID, r.PathValue("id"))
	if err != nil {
		return nil, nil, nil, err
	}
	return rv, t, g, nil
}

// handleGetGroup answers GET /tenants/{tenant}/linked-groups/{id}.
func (a *api) handleGetGroup(w http.ResponseWriter, r *http.Request) error {
	_, _, g, err := a.groupTarget(r)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, a.groupView(r, g))
	return nil
}

// handleMergeGroup answers POST /tenants/{tenant}/linked-groups/{id}/merge
// with an optional body {"method"} (v0.6, §14.1).
func (a *api) handleMergeGroup(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, t, g, err := a.groupTarget(r)
	if err != nil {
		return err
	}
	var req struct {
		Method string `json:"method"`
	}
	if err := decodeOptionalJSON(w, r, &req); err != nil {
		return err
	}
	if err := (&mergeMRRequest{Method: req.Method}).validate(); err != nil {
		return err
	}
	var merged *domain.LinkedGroup
	if m, ok := rv.(merger); ok {
		merged, err = m.MergeGroupWith(r.Context(), p, t.ID, g.ID, strings.TrimSpace(req.Method))
	} else {
		merged, err = rv.MergeGroup(r.Context(), p, t.ID, g.ID)
	}
	if writeMergeConflict(w, err) {
		return nil
	}
	if err != nil {
		return reviewError(err)
	}
	writeJSON(w, http.StatusOK, a.groupView(r, merged))
	return nil
}

// handleCloseGroup answers DELETE /tenants/{tenant}/linked-groups/{id}: the
// group is closed (kept for history) and its members unlinked.
func (a *api) handleCloseGroup(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	rv, t, g, err := a.groupTarget(r)
	if err != nil {
		return err
	}
	closed, err := rv.CloseGroup(r.Context(), p, t.ID, g.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, a.groupView(r, closed))
	return nil
}

// ---- tag protections ----

// handleListTagProtections answers GET {tree}/tag-protections.
func (a *api) handleListTagProtections(w http.ResponseWriter, r *http.Request) error {
	rv, err := a.reviewSvc()
	if err != nil {
		return err
	}
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	tps, err := rv.TagProtections(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(tps))
	return nil
}

// handlePutTagProtection answers PUT {tree}/tag-protections (body =
// TagProtection).
func (a *api) handlePutTagProtection(w http.ResponseWriter, r *http.Request) error {
	rv, err := a.reviewSvc()
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var tp domain.TagProtection
	if err := decodeJSON(w, r, &tp); err != nil {
		return err
	}
	if err := validateGlob("pattern", tp.Pattern); err != nil {
		return err
	}
	for _, u := range tp.AllowedTaggers {
		if strings.TrimSpace(u) == "" || len(u) > 100 {
			return badRequestf("allowed_taggers entries must be non-empty usernames")
		}
	}
	out, err := rv.PutTagProtection(r.Context(), principal(r), tree, tp)
	if err != nil {
		return err
	}
	a.audit(r, "tag_protection.update", t.ID, "tree", tree.ID, map[string]string{"pattern": out.Pattern})
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleDeleteTagProtection answers DELETE {tree}/tag-protections?pattern=.
func (a *api) handleDeleteTagProtection(w http.ResponseWriter, r *http.Request) error {
	rv, err := a.reviewSvc()
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	pattern := r.URL.Query().Get("pattern")
	if pattern == "" {
		return badRequestf("query parameter pattern is required")
	}
	if err := rv.DeleteTagProtection(r.Context(), principal(r), tree, pattern); err != nil {
		return err
	}
	a.audit(r, "tag_protection.delete", t.ID, "tree", tree.ID, map[string]string{"pattern": pattern})
	w.WriteHeader(http.StatusNoContent)
	return nil
}
