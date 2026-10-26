package httpapi

import (
	"net/http"
	"strconv"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/scan"
)

// registerComplianceRoutes registers the v0.8 routes for secret scanning
// and license compliance (docs/ENGINEERING_PLAN.md §16.7). Without
// Deps.Scan they answer 501.
//
//	GET|PUT          {tree}/compliance/settings          tree:read | tree:admin
//	GET              {tree}/compliance/findings          tree:write
//	POST             {tree}/compliance/findings/{id}/resolve  tree:admin
//	GET|POST|DELETE  {tree}/compliance/allowlist         tree:write | tree:admin
//	POST             {tree}/compliance/scan              tree:admin
//	GET              {tree}/compliance/licenses          tree:admin
//	GET              /api/v1/admin/compliance            server:admin
func (a *api) registerComplianceRoutes(tree string) {
	base := tree + "/compliance"
	a.handle("GET "+base+"/settings", a.handleGetComplianceSettings)
	a.handle("PUT "+base+"/settings", a.handlePutComplianceSettings)
	a.handle("GET "+base+"/findings", a.handleListComplianceFindings)
	a.handle("POST "+base+"/findings/{id}/resolve", a.handleResolveComplianceFinding)
	a.handle("GET "+base+"/allowlist", a.handleGetComplianceAllowlist)
	a.handle("POST "+base+"/allowlist", a.handleAddComplianceAllow)
	a.handle("DELETE "+base+"/allowlist", a.handleDeleteComplianceAllow)
	a.handle("POST "+base+"/scan", a.handleComplianceScan)
	a.handle("GET "+base+"/licenses", a.handleComplianceLicenses)
	a.handle("GET /api/v1/admin/compliance", a.handleAdminCompliance)
}

// complianceScope resolves the scan service and the tree of r (act).
func (a *api) complianceScope(r *http.Request, act domain.Action) (*scan.Service, *domain.Tenant, *domain.Tree, error) {
	if a.d.Scan == nil {
		return nil, nil, nil, errIAMNotImplemented("Secret scanning and license compliance")
	}
	t, tree, err := a.treeFor(r, act)
	if err != nil {
		return nil, nil, nil, err
	}
	return a.d.Scan, t, tree, nil
}

