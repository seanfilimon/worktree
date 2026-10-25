package httpapi

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/policyhist"
)

// Policy version history, diff and rollback (docs/ENGINEERING_PLAN.md
// §14.7). Versions are recorded by the policy write handlers in
// iam_policies.go and by accesscfg applies (package policyhist).
//
//	GET  /tenants/{t}/policies/deleted                 -> [PolicyVersion]   (latest version of each deleted policy)
//	GET  /tenants/{t}/policies/{id}/versions           -> [PolicyVersion]   (oldest first)
//	GET  /tenants/{t}/policies/{id}/versions/{v}       -> PolicyVersion
//	GET  /tenants/{t}/policies/{id}/diff?from=&to=     -> {from, to, changes, fields}
//	POST /tenants/{t}/policies/{id}/rollback {version, comment?} -> PolicyVersion (the new version)
//
// Reads need iam:read, rollback iam:admin. History stays readable after a
// policy is deleted, and rolling a deleted policy back restores it.

// registerPolicyHistoryRoutes registers the v0.6 routes for policy version
// history, diff and rollback (§14.7).
func (a *api) registerPolicyHistoryRoutes() {
	const pols = "/api/v1/tenants/{tenant}/policies"
	a.handle("GET "+pols+"/deleted", a.handleListDeletedPolicies)
	a.handle("GET "+pols+"/{id}/versions", a.handleListPolicyVersions)
	a.handle("GET "+pols+"/{id}/versions/{version}", a.handleGetPolicyVersion)
	a.handle("GET "+pols+"/{id}/diff", a.handlePolicyDiff)
	a.handle("POST "+pols+"/{id}/rollback", a.handlePolicyRollback)
}

// policyHistory returns the version recorder.
func (a *api) policyHistory() *policyhist.Recorder {
	return policyhist.New(a.d.Store, a.d.Bus, a.log)
}

// recordPolicy appends a version for a REST policy change. The change is
// already stored, so a recording failure is logged rather than returned;
// the returned version is 0 then.
func (a *api) recordPolicy(r *http.Request, e policyhist.Entry) int {
	if p := principal(r); p != nil {
		e.Actor = p.Username
	}
	ctx := context.WithoutCancel(r.Context())
	pv, err := a.policyHistory().Record(ctx, e)
	if err != nil {
		a.log.ErrorContext(ctx, "recording a policy version failed", "policy_id", e.Policy.ID, "error", err)
		return 0
	}
	return pv.Version
}

// policyVersions loads the history of policy {id} of tenant t (with the
// synthetic version 1 for policies older than history recording). current
// is the live policy, nil when it was deleted. 404 when the policy has
// neither a live copy nor a history.
func (a *api) policyVersions(r *http.Request, t *domain.Tenant) (vs []domain.PolicyVersion, current *domain.Policy, err error) {
	ctx := r.Context()
	id := r.PathValue("id")
	current, err = a.d.Store.GetPolicy(ctx, t.ID, id)
	if errors.Is(err, domain.ErrNotFound) {
		current, err = nil, nil
	}
	if err != nil {
		return nil, nil, err
	}
	vs, err = a.policyHistory().History(ctx, t.ID, id, current)
	if err != nil {
		return nil, nil, err
	}
	if len(vs) == 0 {
		return nil, nil, notFoundf("policy %q not found", id)
	}
	return vs, current, nil
}

// findVersion returns version v of vs, or a 404.
func findVersion(vs []domain.PolicyVersion, v int) (*domain.PolicyVersion, error) {
	for i := range vs {
		if vs[i].Version == v {
			return &vs[i], nil
		}
	}
	return nil, notFoundf("policy version %d not found", v)
}

// parseVersion parses a version number (path value or query parameter).
// allowZero admits 0 (the empty policy, for diffs).
func parseVersion(name, s string, allowZero bool) (int, error) {
	n, err := strconv.Atoi(strings.TrimSpace(s))
	if err != nil || n < 0 || (n == 0 && !allowZero) {
		if allowZero {
			return 0, badRequestf("%s must be a version number (0 = the empty policy)", name)
		}
		return 0, badRequestf("%s must be a positive version number", name)
	}
	return n, nil
}

// handleListDeletedPolicies answers GET /api/v1/tenants/{tenant}/policies/deleted.
func (a *api) handleListDeletedPolicies(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	deleted, err := a.policyHistory().Deleted(ctx, t.ID)
	if err != nil {
		return err
	}
	out := make([]domain.PolicyVersion, 0, len(deleted))
	for _, v := range deleted {
		// A policy restored outside the history (e.g. an undone apply) is
		// not deleted.
		if _, err := a.d.Store.GetPolicy(ctx, t.ID, v.PolicyID); err == nil {
			continue
		} else if !errors.Is(err, domain.ErrNotFound) {
			return err
		}
		out = append(out, v)
	}
	writeJSON(w, http.StatusOK, out)
	return nil
}

// handleListPolicyVersions answers GET /api/v1/tenants/{tenant}/policies/{id}/versions.
func (a *api) handleListPolicyVersions(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	vs, _, err := a.policyVersions(r, t)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, vs)
	return nil
}

