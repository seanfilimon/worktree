package httpapi

import (
	"context"
	"errors"
	"fmt"
	"net/http"
	"net/netip"
	"reflect"
	"strconv"
	"strings"
	"time"
	_ "time/tzdata" // policy time windows name IANA zones; do not depend on the host database

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// maxPolicyStatements bounds the size of one policy.
const maxPolicyStatements = 100

// ValidationErr is one problem found in a list of policy statements
// (POST /tenants/{t}/policies/validate).
type ValidationErr struct {
	Statement int    `json:"statement"` // index into the statements array
	Field     string `json:"field"`     // e.g. "actions[1]", "conditions.ip_ranges[0]"
	Message   string `json:"message"`
}

// PolicyValidator is implemented by policy engines (Deps.Access) that can
// validate statements authoritatively. When Deps.Access does not implement
// it, a method ValidateStatements([]domain.PolicyStatement) []T is still
// used if T is (a pointer to) a struct with Statement/Index int and
// Field/Path, Message string fields, or an error type; otherwise the
// built-in structural validation below applies.
type PolicyValidator interface {
	ValidateStatements(stmts []domain.PolicyStatement) []ValidationErr
}

type policyRequest struct {
	Name        string                   `json:"name"`
	Description string                   `json:"description"`
	Enabled     *bool                    `json:"enabled"` // default true
	Statements  []domain.PolicyStatement `json:"statements"`
	// Version, when non-zero on PUT, must equal the stored version
	// (optimistic concurrency).
	Version int `json:"version"`

	// Server-managed fields: accepted so that a fetched policy can be sent
	// back unchanged, and ignored.
	ID        string     `json:"id"`
	TenantID  string     `json:"tenant_id"`
	CreatedBy string     `json:"created_by"`
	CreatedAt *time.Time `json:"created_at"`
	UpdatedAt *time.Time `json:"updated_at"`
}

type validatePolicyRequest struct {
	Statements []domain.PolicyStatement `json:"statements"`
}

type validatePolicyResponse struct {
	Valid  bool            `json:"valid"`
	Errors []ValidationErr `json:"errors"`
}

// validateStatements runs the engine's validator when available, else the
// structural one. The result is never nil.
func (a *api) validateStatements(stmts []domain.PolicyStatement) []ValidationErr {
	var out []ValidationErr
	if len(stmts) > maxPolicyStatements {
		out = append(out, ValidationErr{Statement: maxPolicyStatements, Field: "statements",
			Message: fmt.Sprintf("a policy may have at most %d statements", maxPolicyStatements)})
	}
	if v := engineValidator(a.d.Access); v != nil {
		out = append(out, v(stmts)...)
	} else {
		out = append(out, basicValidateStatements(stmts)...)
	}
	return nonNil(out)
}

// engineValidator adapts an engine's ValidateStatements method, or returns
// nil when x has none.
func engineValidator(x any) func([]domain.PolicyStatement) []ValidationErr {
	if x == nil {
		return nil
	}
	if v, ok := x.(PolicyValidator); ok {
		return v.ValidateStatements
	}
	m := reflect.ValueOf(x).MethodByName("ValidateStatements")
	if !m.IsValid() {
		return nil
	}
	mt := m.Type()
	if mt.NumIn() != 1 || mt.In(0) != reflect.TypeOf([]domain.PolicyStatement(nil)) ||
		mt.NumOut() != 1 || mt.Out(0).Kind() != reflect.Slice {
		return nil
	}
	return func(stmts []domain.PolicyStatement) []ValidationErr {
		res := m.Call([]reflect.Value{reflect.ValueOf(stmts)})[0]
		out := make([]ValidationErr, 0, res.Len())
		for i := 0; i < res.Len(); i++ {
			if ve, ok := reflectValidationErr(res.Index(i)); ok {
				out = append(out, ve)
			}
		}
		return out
	}
}

var iamErrorType = reflect.TypeOf((*error)(nil)).Elem()

// reflectValidationErr converts one element of a foreign validation result.
func reflectValidationErr(v reflect.Value) (ValidationErr, bool) {
	var ve ValidationErr
	orig := v
	for v.Kind() == reflect.Interface || v.Kind() == reflect.Pointer {
		if v.IsNil() {
			return ve, false
		}
		v = v.Elem()
	}
	if v.Kind() == reflect.Struct {
		for _, name := range []string{"Statement", "Index", "StatementIndex"} {
			if f := v.FieldByName(name); f.IsValid() && f.CanInt() {
				ve.Statement = int(f.Int())
				break
			}
		}
		for _, name := range []string{"Field", "Path"} {
			if f := v.FieldByName(name); f.IsValid() && f.Kind() == reflect.String {
				ve.Field = f.String()
				break
			}
		}
		for _, name := range []string{"Message", "Msg", "Reason"} {
			if f := v.FieldByName(name); f.IsValid() && f.Kind() == reflect.String {
				ve.Message = f.String()
				break
			}
		}
	}
	if ve.Message == "" && orig.Type().Implements(iamErrorType) && orig.CanInterface() {
		if err, ok := orig.Interface().(error); ok && err != nil {
			ve.Message = err.Error()
		}
	}
	if ve.Message == "" {
		ve.Message = "invalid statement"
	}
	return ve, true
}

var iamWeekdays = map[string]bool{"mon": true, "tue": true, "wed": true, "thu": true, "fri": true, "sat": true, "sun": true}

// basicValidateStatements checks statement structure: effects, action
// patterns, resource and principal syntax and conditions.
func basicValidateStatements(stmts []domain.PolicyStatement) []ValidationErr {
	var out []ValidationErr
	sids := map[string]int{}
	for i, s := range stmts {
		add := func(field, format string, args ...any) {
			out = append(out, ValidationErr{Statement: i, Field: field, Message: fmt.Sprintf(format, args...)})
		}
		if s.Sid != "" {
			if len(s.Sid) > 64 || strings.ContainsFunc(s.Sid, func(r rune) bool { return r < 0x20 || r == 0x7f }) {
				add("sid", "sid must be at most 64 printable characters")
			} else if j, dup := sids[s.Sid]; dup {
				add("sid", "sid %q is already used by statement %d", s.Sid, j)
			} else {
				sids[s.Sid] = i
			}
		}
		if s.Effect != domain.EffectAllow && s.Effect != domain.EffectDeny {
			add("effect", "effect must be %q or %q", domain.EffectAllow, domain.EffectDeny)
		}
		checkList := func(field string, vals []string, check func(string) string) {
			if len(vals) == 0 {
				add(field, "%s must not be empty", field)
				return
			}
			if len(vals) > 256 {
				add(field, "%s may have at most 256 entries", field)
				return
			}
			for j, v := range vals {
				if msg := check(v); msg != "" {
					add(field+"["+strconv.Itoa(j)+"]", "%s", msg)
				}
			}
		}
		checkList("actions", s.Actions, func(v string) string { return checkActionPattern(v, true) })
		checkList("resources", s.Resources, checkResourcePattern)
		checkList("principals", s.Principals, checkPrincipalPattern)

		c := s.Conditions
		for j, r := range c.IPRanges {
			if _, err := netip.ParsePrefix(strings.TrimSpace(r)); err != nil {
				if _, err := netip.ParseAddr(strings.TrimSpace(r)); err != nil {
					add("conditions.ip_ranges["+strconv.Itoa(j)+"]", "%q is not a CIDR range", r)
				}
			}
		}
		if c.NotBefore != nil && c.NotAfter != nil && !c.NotAfter.After(*c.NotBefore) {
			add("conditions.not_after", "not_after must be later than not_before")
		}
		if tw := c.TimeWindow; tw != nil {
			for j, d := range tw.Days {
				if !iamWeekdays[strings.ToLower(d)] {
					add("conditions.time_window.days["+strconv.Itoa(j)+"]", "%q is not a day (mon..sun)", d)
				}
			}
			start, errS := time.Parse("15:04", tw.Start)
			if errS != nil {
				add("conditions.time_window.start", "start must be HH:MM (24-hour)")
			}
			end, errE := time.Parse("15:04", tw.End)
			if errE != nil {
				add("conditions.time_window.end", "end must be HH:MM (24-hour)")
			}
			if errS == nil && errE == nil && start.Equal(end) {
				add("conditions.time_window.end", "end must differ from start")
			}
			if tw.Timezone != "" {
				if _, err := time.LoadLocation(tw.Timezone); err != nil {
					add("conditions.time_window.timezone", "unknown time zone %q", tw.Timezone)
				}
			}
		}
	}
	return out
}

// checkResourcePattern validates "*" or "wt:<kind>/<segments>".
func checkResourcePattern(v string) string {
	if v == "*" {
		return ""
	}
	rest, ok := strings.CutPrefix(v, "wt:")
	if !ok {
		return fmt.Sprintf("resource %q must be \"*\" or start with \"wt:\"", v)
	}
	kind, path, _ := strings.Cut(rest, "/")
	var lo, hi int
	switch kind {
	case "tenant":
		lo, hi = 1, 1
	case "tree":
		lo, hi = 2, 2
	case "branch", "path":
		lo, hi = 3, -1
	default:
		return fmt.Sprintf("resource %q: kind must be tenant, tree, branch or path", v)
	}
	segs := strings.Split(path, "/")
	deep := false
	for _, s := range segs {
		switch {
		case s == "":
			return fmt.Sprintf("resource %q has an empty segment", v)
		case s == "**":
			deep = true
		case strings.Contains(s, "**"):
			return fmt.Sprintf("resource %q: \"**\" must be a whole segment", v)
		}
	}
	if !deep && (len(segs) < lo || (hi > 0 && len(segs) > hi)) {
		want := strconv.Itoa(lo)
		if hi < 0 {
			want = "at least " + want
		}
		return fmt.Sprintf("resource %q: wt:%s/ needs %s segment(s)", v, kind, want)
	}
	return ""
}

// checkPrincipalPattern validates a statement principal.
func checkPrincipalPattern(v string) string {
	switch v {
	case "*", "authenticated", "anonymous":
		return ""
	}
	kind, name, ok := strings.Cut(v, ":")
	if ok {
		switch kind {
		case "user", "team", "role", "sa":
			if name != "" && !strings.ContainsFunc(name, func(r rune) bool { return r <= ' ' || r == 0x7f }) {
				return ""
			}
			return fmt.Sprintf("principal %q needs a name after %q", v, kind+":")
		}
	}
	return fmt.Sprintf("principal %q must be user:<name>, team:<slug>, role:<role>, sa:<name>, authenticated, anonymous or *", v)
}

// statementsError turns validation problems into a 400.
func statementsError(errs []ValidationErr) error {
	if len(errs) == 0 {
		return nil
	}
	msgs := make([]string, 0, 3)
	for i, e := range errs {
		if i == 3 {
			msgs = append(msgs, fmt.Sprintf("and %d more", len(errs)-3))
			break
		}
		msgs = append(msgs, fmt.Sprintf("statements[%d].%s: %s", e.Statement, e.Field, e.Message))
	}
	return badRequestf("invalid policy: %s", strings.Join(msgs, "; "))
}

// checkPolicyRequest normalizes and validates a create / replace body.
func (a *api) checkPolicyRequest(req *policyRequest) error {
	req.Name = strings.TrimSpace(req.Name)
	req.Description = strings.TrimSpace(req.Description)
	if err := validateText("name", req.Name, 100, true); err != nil {
		return err
	}
	if err := validateText("description", req.Description, 2000, false); err != nil {
		return err
	}
	if req.Statements == nil {
		req.Statements = []domain.PolicyStatement{}
	}
	return statementsError(a.validateStatements(req.Statements))
}

// checkPolicyName fails with 409 when another policy of the tenant is
// already called name.
func (a *api) checkPolicyName(ctx context.Context, tenantID, name, selfID string) error {
	ps, err := a.d.Store.ListPolicies(ctx, tenantID)
	if err != nil {
		return err
	}
	for _, p := range ps {
		if p.ID != selfID && strings.EqualFold(p.Name, name) {
			return conflictf("policy %q already exists", name)
		}
	}
	return nil
}

// handleListPolicies answers GET /api/v1/tenants/{tenant}/policies.
func (a *api) handleListPolicies(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	ps, err := a.d.Store.ListPolicies(r.Context(), t.ID)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, nonNil(ps))
	return nil
}