// handleGetComplianceSettings answers GET {tree}/compliance/settings.
func (a *api) handleGetComplianceSettings(w http.ResponseWriter, r *http.Request) error {
	svc, _, tree, err := a.complianceScope(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	st, err := svc.GetSettings(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, st)
	return nil
}

// handlePutComplianceSettings answers PUT {tree}/compliance/settings
// {mode?, exclude?, max_file_bytes?}.
func (a *api) handlePutComplianceSettings(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, t, tree, err := a.complianceScope(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var patch scan.SettingsPatch
	if err := decodeJSON(w, r, &patch); err != nil {
		return err
	}
	st, err := svc.UpdateSettings(r.Context(), p, tree, patch)
	if err != nil {
		return err
	}
	a.audit(r, "compliance.settings", t.ID, "tree", tree.ID, map[string]string{
		"tree": tree.Name, "mode": st.Mode, "exclude": strings.Join(st.Exclude, ","),
		"max_file_bytes": strconv.FormatInt(st.MaxFileBytes, 10),
	})
	writeJSON(w, http.StatusOK, st)
	return nil
}

// handleListComplianceFindings answers GET {tree}/compliance/findings
// ?status=open|resolved|all&limit=.
func (a *api) handleListComplianceFindings(w http.ResponseWriter, r *http.Request) error {
	svc, _, tree, err := a.complianceScope(r, domain.ActTreeWrite)
	if err != nil {
		return err
	}
	status := r.URL.Query().Get("status")
	switch status {
	case "", scan.StatusOpen:
		status = scan.StatusOpen
	case scan.StatusResolved:
	case "all":
		status = ""
	default:
		return badRequestf("status must be open, resolved or all")
	}
	limit, err := queryInt(r, "limit", 100, 1000)
	if err != nil {
		return err
	}
	list, err := svc.ListFindings(r.Context(), tree.ID, status, limit)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, list)
	return nil
}

// handleResolveComplianceFinding answers POST
// {tree}/compliance/findings/{id}/resolve {resolution, note?, reopen?}.
func (a *api) handleResolveComplianceFinding(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, t, tree, err := a.complianceScope(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var req scan.ResolveRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	id := r.PathValue("id")
	f, err := svc.Resolve(r.Context(), p, tree, id, req)
	if err != nil {
		return err
	}
	action := "compliance.finding.resolve"
	if req.Reopen {
		action = "compliance.finding.reopen"
	}
	a.audit(r, action, t.ID, "secret_finding", tree.ID+"/"+id, map[string]string{
		"tree": tree.Name, "path": f.Path, "rule": f.Rule, "fingerprint": f.Fingerprint, "resolution": f.Resolution,
	})
	writeJSON(w, http.StatusOK, f)
	return nil
}

// handleGetComplianceAllowlist answers GET {tree}/compliance/allowlist.
func (a *api) handleGetComplianceAllowlist(w http.ResponseWriter, r *http.Request) error {
	svc, _, tree, err := a.complianceScope(r, domain.ActTreeWrite)
	if err != nil {
		return err
	}
	al, err := svc.GetAllowList(r.Context(), tree.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, al)
	return nil
}

// handleAddComplianceAllow answers POST {tree}/compliance/allowlist
// {fingerprint, reason}.
func (a *api) handleAddComplianceAllow(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	svc, t, tree, err := a.complianceScope(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var req scan.AllowRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	e, err := svc.Allow(r.Context(), p, tree, req)
	if err != nil {
		return err
	}
	a.audit(r, "compliance.allow.add", t.ID, "tree", tree.ID, map[string]string{
		"tree": tree.Name, "fingerprint": e.Fingerprint, "reason": e.Reason,
	})
	writeJSON(w, http.StatusCreated, e)
	return nil
}

// handleDeleteComplianceAllow answers DELETE
// {tree}/compliance/allowlist?fingerprint=.
func (a *api) handleDeleteComplianceAllow(w http.ResponseWriter, r *http.Request) error {
	if _, err := requireLogin(r); err != nil {
		return err
	}
	svc, t, tree, err := a.complianceScope(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	fp := r.URL.Query().Get("fingerprint")
	if fp == "" {
		return badRequestf("fingerprint is required")
	}
	if err := svc.Disallow(r.Context(), tree, fp); err != nil {
		return err
	}
	a.audit(r, "compliance.allow.remove", t.ID, "tree", tree.ID, map[string]string{"tree": tree.Name, "fingerprint": fp})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleComplianceScan answers POST {tree}/compliance/scan {ref?}: an
// on-demand scan of a whole snapshot (nothing is stored).
func (a *api) handleComplianceScan(w http.ResponseWriter, r *http.Request) error {
	svc, _, tree, err := a.complianceScope(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	var req scan.ScanRequest
	if r.ContentLength != 0 {
		if err := decodeJSON(w, r, &req); err != nil {
			return err
		}
	}
	res, err := svc.ScanSnapshot(r.Context(), tree, req.Ref)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// handleComplianceLicenses answers GET {tree}/compliance/licenses?ref=.
func (a *api) handleComplianceLicenses(w http.ResponseWriter, r *http.Request) error {
	svc, _, tree, err := a.complianceScope(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	audit, err := svc.AuditLicenses(r.Context(), tree, r.URL.Query().Get("ref"))
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, audit)
	return nil
}

// handleAdminCompliance answers GET /api/v1/admin/compliance.
func (a *api) handleAdminCompliance(w http.ResponseWriter, r *http.Request) error {
	if err := a.d.Authz.Authorize(r.Context(), principal(r), domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	if a.d.Scan == nil {
		return errIAMNotImplemented("Secret scanning and license compliance")
	}
	sum, err := a.d.Scan.Summarize(r.Context())
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, sum)
	return nil
}
