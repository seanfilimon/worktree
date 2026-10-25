package httpapi

import (
	"context"
	"crypto/rand"
	"errors"
	"fmt"
	"net"
	"net/http"
	"path"
	"regexp"
	"strconv"
	"strings"
	"time"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// principal returns the authenticated caller, or nil for anonymous.
func principal(r *http.Request) *domain.Principal { return iam.PrincipalFrom(r.Context()) }

// requireLogin returns the caller or errLoginRequired.
func requireLogin(r *http.Request) (*domain.Principal, error) {
	p := principal(r)
	if p == nil {
		return nil, errLoginRequired
	}
	return p, nil
}

// audit records an audit event attributed to the caller of r.
func (a *api) audit(r *http.Request, action, tenantID, resourceType, resourceID string, details map[string]string) {
	if a.d.Audit == nil {
		return
	}
	e := domain.AuditEvent{
		ID:           newID(),
		TenantID:     tenantID,
		Actor:        "anonymous",
		Action:       action,
		ResourceType: resourceType,
		ResourceID:   resourceID,
		Details:      details,
		RemoteAddr:   remoteHost(r),
		At:           time.Now().UTC(),
	}
	if p := principal(r); p != nil {
		e.ActorID = p.AccountID
		e.Actor = p.Username
	}
	a.d.Audit.Record(context.WithoutCancel(r.Context()), e)
}

// publish emits an event on the bus, if one is configured.
func (a *api) publish(r *http.Request, typ, tenantID, treeID string, payload any) {
	if a.d.Bus == nil {
		return
	}
	e := domain.Event{Type: typ, TenantID: tenantID, TreeID: treeID, Payload: payload, At: time.Now().UTC()}
	if p := principal(r); p != nil {
		e.Actor = p.Username
	}
	a.d.Bus.Publish(e)
}

func remoteHost(r *http.Request) string {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		return r.RemoteAddr
	}
	return host
}

// newID returns a random RFC 4122 version 4 UUID string.
func newID() string {
	var b [16]byte
	_, _ = rand.Read(b[:])
	b[6] = b[6]&0x0f | 0x40
	b[8] = b[8]&0x3f | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}

// tenantByRef resolves a {tenant} path value: a slug, or a tenant id.
func (a *api) tenantByRef(ctx context.Context, ref string) (*domain.Tenant, error) {
	t, err := a.d.Store.GetTenantBySlug(ctx, ref)
	if err == nil {
		return t, nil
	}
	if !errors.Is(err, domain.ErrNotFound) {
		return nil, err
	}
	if t, err2 := a.d.Store.GetTenant(ctx, ref); err2 == nil {
		return t, nil
	}
	return nil, notFoundf("tenant %q not found", ref)
}

// tenantFor resolves the {tenant} path value and authorizes act on it.
func (a *api) tenantFor(r *http.Request, act domain.Action) (*domain.Tenant, error) {
	t, err := a.tenantByRef(r.Context(), r.PathValue("tenant"))
	if err != nil {
		return nil, err
	}
	if err := a.d.Authz.Authorize(r.Context(), principal(r), act, t.ID, nil); err != nil {
		return nil, err
	}
	return t, nil
}

// treeFor resolves the {tenant}/{tree} path values and authorizes act on
// the tree.
func (a *api) treeFor(r *http.Request, act domain.Action) (*domain.Tenant, *domain.Tree, error) {
	ctx := r.Context()
	t, err := a.tenantByRef(ctx, r.PathValue("tenant"))
	if err != nil {
		return nil, nil, err
	}
	name := r.PathValue("tree")
	tree, err := a.d.Store.GetTreeByName(ctx, t.ID, name)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil, notFoundf("tree %s/%s not found", t.Slug, name)
		}
		return nil, nil, err
	}
	if err := a.d.Authz.Authorize(ctx, principal(r), act, t.ID, tree); err != nil {
		return nil, nil, err
	}
	return t, tree, nil
}

// isDenied reports whether err is an authorization refusal (as opposed to
// an operational failure).
func isDenied(err error) bool {
	return errors.Is(err, domain.ErrForbidden) || errors.Is(err, domain.ErrUnauthenticated)
}

// queryInt parses an optional positive integer query parameter, clamping
// it to max.
func queryInt(r *http.Request, name string, def, max int) (int, error) {
	s := r.URL.Query().Get(name)
	if s == "" {
		return def, nil
	}
	n, err := strconv.Atoi(s)
	if err != nil || n < 1 {
		return 0, badRequestf("%s must be a positive integer", name)
	}
	if n > max {
		n = max
	}
	return n, nil
}

// ---- validation ----

var (
	slugRe     = regexp.MustCompile(`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`)
	treeNameRe = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]{0,99}$`)
)

func validateSlug(slug string) error {
	if !slugRe.MatchString(slug) {
		return badRequestf("slug must be 1-63 characters of lowercase letters, digits and '-', not starting or ending with '-'")
	}
	return nil
}

func validateTreeName(name string) error {
	if !treeNameRe.MatchString(name) || strings.HasSuffix(name, ".") {
		return badRequestf("tree name must be 1-100 characters of letters, digits, '.', '_' and '-', starting with a letter or digit")
	}
	return nil
}

// validateBranchName applies conservative, Git-like ref-name rules.
func validateBranchName(name string) error {
	switch {
	case name == "" || len(name) > 255:
		return badRequestf("branch name must be 1-255 bytes")
	case !utf8.ValidString(name):
		return badRequestf("branch name must be valid UTF-8")
	case strings.HasPrefix(name, "/") || strings.HasSuffix(name, "/") || strings.Contains(name, "//"):
		return badRequestf("branch name must not start or end with '/' or contain '//'")
	case strings.Contains(name, "..") || strings.HasPrefix(name, "-") || strings.HasSuffix(name, "."):
		return badRequestf("branch name must not contain '..', start with '-' or end with '.'")
	case strings.ContainsFunc(name, func(r rune) bool {
		return unicode.IsSpace(r) || unicode.IsControl(r) || strings.ContainsRune(`~^:?*[\`, r)
	}):
		return badRequestf("branch name contains a forbidden character")
	}
	return nil
}

func validateVisibility(v string) error {
	switch v {
	case domain.VisibilityPrivate, domain.VisibilityInternal, domain.VisibilityPublic:
		return nil
	}
	return badRequestf("visibility must be one of %q, %q, %q", domain.VisibilityPrivate, domain.VisibilityInternal, domain.VisibilityPublic)
}

// validateText bounds a free-text field.
func validateText(field, s string, max int, required bool) error {
	if required && strings.TrimSpace(s) == "" {
		return badRequestf("%s is required", field)
	}
	if len(s) > max {
		return badRequestf("%s must be at most %d bytes", field, max)
	}
	if !utf8.ValidString(s) {
		return badRequestf("%s must be valid UTF-8", field)
	}
	return nil
}

// validateGlob checks a '/'-separated glob pattern.
func validateGlob(field, pattern string) error {
	if err := validateText(field, pattern, 512, true); err != nil {
		return err
	}
	if _, err := path.Match(pattern, ""); err != nil {
		return badRequestf("%s is not a valid glob pattern", field)
	}
	return nil
}
