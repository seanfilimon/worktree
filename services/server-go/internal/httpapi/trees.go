package httpapi

import (
	"context"
	"errors"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// treeSummary is the list/detail view of a tree.
type treeSummary struct {
	Tree          domain.Tree `json:"tree"`
	BranchCount   int         `json:"branch_count"`
	SnapshotCount int         `json:"snapshot_count"`
	StagedCount   int         `json:"staged_count"`
	OpenMRCount   int         `json:"open_mr_count"`
}

type createTreeRequest struct {
	Name          string `json:"name"`
	Description   string `json:"description"`
	Visibility    string `json:"visibility"`
	DefaultBranch string `json:"default_branch"`
}

type patchTreeRequest struct {
	Description   *string `json:"description"`
	Visibility    *string `json:"visibility"`
	DefaultBranch *string `json:"default_branch"`
}

const maxDescription = 4096

func (a *api) summarize(ctx context.Context, t *domain.Tree) (treeSummary, error) {
	s := treeSummary{Tree: *t}
	branches, err := a.d.Store.ListBranches(ctx, t.ID)
	if err != nil {
		return s, err
	}
	s.BranchCount = len(branches)
	if s.SnapshotCount, err = a.d.Store.CountSnapshots(ctx, t.ID); err != nil {
		return s, err
	}
	staged, err := a.d.Store.ListStaged(ctx, t.ID, "")
	if err != nil {
		return s, err
	}
	s.StagedCount = len(staged)
	mrs, err := a.d.Store.ListMergeRequests(ctx, t.ID, domain.MRStateOpen)
	if err != nil {
		return s, err
	}
	s.OpenMRCount = len(mrs)
	return s, nil
}

// handleListTrees answers GET /api/v1/tenants/{tenant}/trees with the trees
// of the tenant the caller may read.
func (a *api) handleListTrees(w http.ResponseWriter, r *http.Request) error {
	ctx := r.Context()
	t, err := a.tenantByRef(ctx, r.PathValue("tenant"))
	if err != nil {
		return err
	}
	trees, err := a.d.Store.ListTrees(ctx, t.ID)
	if err != nil {
		return err
	}
	p := principal(r)
	out := make([]treeSummary, 0, len(trees))
	for i := range trees {
		tree := &trees[i]
		if err := a.d.Authz.Authorize(ctx, p, domain.ActTreeRead, t.ID, tree); err != nil {
			if isDenied(err) {
				continue
			}
			return err
		}
		s, err := a.summarize(ctx, tree)
		if err != nil {
			return err
		}
		out = append(out, s)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Tree.Name < out[j].Tree.Name })
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleCreateTree answers POST /api/v1/tenants/{tenant}/trees.
func (a *api) handleCreateTree(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTreeCreate)
	if err != nil {
		return err
	}
	var req createTreeRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Name = strings.TrimSpace(req.Name)
	if req.Visibility == "" {
		req.Visibility = domain.VisibilityPrivate
	}
	if req.DefaultBranch == "" {
		req.DefaultBranch = "main"
	}
	if err := validateTreeName(req.Name); err != nil {
		return err
	}
	if err := validateText("description", req.Description, maxDescription, false); err != nil {
		return err
	}
	if err := validateVisibility(req.Visibility); err != nil {
		return err
	}
	if err := validateBranchName(req.DefaultBranch); err != nil {
		return err
	}
	ctx := r.Context()
	// ---- ops (§13.3): tenant MaxTrees quota (409 quota_exceeded) ----
	if err := a.checkTreeQuota(ctx, t); err != nil {
		return err
	}
	// ---- end ops ----
	now := time.Now().UTC()
	tree := &domain.Tree{
		ID:            newID(),
		TenantID:      t.ID,
		Name:          req.Name,
		Description:   req.Description,
		DefaultBranch: req.DefaultBranch,
		Visibility:    req.Visibility,
		CreatedAt:     now,
		UpdatedAt:     now,
	}
	if err := a.d.Store.CreateTree(ctx, tree); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("tree %s/%s already exists", t.Slug, req.Name)
		}
		return err
	}
	// The default branch starts empty; the first push sets its tip.
	if err := a.d.Store.PutBranch(ctx, &domain.Branch{TreeID: tree.ID, Name: tree.DefaultBranch, CreatedAt: now, UpdatedAt: now}); err != nil {
		return err
	}
	a.audit(r, "tree.create", t.ID, "tree", tree.ID, map[string]string{"name": tree.Name, "visibility": tree.Visibility})
	a.publish(r, domain.EventTreeCreated, t.ID, tree.ID, tree)
	writeJSON(w, http.StatusCreated, tree)
	return nil
}

// handleGetTree answers GET /api/v1/trees/{tenant}/{tree}.
func (a *api) handleGetTree(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	s, err := a.summarize(r.Context(), tree)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, s)
	return nil
}

