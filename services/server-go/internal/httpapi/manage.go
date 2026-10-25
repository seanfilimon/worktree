package httpapi

import (
	"context"
	"errors"
	"net/http"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/events"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// registerManageRoutes registers the management endpoints of
// ENGINEERING_PLAN §11: branch/tag create and delete, tenant update,
// password changes, admin account creation, system info and search.
// tree is the "/api/v1/trees/{tenant}/{tree}" prefix.
func (a *api) registerManageRoutes(tree string) {
	a.handle("POST "+tree+"/branches", a.handleCreateBranch)
	a.handle("DELETE "+tree+"/branches/{name...}", a.handleDeleteBranch)
	a.handle("POST "+tree+"/tags", a.handleCreateTag)
	a.handle("DELETE "+tree+"/tags/{name...}", a.handleDeleteTag)
	a.handle("PATCH /api/v1/tenants/{tenant}", a.handlePatchTenant)
	a.handle("POST /api/v1/auth/password", a.handleChangePassword)
	a.handle("POST /api/v1/admin/accounts", a.handleAdminCreateAccount)
	a.handle("POST /api/v1/admin/accounts/{username}/password", a.handleAdminSetPassword)
	a.handle("GET /api/v1/admin/system", a.handleAdminSystem)
	a.handle("GET /api/v1/search", a.handleSearch)
}

// Event types published by the management endpoints that have no constant
// in internal/domain (which is shared and changed additively only).
const (
	eventBranchDeleted = "branch.deleted"
	eventTagDeleted    = "tag.deleted"
)

const (
	maxRefNameLen    = 200
	maxTagMessage    = 4096
	maxTenantNameLen = 100 // characters
	maxSearchQuery   = 100 // characters
	defaultSearchMax = 20
	maxSearchLimit   = 50
)

// validPlans are the tenant plans accepted by PATCH /tenants/{tenant}.
var validPlans = map[string]bool{"free": true, "pro": true, "enterprise": true}

// validateRefNameManage checks a branch or tag name with the CLI's rules:
// no whitespace, no "..", no leading '-' or '/', no trailing '/' or
// ".lock", at most 200 bytes (plus the server's Git-like ref rules).
func validateRefNameManage(kind, name string) error {
	switch {
	case name == "" || len(name) > maxRefNameLen:
		return badRequestf("%s name must be 1-%d bytes", kind, maxRefNameLen)
	case !utf8.ValidString(name):
		return badRequestf("%s name must be valid UTF-8", kind)
	case strings.HasPrefix(name, "/") || strings.HasSuffix(name, "/") || strings.Contains(name, "//"):
		return badRequestf("%s name must not start or end with '/' or contain '//'", kind)
	case strings.HasPrefix(name, "-"):
		return badRequestf("%s name must not start with '-'", kind)
	case strings.Contains(name, ".."):
		return badRequestf("%s name must not contain '..'", kind)
	case strings.HasSuffix(name, ".lock") || strings.HasSuffix(name, "."):
		return badRequestf("%s name must not end with '.lock' or '.'", kind)
	case strings.ContainsFunc(name, func(r rune) bool {
		return unicode.IsSpace(r) || unicode.IsControl(r) || strings.ContainsRune(`~^:?*[\`, r)
	}):
		return badRequestf("%s name must not contain whitespace, control characters or any of ~^:?*[\\", kind)
	}
	return nil
}

// revisionID resolves rev with snapshotForRev and returns the snapshot id.
// A branch that has no snapshots yet resolves to "" (so a new branch can be
// created from an empty one).
func (a *api) revisionID(ctx context.Context, tree *domain.Tree, rev string) (string, error) {
	if b, err := a.d.Store.GetBranch(ctx, tree.ID, rev); err == nil && b.Tip == "" {
		return "", nil
	}
	s, err := a.snapshotForRev(ctx, tree, rev)
	if err != nil {
		return "", err
	}
	return s.ID, nil
}

// ---- branches ----

type createBranchRequest struct {
	Name string `json:"name"`
	From string `json:"from"`
}

// handleCreateBranch answers POST {tree}/branches.
func (a *api) handleCreateBranch(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActBranchCreate)
	if err != nil {
		return err
	}
	var req createBranchRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := validateRefNameManage("branch", req.Name); err != nil {
		return err
	}
	ctx := r.Context()
	if _, err := a.d.Store.GetBranch(ctx, tree.ID, req.Name); err == nil {
		return conflictf("branch %q already exists", req.Name)
	} else if !errors.Is(err, domain.ErrNotFound) {
		return err
	}
	var tip string
	if req.From == "" {
		def, err := a.d.Store.GetBranch(ctx, tree.ID, tree.DefaultBranch)
		switch {
		case err == nil:
			tip = def.Tip
		case !errors.Is(err, domain.ErrNotFound):
			return err
		}
	} else if tip, err = a.revisionID(ctx, tree, req.From); err != nil {
		return err
	}
	if err := a.d.Store.CompareAndSwapBranchTip(ctx, tree.ID, req.Name, "", tip); err != nil {
		if errors.Is(err, domain.ErrConflict) || errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("branch %q already exists", req.Name)
		}
		return err
	}
	b, err := a.d.Store.GetBranch(ctx, tree.ID, req.Name)
	if err != nil {
		return err
	}
	details := map[string]string{"branch": b.Name, "tip": b.Tip}
	if req.From != "" {
		details["from"] = req.From
	}
	a.audit(r, "branch.create", t.ID, "branch", tree.ID+"/"+b.Name, details)
	a.publish(r, domain.EventBranchCreated, t.ID, tree.ID, b)
	writeJSON(w, http.StatusCreated, b)
	return nil
}

// handleDeleteBranch answers DELETE {tree}/branches/{name...}. Branch
// protection (default branch, NoDelete rules) is enforced by
// Review.CheckDeleteBranch; its *domain.PushDenied maps to 403 with the
// denial kind as the error code.
func (a *api) handleDeleteBranch(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActBranchDelete)
	if err != nil {
		return err
	}
	// Names are not re-validated: branches pushed by older clients must
	// stay deletable.
	name := r.PathValue("name")
	if name == "" {
		return badRequestf("branch name is required")
	}
	ctx := r.Context()
	if err := a.d.Review.CheckDeleteBranch(ctx, principal(r), tree, name); err != nil {
		return err
	}
	b, err := a.d.Store.GetBranch(ctx, tree.ID, name)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("branch %q not found", name)
		}
		return err
	}
	if err := a.d.Store.DeleteBranch(ctx, tree.ID, name); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("branch %q not found", name)
		}
		return err
	}
	a.audit(r, "branch.delete", t.ID, "branch", tree.ID+"/"+name, map[string]string{"branch": name, "tip": b.Tip})
	a.publish(r, eventBranchDeleted, t.ID, tree.ID, b)
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// ---- tags ----

type createTagRequest struct {
	Name    string `json:"name"`
	Target  string `json:"target"`
	Message string `json:"message"`
}

// handleCreateTag answers POST {tree}/tags.
func (a *api) handleCreateTag(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, tree, err := a.treeFor(r, domain.ActTreeWrite)
	if err != nil {
		return err
	}
	var req createTagRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := validateRefNameManage("tag", req.Name); err != nil {
		return err
	}
	if strings.TrimSpace(req.Target) == "" {
		return badRequestf("target is required")
	}
	if err := validateText("message", req.Message, maxTagMessage, false); err != nil {
		return err
	}
	ctx := r.Context()
	target, err := a.revisionID(ctx, tree, req.Target)
	if err != nil {
		return err
	}
	if target == "" {
		return badRequestf("target %q has no snapshots", req.Target)
	}
	tag := &domain.Tag{
		TreeID:    tree.ID,
		Name:      req.Name,
		Target:    target,
		Message:   req.Message,
		Tagger:    p.Username,
		CreatedAt: time.Now().UTC(),
	}
	if err := a.d.Store.PutTag(ctx, tag); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("tag %q already exists", req.Name)
		}
		return err
	}
	a.audit(r, "tag.create", t.ID, "tag", tree.ID+"/"+tag.Name, map[string]string{"tag": tag.Name, "target": tag.Target})
	a.publish(r, domain.EventTagPushed, t.ID, tree.ID, tag)
	writeJSON(w, http.StatusCreated, tag)
	return nil
}

// handleDeleteTag answers DELETE {tree}/tags/{name...}.
func (a *api) handleDeleteTag(w http.ResponseWriter, r *http.Request) error {
	t, tree, err := a.treeFor(r, domain.ActTreeAdmin)
	if err != nil {
		return err
	}
	name := r.PathValue("name")
	ctx := r.Context()
	tag, err := a.d.Store.GetTag(ctx, tree.ID, name)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("tag %q not found", name)
		}
		return err
	}
	if err := a.d.Store.DeleteTag(ctx, tree.ID, name); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return notFoundf("tag %q not found", name)
		}
		return err
	}
	a.audit(r, "tag.delete", t.ID, "tag", tree.ID+"/"+name, map[string]string{"tag": name, "target": tag.Target})
	a.publish(r, eventTagDeleted, t.ID, tree.ID, tag)
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// ---- tenants ----

type patchTenantRequest struct {
	Name *string `json:"name"`
	Plan *string `json:"plan"`
}

// handlePatchTenant answers PATCH /api/v1/tenants/{tenant}. Tenant admins
// may rename the tenant; changing the plan requires a server admin.
func (a *api) handlePatchTenant(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	t, err := a.tenantFor(r, domain.ActTenantAdmin)
	if err != nil {
		return err
	}
	var req patchTenantRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if req.Name == nil && req.Plan == nil {
		return badRequestf("at least one of name, plan is required")
	}
	ctx := r.Context()
	details := map[string]string{}
	if req.Name != nil {
		name := strings.TrimSpace(*req.Name)
		if err := validateText("name", name, 4*maxTenantNameLen, true); err != nil {
			return err
		}
		if utf8.RuneCountInString(name) > maxTenantNameLen {
			return badRequestf("name must be 1-%d characters", maxTenantNameLen)
		}
		details["name"] = name
		details["previous_name"] = t.Name
		t.Name = name
	}
	if req.Plan != nil {
		if !validPlans[*req.Plan] {
			return badRequestf("plan must be one of free, pro, enterprise")
		}
		if err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err != nil {
			if errors.Is(err, domain.ErrForbidden) {
				return forbiddenf("only server admins can change a tenant's plan")
			}
			return err
		}
		details["plan"] = *req.Plan
		details["previous_plan"] = t.Plan
		t.Plan = *req.Plan
	}
	if err := a.d.Store.UpdateTenant(ctx, t); err != nil {
		return err
	}
	a.audit(r, "tenant.update", t.ID, "tenant", t.ID, details)
	writeJSON(w, http.StatusOK, t)
	return nil
}

// ---- passwords ----

type changePasswordRequest struct {
	CurrentPassword string `json:"current_password"`
	NewPassword     string `json:"new_password"`
}

type setPasswordRequest struct {
	Password string `json:"password"`
}

// handleChangePassword answers POST /api/v1/auth/password. The current
// password is verified through Auth.Login (so failures are rate limited
// like logins); the session token that Login issues is revoked at once.
// After the change every other token of the caller is revoked; the token
// that authenticated this request stays valid.
func (a *api) handleChangePassword(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	var req changePasswordRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if req.CurrentPassword == "" {
		return badRequestf("current_password is required")
	}
	if err := iam.ValidatePassword(req.NewPassword); err != nil {
		return err
	}
	ctx := r.Context()
	acct, err := a.d.Store.GetAccount(ctx, p.AccountID)
	if err != nil {
		return err
	}
	raw, _, err := a.d.Auth.Login(ctx, acct.Username, req.CurrentPassword)
	if err != nil {
		if errors.Is(err, domain.ErrUnauthenticated) || errors.Is(err, domain.ErrNotFound) {
			a.audit(r, "auth.password_change_failed", p.TenantID, "account", p.AccountID, nil)
			// 403, not 401: the caller's session is fine, the input is not.
			return forbiddenf("current password is incorrect")
		}
		return err
	}
	// Login issued a session token as a side effect: revoke it.
	if err := a.revokeTokenByHash(ctx, p.AccountID, iam.HashToken(raw)); err != nil {
		return err
	}
	if err := a.d.Auth.SetPassword(ctx, p.AccountID, req.NewPassword); err != nil {
		return err
	}
	n, err := a.revokeAccountTokens(ctx, p.AccountID, iam.TokenHashFrom(ctx))
	if err != nil {
		return err
	}
	a.audit(r, "auth.password_change", p.TenantID, "account", p.AccountID,
		map[string]string{"revoked_tokens": strconv.Itoa(n)})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// handleAdminSetPassword answers POST /api/v1/admin/accounts/{username}/password.
// Every token of the account is revoked, except the token of this request
// when admins reset their own password.
func (a *api) handleAdminSetPassword(w http.ResponseWriter, r *http.Request) error {
	p := principal(r)
	ctx := r.Context()
	if err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	var req setPasswordRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	if err := iam.ValidatePassword(req.Password); err != nil {
		return err
	}
	username := r.PathValue("username")
	acct, err := a.d.Store.GetAccountByUsername(ctx, username)
	if errors.Is(err, domain.ErrNotFound) {
		return notFoundf("account %q not found", username)
	}
	if err != nil {
		return err
	}
	if err := a.d.Auth.SetPassword(ctx, acct.ID, req.Password); err != nil {
		return err
	}
	keep := ""
	if p != nil && p.AccountID == acct.ID {
		keep = iam.TokenHashFrom(ctx)
	}
	n, err := a.revokeAccountTokens(ctx, acct.ID, keep)
	if err != nil {
		return err
	}
	a.audit(r, "account.password_reset", acct.TenantID, "account", acct.ID,
		map[string]string{"username": acct.Username, "revoked_tokens": strconv.Itoa(n)})
	w.WriteHeader(http.StatusNoContent)
	return nil
}

// revokeAccountTokens revokes every token of accountID except the one
// whose hash is keepHash ("" keeps none) and returns how many it revoked.
// Tokens that disappear concurrently are skipped.
func (a *api) revokeAccountTokens(ctx context.Context, accountID, keepHash string) (int, error) {
	toks, err := a.d.Store.ListTokens(ctx, accountID)
	if err != nil {
		return 0, err
	}
	n := 0
	for _, t := range toks {
		if keepHash != "" && t.TokenHash == keepHash {
			continue
		}
		if err := a.d.Auth.RevokeToken(ctx, accountID, t.ID); err != nil {
			if errors.Is(err, domain.ErrNotFound) {
				continue
			}
			return n, err
		}
		n++
	}
	return n, nil
}

// revokeTokenByHash revokes accountID's token with the given hash, if any.
func (a *api) revokeTokenByHash(ctx context.Context, accountID, tokenHash string) error {
	tok, err := a.d.Store.GetTokenByHash(ctx, tokenHash)
	if errors.Is(err, domain.ErrNotFound) {
		return nil
	}
	if err != nil {
		return err
	}
	if err := a.d.Auth.RevokeToken(ctx, accountID, tok.ID); err != nil && !errors.Is(err, domain.ErrNotFound) {
		return err
	}
	return nil
}

// ---- admin accounts ----

type createAccountRequest struct {
	Username string `json:"username"`
	Email    string `json:"email"`
	Password string `json:"password"`
	IsAdmin  bool   `json:"is_admin"`
}

// accountCreator is implemented by authenticators (iam.Service) that can
// create accounts regardless of the self-service registration setting.
type accountCreator interface {
	CreateAccount(ctx context.Context, username, email, password string, isAdmin bool) (*domain.Account, error)
}

// handleAdminCreateAccount answers POST /api/v1/admin/accounts.
func (a *api) handleAdminCreateAccount(w http.ResponseWriter, r *http.Request) error {
	ctx := r.Context()
	if err := a.d.Authz.Authorize(ctx, principal(r), domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	var req createAccountRequest
	if err := decodeJSON(w, r, &req); err != nil {
		return err
	}
	req.Username = iam.NormalizeUsername(req.Username)
	req.Email = strings.TrimSpace(req.Email)
	if err := iam.ValidateUsername(req.Username); err != nil {
		return err
	}
	if err := iam.ValidateEmail(req.Email); err != nil {
		return err
	}
	if err := iam.ValidatePassword(req.Password); err != nil {
		return err
	}
	acct, err := a.createAccount(ctx, req)
	if err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return conflictf("username or email is already in use")
		}
		return err
	}
	a.audit(r, "account.create", acct.TenantID, "account", acct.ID,
		map[string]string{"username": acct.Username, "is_admin": strconv.FormatBool(acct.IsAdmin)})
	writeJSON(w, http.StatusCreated, acct)
	return nil
}

// createAccount creates the account through the Authenticator. Without
// accountCreator support it falls back to Register (subject to that
// implementation's registration policy) and then sets IsAdmin.
func (a *api) createAccount(ctx context.Context, req createAccountRequest) (*domain.Account, error) {
	if c, ok := a.d.Auth.(accountCreator); ok {
		return c.CreateAccount(ctx, req.Username, req.Email, req.Password, req.IsAdmin)
	}
	acct, err := a.d.Auth.Register(ctx, req.Username, req.Email, req.Password)
	if err != nil {
		return nil, err
	}
	if req.IsAdmin && !acct.IsAdmin {
		acct.IsAdmin = true
		if err := a.d.Store.UpdateAccount(ctx, acct); err != nil {
			return nil, err
		}
	}
	return acct, nil
}

// ---- system info ----

type systemMemory struct {
	AllocBytes     uint64 `json:"alloc_bytes"`
	SysBytes       uint64 `json:"sys_bytes"`
	HeapInuseBytes uint64 `json:"heap_inuse_bytes"`
	GCRuns         uint32 `json:"gc_runs"`
}

type systemStorage struct {
	Objects     int64 `json:"objects"`
	ObjectBytes int64 `json:"object_bytes"`
	MetaDBBytes int64 `json:"meta_db_bytes"`
}

type systemInfo struct {
	Version           string        `json:"version"`
	GoVersion         string        `json:"go_version"`
	OS                string        `json:"os"`
	Arch              string        `json:"arch"`
	StartedAt         time.Time     `json:"started_at"`
	UptimeSeconds     int64         `json:"uptime_seconds"`
	AllowRegistration bool          `json:"allow_registration"`
	DataDir           string        `json:"data_dir"`
	NumCPU            int           `json:"num_cpu"`
	NumGoroutine      int           `json:"num_goroutine"`
	Memory            systemMemory  `json:"memory"`
	Storage           systemStorage `json:"storage"`
	EventBus          events.Stats  `json:"event_bus"`
}

// busStatter is implemented by event buses that expose counters
// (events.Bus).
type busStatter interface {
	Stats() events.Stats
}

// handleAdminSystem answers GET /api/v1/admin/system.
func (a *api) handleAdminSystem(w http.ResponseWriter, r *http.Request) error {
	ctx := r.Context()
	if err := a.d.Authz.Authorize(ctx, principal(r), domain.ActServerAdmin, "", nil); err != nil {
		return err
	}
	var ms runtime.MemStats
	runtime.ReadMemStats(&ms)
	info := systemInfo{
		Version:           a.d.Version,
		GoVersion:         runtime.Version(),
		OS:                runtime.GOOS,
		Arch:              runtime.GOARCH,
		StartedAt:         a.d.StartedAt.UTC(),
		UptimeSeconds:     int64(time.Since(a.d.StartedAt).Seconds()),
		AllowRegistration: a.d.AllowRegistration,
		DataDir:           a.d.DataDir,
		NumCPU:            runtime.NumCPU(),
		NumGoroutine:      runtime.NumGoroutine(),
		Memory: systemMemory{
			AllocBytes:     ms.Alloc,
			SysBytes:       ms.Sys,
			HeapInuseBytes: ms.HeapInuse,
			GCRuns:         ms.NumGC,
		},
	}
	if a.d.DataDir != "" {
		if abs, err := filepath.Abs(a.d.DataDir); err == nil {
			info.DataDir = abs
		}
		if fi, err := os.Stat(filepath.Join(a.d.DataDir, "meta.db")); err == nil && fi.Mode().IsRegular() {
			info.Storage.MetaDBBytes = fi.Size()
		}
	}
	if s, ok := a.d.Objects.(objectStatter); ok {
		n, b, err := s.Stats(ctx)
		if err != nil {
			return err
		}
		info.Storage.Objects, info.Storage.ObjectBytes = n, b
	}
	if bs, ok := a.d.Bus.(busStatter); ok {
		info.EventBus = bs.Stats()
	}
	w.Header().Set("Cache-Control", "no-store")
	writeJSON(w, http.StatusOK, info)
	return nil
}

// ---- search ----

type searchTreeHit struct {
	Tree       domain.Tree `json:"tree"`
	TenantSlug string      `json:"tenant_slug"`
}

type searchBranchHit struct {
	TenantSlug string        `json:"tenant_slug"`
	Tree       string        `json:"tree"`
	Branch     domain.Branch `json:"branch"`
}

type searchResults struct {
	Tenants  []domain.Tenant   `json:"tenants"`
	Trees    []searchTreeHit   `json:"trees"`
	Branches []searchBranchHit `json:"branches"`
	Accounts []domain.Account  `json:"accounts"`
}

// handleSearch answers GET /api/v1/search?q=&limit=20: a case-insensitive
// substring search over tenant slugs/names, tree names/descriptions,
// branch names and (server admins only) account usernames/emails. The
// scope is the tenants the caller belongs to (every tenant for server
// admins) and, within them, the trees the caller may read. Each category
// holds at most limit hits, in slug / name order.
func (a *api) handleSearch(w http.ResponseWriter, r *http.Request) error {
	p, err := requireLogin(r)
	if err != nil {
		return err
	}
	q := strings.TrimSpace(r.URL.Query().Get("q"))
	if q == "" {
		return badRequestf("query parameter q is required")
	}
	if !utf8.ValidString(q) || utf8.RuneCountInString(q) > maxSearchQuery {
		return badRequestf("q must be 1-%d characters", maxSearchQuery)
	}
	limit, err := queryInt(r, "limit", defaultSearchMax, maxSearchLimit)
	if err != nil {
		return err
	}
	ctx := r.Context()
	needle := strings.ToLower(q)
	match := func(fields ...string) bool {
		for _, f := range fields {
			if strings.Contains(strings.ToLower(f), needle) {
				return true
			}
		}
		return false
	}
	res := searchResults{
		Tenants:  []domain.Tenant{},
		Trees:    []searchTreeHit{},
		Branches: []searchBranchHit{},
		Accounts: []domain.Account{},
	}

	tenants, err := a.searchScopeTenants(ctx, p)
	if err != nil {
		return err
	}
	for _, t := range tenants {
		if len(res.Tenants) < limit && match(t.Slug, t.Name) {
			res.Tenants = append(res.Tenants, t)
		}
	}
	for i := range tenants {
		if len(res.Trees) >= limit && len(res.Branches) >= limit {
			break
		}
		t := &tenants[i]
		trees, err := a.d.Store.ListTrees(ctx, t.ID)
		if err != nil {
			return err
		}
		sort.Slice(trees, func(i, j int) bool { return trees[i].Name < trees[j].Name })
		for j := range trees {
			if len(res.Trees) >= limit && len(res.Branches) >= limit {
				break
			}
			tree := &trees[j]
			if err := a.d.Authz.Authorize(ctx, p, domain.ActTreeRead, t.ID, tree); err != nil {
				if isDenied(err) {
					continue
				}
				return err
			}
			if len(res.Trees) < limit && match(tree.Name, tree.Description) {
				res.Trees = append(res.Trees, searchTreeHit{Tree: *tree, TenantSlug: t.Slug})
			}
			if len(res.Branches) >= limit {
				continue
			}
			bs, err := a.d.Store.ListBranches(ctx, tree.ID)
			if err != nil {
				return err
			}
			sort.Slice(bs, func(i, j int) bool { return bs[i].Name < bs[j].Name })
			for _, b := range bs {
				if len(res.Branches) >= limit {
					break
				}
				if match(b.Name) {
					res.Branches = append(res.Branches, searchBranchHit{TenantSlug: t.Slug, Tree: tree.Name, Branch: b})
				}
			}
		}
	}

	if err := a.d.Authz.Authorize(ctx, p, domain.ActServerAdmin, "", nil); err == nil {
		accts, err := a.d.Store.ListAccounts(ctx)
		if err != nil {
			return err
		}
		sort.Slice(accts, func(i, j int) bool { return accts[i].Username < accts[j].Username })
		for _, acct := range accts {
			if len(res.Accounts) >= limit {
				break
			}
			if match(acct.Username, acct.Email) {
				res.Accounts = append(res.Accounts, acct)
			}
		}
	} else if !isDenied(err) {
		return err
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// searchScopeTenants returns the tenants a search by p covers, sorted by
// slug: every tenant for server admins, otherwise p's memberships.
func (a *api) searchScopeTenants(ctx context.Context, p *domain.Principal) ([]domain.Tenant, error) {
	var out []domain.Tenant
	if p.IsAdmin {
		ts, err := a.d.Store.ListTenants(ctx)
		if err != nil {
			return nil, err
		}
		out = ts
	} else {
		ms, err := a.d.Store.ListAccountMemberships(ctx, p.AccountID)
		if err != nil {
			return nil, err
		}
		for _, m := range ms {
			t, err := a.d.Store.GetTenant(ctx, m.TenantID)
			if errors.Is(err, domain.ErrNotFound) {
				continue // dangling membership
			}
			if err != nil {
				return nil, err
			}
			out = append(out, *t)
		}
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Slug < out[j].Slug })
	return out, nil
}