// handleGetPolicyVersion answers GET /api/v1/tenants/{tenant}/policies/{id}/versions/{version}.
func (a *api) handleGetPolicyVersion(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	n, err := parseVersion("version", r.PathValue("version"), false)
	if err != nil {
		return err
	}
	vs, _, err := a.policyVersions(r, t)
	if err != nil {
		return err
	}
	v, err := findVersion(vs, n)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, v)
	return nil
}

// diffSide is the policy a version stands for in a diff: deletes stand for
// the empty policy, as does version 0.
func diffSide(vs []domain.PolicyVersion, n int) (*domain.Policy, error) {
	if n == 0 {
		return nil, nil
	}
	v, err := findVersion(vs, n)
	if err != nil {
		return nil, err
	}
	if v.Change == policyhist.ChangeDelete {
		return nil, nil
	}
	return &v.Policy, nil
}

// handlePolicyDiff answers GET /api/v1/tenants/{tenant}/policies/{id}/diff?from=&to=.
// to defaults to the latest version and from to the version before to.
func (a *api) handlePolicyDiff(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	vs, _, err := a.policyVersions(r, t)
	if err != nil {
		return err
	}
	q := r.URL.Query()
	to := vs[len(vs)-1].Version
	if s := q.Get("to"); s != "" {
		if to, err = parseVersion("to", s, false); err != nil {
			return err
		}
	}
	from := to - 1
	if s := q.Get("from"); s != "" {
		if from, err = parseVersion("from", s, true); err != nil {
			return err
		}
	}
	before, err := diffSide(vs, from)
	if err != nil {
		return err
	}
	after, err := diffSide(vs, to)
	if err != nil {
		return err
	}
	changes, fields := policyhist.Diff(before, after)
	writeJSON(w, http.StatusOK, policyhist.DiffResult{From: from, To: to, Changes: changes, Fields: fields})
	return nil
}

type rollbackRequest struct {
	Version int    `json:"version"`
	Comment string `json:"comment"`
}

// handlePolicyRollback answers POST /api/v1/tenants/{tenant}/policies/{id}/rollback:
// the policy's name, description, enabled flag and statements become those
// of the requested version, recorded as a new "rollback" version (Policy
// .Version is incremented as by PUT). A deleted policy is restored with
// its id. Config-managed policies need ?force=true by a tenant owner.
func (a *api) handlePolicyRollback(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req rollbackRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Comment = strings.TrimSpace(req.Comment)
	if req.Version < 1 {
		return badRequestf("version must be a positive version number")
	}
	if err := validateText("comment", req.Comment, 1000, false); err != nil {
		return err
	}
	vs, current, err := a.policyVersions(r, t)
	if err != nil {
		return err
	}
	target, err := findVersion(vs, req.Version)
	if err != nil {
		return err
	}
	src := target.Policy
	managedBy := src.ManagedBy
	if current != nil {
		managedBy = current.ManagedBy
	}
	if err := a.checkManaged(r, t.ID, managedBy, "policy "+src.Name); err != nil {
		return err
	}
	if src.Statements == nil {
		src.Statements = []domain.PolicyStatement{}
	}
	// The catalog may have changed since the version was written.
	if err := statementsError(a.validateStatements(src.Statements)); err != nil {
		return err
	}
	ctx := r.Context()
	selfID := src.ID
	if err := a.checkPolicyName(ctx, t.ID, src.Name, selfID); err != nil {
		return err
	}

	comment := fmt.Sprintf("rollback to version %d", target.Version)
	if req.Comment != "" {
		comment += ": " + req.Comment
	}
	now := time.Now().UTC()
	var next domain.Policy
	restored := current == nil
	if current != nil {
		if policyhist.SameContent(current, &src) {
			return conflictf("policy %s already matches version %d", current.Name, target.Version)
		}
		next = *current
		next.Name, next.Description, next.Enabled, next.Statements = src.Name, src.Description, src.Enabled, src.Statements
		next.Version = current.Version + 1
		next.UpdatedAt = now
		if err := a.d.Store.UpdatePolicy(ctx, &next); err != nil {
			return err
		}
	} else {
		// The store starts a re-created policy at Policy.Version 1; the
		// history numbering continues.
		next = src
		next.ID, next.TenantID = r.PathValue("id"), t.ID
		next.UpdatedAt = now
		if err := a.d.Store.CreatePolicy(ctx, &next); err != nil {
			if errors.Is(err, domain.ErrAlreadyExists) {
				return conflictf("policy %q already exists", next.Name)
			}
			return err
		}
	}
	e := policyhist.Entry{Change: policyhist.ChangeRollback, Comment: comment, Before: current, Policy: next}
	if p := principal(r); p != nil {
		e.Actor = p.Username
	}
	pv, err := a.policyHistory().Record(context.WithoutCancel(ctx), e)
	if err != nil {
		return err
	}
	a.audit(r, "policy.rollback", t.ID, "policy", next.ID, map[string]string{
		"name": next.Name, "version": strconv.Itoa(next.Version), "to_version": strconv.Itoa(target.Version),
		"history_version": strconv.Itoa(pv.Version), "restored": strconv.FormatBool(restored), "comment": req.Comment})
	writeJSON(w, http.StatusOK, pv)
	return nil
}