// handlePatchTree answers PATCH /api/v1/trees/{tenant}/{tree}.
func (a *api) handlePatchTree(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var req patchTreeRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	changed := map[string]string{}
	if req.Description != nil {
		if err := validateText("description", *req.Description, maxDescription, false); err != nil {
			return err
		}
		tree.Description = *req.Description
		changed["description"] = "updated"
	}
	if req.Visibility != nil {
		if err := validateVisibility(*req.Visibility); err != nil {
			return err
		}
		tree.Visibility = *req.Visibility
		changed["visibility"] = *req.Visibility
	}
	if req.DefaultBranch != nil {
		if err := validateBranchName(*req.DefaultBranch); err != nil {
			return err
		}
		tree.DefaultBranch = *req.DefaultBranch
		changed["default_branch"] = *req.DefaultBranch
	}
	if len(changed) == 0 {
		return badRequestf("at least one of description, visibility, default_branch is required")
	}
	tree.UpdatedAt = time.Now().UTC()
	if err := a.d.Store.UpdateTree(r.Context(), tree); err != nil {
		return err
	}
	a.audit(r, "tree.update", t.ID, "tree", tree.ID, changed)
	writeJSON(w, http.StatusOK, tree)
	return nil
}

// handleDeleteTree answers DELETE /api/v1/trees/{tenant}/{tree}.
func (a *api) handleDeleteTree(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	if err := a.d.Store.DeleteTree(r.Context(), tree.ID); err != nil {
		return err
	}
	a.audit(r, "tree.delete", t.ID, "tree", tree.ID, map[string]string{"name": tree.Name})
	// v0.5: owners of tree-scoped documents clean up on tree.deleted (§13.14).
	a.publish(r, domain.EventTreeDeleted, t.ID, tree.ID, map[string]any{"name": tree.Name})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleListBranches answers GET /api/v1/trees/{tenant}/{tree}/branches.
func (a *api) handleListBranches(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	bs, err := a.d.Store.ListBranches(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	sort.Slice(bs, func(i, j int) bool { return bs[i].Name < bs[j].Name })
	writeJSON(w, http.StatusOK, nonNil(bs))
	return nil
}

// handleListTags answers GET /api/v1/trees/{tenant}/{tree}/tags.
func (a *api) handleListTags(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	tags, err := a.d.Store.ListTags(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	sort.Slice(tags, func(i, j int) bool { return tags[i].Name < tags[j].Name })
	writeJSON(w, http.StatusOK, nonNil(tags))
	return nil
}

// handleListStaged answers GET /api/v1/trees/{tenant}/{tree}/staged?branch=.
func (a *api) handleListStaged(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	branch := r.URL.Query().Get("branch")
	if branch != "" {
		if err := validateBranchName(branch); err != nil {
			return err
		}
	}
	staged, err := a.d.Staged.List(r.Context(), principal(r), tree, branch)
	if err != nil {
		return err
	}
	staged = nonNil(staged)
	for i := range staged {
		staged[i].FilesChanged = nonNil(staged[i].FilesChanged)
	}
	writeJSON(w, http.StatusOK, staged)
	return nil
}

// ---- branch protection ----

// handleListProtections answers GET /api/v1/trees/{tenant}/{tree}/protections.
func (a *api) handleListProtections(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	ps, err := a.d.Store.ListProtections(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	ps = nonNil(ps)
	for i := range ps {
		ps[i].AllowedPushers = nonNil(ps[i].AllowedPushers)
	}
	writeJSON(w, http.StatusOK, ps)
	return nil
}

// handlePutProtection answers PUT /api/v1/trees/{tenant}/{tree}/protections.
func (a *api) handlePutProtection(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var p domain.BranchProtection
	if err := decodeJSON(w, r, &p); err != nil {
		return err
	}
	p.TreeID = tree.ID
	if err := validateGlob("pattern", p.Pattern); err != nil {
		return err
	}
	if p.RequiredApprovals < 0 || p.RequiredApprovals > 100 {
		return badRequestf("required_approvals must be between 0 and 100")
	}
	p.AllowedPushers = nonNil(p.AllowedPushers)
	for _, u := range p.AllowedPushers {
		if u == "" || len(u) > 100 {
			return badRequestf("allowed_pushers entries must be non-empty usernames")
		}
	}
	// review-ci (§13.4): v0.5 protection fields.
	if err := validateRequiredChecks(&p); err != nil {
		return err
	}
	// server-merge (§14.1): merge_methods (validated in mergerequests.go).
	if err := validateMergeMethods(&p); err != nil {
		return err
	}
	if err := a.d.Store.PutProtection(r.Context(), &p); err != nil {
		return err
	}
	a.audit(r, "protection.update", t.ID, "tree", tree.ID, map[string]string{"pattern": p.Pattern})
	writeJSON(w, http.StatusOK, p)
	return nil
}

// handleDeleteProtection answers DELETE /api/v1/trees/{tenant}/{tree}/protections?pattern=.
func (a *api) handleDeleteProtection(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	pattern := r.URL.Query().Get("pattern")
	if pattern == "" {
		return badRequestf("query parameter pattern is required")
	}
	if err := a.d.Store.DeleteProtection(r.Context(), tree.ID, pattern); err != nil {
		return err
	}
	a.audit(r, "protection.delete", t.ID, "tree", tree.ID, map[string]string{"pattern": pattern})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// ---- license rules ----

// licenseGrants are the accepted LicenseRule.Grant values; "" falls back to
// the license default (proprietary: no access for non-members), available
// only through the REST API (specs/licensing/LicenseCompliance.md §2).
var licenseGrants = map[string]bool{"": true, "read-only": true, "modify": true, "redistribute": true}

// handleListLicenses answers GET /api/v1/trees/{tenant}/{tree}/licenses.
func (a *api) handleListLicenses(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	rules, err := a.d.Store.ListLicenseRules(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(rules))
	return nil
}

// handlePutLicense answers PUT /api/v1/trees/{tenant}/{tree}/licenses.
func (a *api) handlePutLicense(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var rule domain.LicenseRule
	if err := decodeJSON(w, r, &rule); err != nil {
		return err
	}
	rule.TreeID = tree.ID
	if err := validateGlob("path_glob", rule.PathGlob); err != nil {
		return err
	}
	if err := validateText("spdx", rule.SPDX, 200, true); err != nil {
		return err
	}
	if !licenseGrants[rule.Grant] {
		return badRequestf("grant must be one of read-only, modify, redistribute or \"\" (license default)")
	}
	if err := a.validateLicenseGrantee(r.Context(), t, &rule); err != nil {
		return err
	}
	if rule.ExpiresAt != nil {
		if !rule.ExpiresAt.After(time.Now()) {
			return badRequestf("expires_at must be in the future")
		}
		utc := rule.ExpiresAt.UTC()
		rule.ExpiresAt = &utc
	}
	if err := a.d.Store.PutLicenseRule(r.Context(), &rule); err != nil {
		return err
	}
	details := map[string]string{"path_glob": rule.PathGlob, "spdx": rule.SPDX, "grant": rule.Grant}
	if rule.Grantee != "" {
		details["grantee"] = rule.Grantee
	}
	if rule.ExpiresAt != nil {
		details["expires_at"] = rule.ExpiresAt.Format(time.RFC3339)
	}
	a.audit(r, "license.update", t.ID, "tree", tree.ID, details)
	writeJSON(w, http.StatusOK, rule)
	return nil
}

// validateLicenseGrantee checks LicenseRule.Grantee (v0.5, §13.1): "" (every
// non-owning tenant) or the slug of an existing tenant other than the
// tree's owner (whose members are never restricted).
func (a *api) validateLicenseGrantee(ctx context.Context, owner *domain.Tenant, rule *domain.LicenseRule) error {
	rule.Grantee = strings.TrimSpace(rule.Grantee)
	if rule.Grantee == "" {
		return nil
	}
	// tenant-lifecycle (§16.4): a verified tenant email names the tenant
	// (stored as its slug).
	if strings.Contains(rule.Grantee, "@") {
		g, err := a.verifiedEmailTenant(ctx, rule.Grantee)
		if err != nil {
			return badRequestf("grantee %q is not the verified email of a tenant", rule.Grantee)
		}
		rule.Grantee = g.Slug
	}
	if !slugRe.MatchString(rule.Grantee) {
		return badRequestf("grantee must be a tenant slug")
	}
	g, err := a.d.Store.GetTenantBySlug(ctx, rule.Grantee)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		return badRequestf("grantee tenant %q does not exist", rule.Grantee)
	case err != nil:
		return err
	}
	if g.ID == owner.ID {
		return badRequestf("grantee %q owns the tree: its members are never restricted by license rules", rule.Grantee)
	}
	return nil
}

// handleDeleteLicense answers DELETE /api/v1/trees/{tenant}/{tree}/licenses?path_glob=.
func (a *api) handleDeleteLicense(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	glob := r.URL.Query().Get("path_glob")
	if glob == "" {
		return badRequestf("query parameter path_glob is required")
	}
	if err := a.d.Store.DeleteLicenseRule(r.Context(), tree.ID, glob); err != nil {
		return err
	}
	a.audit(r, "license.delete", t.ID, "tree", tree.ID, map[string]string{"path_glob": glob})
	w.WriteHeader(http.StatusNoContent)
	return nil
}