// handleCreatePolicy answers POST /api/v1/tenants/{tenant}/policies.
func (a *api) handleCreatePolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req policyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := a.checkPolicyRequest(&req); err != nil {
		return err
	}
	ctx := r.Context()
	if err := a.checkPolicyName(ctx, t.ID, req.Name, ""); err != nil {
		return err
	}
	now := time.Now().UTC()
	pol := &domain.Policy{
		TenantID: t.ID, Name: req.Name, Description: req.Description, Enabled: req.Enabled == nil || *req.Enabled,
		Statements: req.Statements, Version: 1, CreatedBy: principal(r).Username, CreatedAt: now, UpdatedAt: now,
	}
	if err := a.d.Store.CreatePolicy(ctx, pol); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("policy %q already exists", req.Name)
		}
		return err
	}
	a.audit(r, "policy.create", t.ID, "policy", pol.ID, map[string]string{
		"name": pol.Name, "enabled": strconv.FormatBool(pol.Enabled), "statements": strconv.Itoa(len(pol.Statements))})
	writeJSON(w, http.StatusCreated, pol)
	return nil
}

// handleValidatePolicy answers POST /api/v1/tenants/{tenant}/policies/validate.
func (a *api) handleValidatePolicy(w http.ResponseWriter, r *http.Request) error {
	if _, err := a.iamTenant(r, domain.ActIAMRead); err != nil {
		return err
	}
	var req validatePolicyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	errs := a.validateStatements(req.Statements)
	writeJSON(w, http.StatusOK, validatePolicyResponse{Valid: len(errs) == 0, Errors: errs})
	return nil
}

// policyByPath loads policy {id} of tenant t.
func (a *api) policyByPath(r *http.Request, t *domain.Tenant) (*domain.Policy, error) {
	id := r.PathValue("id")
	p, err := a.d.Store.GetPolicy(r.Context(), t.ID, id)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, notFoundf("policy %q not found", id)
	}
	return p, err
}

// handleGetPolicy answers GET /api/v1/tenants/{tenant}/policies/{id}.
func (a *api) handleGetPolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMRead)
	if err != nil {
		return err
	}
	pol, err := a.policyByPath(r, t)
	if err != nil {
		return err
	}
	writeJSON(w, http.StatusOK, pol)
	return nil
}

// handlePutPolicy answers PUT /api/v1/tenants/{tenant}/policies/{id}: a
// full replace that increments the version.
func (a *api) handlePutPolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	var req policyRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := a.checkPolicyRequest(&req); err != nil {
		return err
	}
	pol, err := a.policyByPath(r, t)
	if err != nil {
		return err
	}
	if req.Version != 0 && req.Version != pol.Version {
		return conflictf("policy was modified (version %d, request is based on %d)", pol.Version, req.Version)
	}
	ctx := r.Context()
	if err := a.checkPolicyName(ctx, t.ID, req.Name, pol.ID); err != nil {
		return err
	}
	prevEnabled := pol.Enabled
	pol.Name, pol.Description, pol.Statements = req.Name, req.Description, req.Statements
	pol.Enabled = req.Enabled == nil || *req.Enabled
	pol.Version++
	pol.UpdatedAt = time.Now().UTC()
	if err := a.d.Store.UpdatePolicy(ctx, pol); err != nil {
		return err
	}
	a.audit(r, "policy.update", t.ID, "policy", pol.ID, map[string]string{
		"name": pol.Name, "version": strconv.Itoa(pol.Version), "enabled": strconv.FormatBool(pol.Enabled),
		"previous_enabled": strconv.FormatBool(prevEnabled), "statements": strconv.Itoa(len(pol.Statements))})
	writeJSON(w, http.StatusOK, pol)
	return nil
}

// handleDeletePolicy answers DELETE /api/v1/tenants/{tenant}/policies/{id}.
func (a *api) handleDeletePolicy(w http.ResponseWriter, r *http.Request) error {
	t, err := a.iamTenant(r, domain.ActIAMAdmin)
	if err != nil {
		return err
	}
	pol, err := a.policyByPath(r, t)
	if err != nil {
		return err
	}
	if err := a.d.Store.DeletePolicy(r.Context(), t.ID, pol.ID); err != nil {
		return err
	}
	a.audit(r, "policy.delete", t.ID, "policy", pol.ID, map[string]string{"name": pol.Name})
	w.WriteHeader(http.StatusNoContent)
	return nil
}
