package httpapi

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// Tenant lifecycle routes (§16.4) against a real meta.Memory store, the
// real authorizer and the real iam.TenantLifecycle.

type lcAuth struct {
	domain.Authenticator
	users map[string]*domain.Principal
}

func (a *lcAuth) Authenticate(_ context.Context, token string) (*domain.Principal, error) {
	if p := a.users[token]; p != nil {
		cp := *p
		return &cp, nil
	}
	return nil, domain.ErrUnauthenticated
}

type lcMailer struct {
	mu   sync.Mutex
	text []string
}

func (m *lcMailer) SendMail(_ context.Context, _, _, _, text, _ string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.text = append(m.text, text)
	return true
}

var lcCodeRe = regexp.MustCompile(`wte_[A-Za-z0-9_-]{43}`)

func (m *lcMailer) lastCode(t *testing.T) string {
	t.Helper()
	m.mu.Lock()
	defer m.mu.Unlock()
	if len(m.text) == 0 {
		t.Fatal("no email sent")
	}
	c := lcCodeRe.FindString(m.text[len(m.text)-1])
	if c == "" {
		t.Fatalf("no code in %q", m.text[len(m.text)-1])
	}
	return c
}

type lcFixture struct {
	store  *meta.Memory
	az     *authz.Authorizer
	mailer *lcMailer
	audit  *fakeAudit
	h      http.Handler
}

func newLCFixture(t *testing.T, withLifecycle bool) *lcFixture {
	t.Helper()
	ctx := context.Background()
	st := meta.NewMemory()
	f := &lcFixture{store: st, az: authz.New(st, authz.Options{}), mailer: &lcMailer{}, audit: &fakeAudit{}}
	users := map[string]*domain.Principal{}
	now := time.Now().UTC()
	tenant := func(id, slug, kind string) {
		must(t, st.CreateTenant(ctx, &domain.Tenant{ID: id, Slug: slug, Name: slug, Kind: kind, Plan: "free", CreatedAt: now}))
	}
	tenant("t-acme", "acme", domain.TenantOrg)
	tenant("t-partner", "partner", domain.TenantOrg)
	for _, u := range []struct{ name, acmeRole, partnerRole string }{
		{"olga", domain.RoleOwner, ""}, {"adam", domain.RoleAdmin, ""}, {"dave", domain.RoleDeveloper, ""},
		{"pete", "", domain.RoleOwner}, {"root", "", ""},
	} {
		tenant("t-"+u.name, u.name, domain.TenantUser)
		acct := &domain.Account{ID: "a-" + u.name, Username: u.name, Email: u.name + "@example.com", TenantID: "t-" + u.name, IsAdmin: u.name == "root", CreatedAt: now}
		must(t, st.CreateAccount(ctx, acct))
		must(t, st.PutMembership(ctx, &domain.Membership{TenantID: "t-" + u.name, AccountID: acct.ID, Role: domain.RoleOwner, CreatedAt: now}))
		if u.acmeRole != "" {
			must(t, st.PutMembership(ctx, &domain.Membership{TenantID: "t-acme", AccountID: acct.ID, Role: u.acmeRole, CreatedAt: now}))
		}
		if u.partnerRole != "" {
			must(t, st.PutMembership(ctx, &domain.Membership{TenantID: "t-partner", AccountID: acct.ID, Role: u.partnerRole, CreatedAt: now}))
		}
		users[u.name] = &domain.Principal{AccountID: acct.ID, Username: u.name, TenantID: acct.TenantID, IsAdmin: acct.IsAdmin, Kind: domain.PrincipalUser}
	}
	must(t, st.CreateTree(ctx, &domain.Tree{ID: "tr-web", TenantID: "t-partner", Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate, CreatedAt: now, UpdatedAt: now}))
	log := slog.New(slog.NewTextHandler(io.Discard, nil))
	d := Deps{Store: st, Auth: &lcAuth{users: users}, Authz: f.az, Access: f.az, Audit: f.audit, Logger: log}
	if withLifecycle {
		d.TenantLifecycle = iam.NewTenantLifecycle(iam.LifecycleDeps{Store: st, Mailer: f.mailer, Logger: log}, iam.LifecycleOptions{})
	}
	f.h = New(d)
	return f
}

func (f *lcFixture) do(t *testing.T, method, target, as string, body any) *httptest.ResponseRecorder {
	t.Helper()
	var rd io.Reader
	if body != nil {
		buf, err := json.Marshal(body)
		if err != nil {
			t.Fatal(err)
		}
		rd = bytes.NewReader(buf)
	}
	req := httptest.NewRequest(method, target, rd)
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}
	if as != "" {
		req.Header.Set("Authorization", "Bearer "+as)
	}
	rec := httptest.NewRecorder()
	f.h.ServeHTTP(rec, req)
	return rec
}

func (f *lcFixture) want(t *testing.T, method, target, as string, body any, status int) map[string]any {
	t.Helper()
	rec := f.do(t, method, target, as, body)
	if rec.Code != status {
		t.Fatalf("%s %s as %q: %d, want %d: %s", method, target, as, rec.Code, status, rec.Body.String())
	}
	out := map[string]any{}
	_ = json.Unmarshal(rec.Body.Bytes(), &out)
	return out
}

func lcErrCode(m map[string]any) string {
	e, _ := m["error"].(map[string]any)
	s, _ := e["code"].(string)
	return s
}

func TestTenantLifecycleEmailRoutes(t *testing.T) {
	f := newLCFixture(t, true)
	// Only tenant admins set the email; bad addresses are refused.
	f.want(t, "PATCH", "/api/v1/tenants/acme", "dave", map[string]any{"email": "ops@acme.io"}, 403)
	f.want(t, "PATCH", "/api/v1/tenants/acme", "adam", map[string]any{"email": "nope"}, 400)
	got := f.want(t, "PATCH", "/api/v1/tenants/acme", "adam", map[string]any{"email": "ops@acme.io"}, 200)
	if got["email"] != "ops@acme.io" || got["email_verified_at"] != nil {
		t.Fatalf("patched = %v", got)
	}
	code := f.mailer.lastCode(t)
	m := f.want(t, "POST", "/api/v1/tenants/acme/email/verify", "", map[string]any{"token": "wte_bogus"}, 400)
	if lcErrCode(m) != iam.CodeInvalidEmailToken {
		t.Fatalf("bogus code: %v", m)
	}
	f.want(t, "POST", "/api/v1/tenants/acme/email/verify", "", map[string]any{}, 400)
	got = f.want(t, "POST", "/api/v1/tenants/acme/email/verify", "", map[string]any{"token": code}, 200)
	if got["email_verified_at"] == nil {
		t.Fatalf("verified = %v", got)
	}
	m = f.want(t, "POST", "/api/v1/tenants/acme/email/resend", "adam", nil, 409)
	if lcErrCode(m) != iam.CodeEmailVerified {
		t.Fatalf("resend verified: %v", m)
	}
	// A verified email names the tenant: path references and grantees.
	f.want(t, "GET", "/api/v1/tenants/ops@acme.io", "dave", nil, 200)
	g := f.want(t, "PUT", "/api/v1/trees/partner/web/grants", "pete",
		map[string]any{"subject_type": "tenant", "subject": "ops@acme.io", "role": "viewer"}, 200)
	if g["subject"] != "acme" || g["subject_id"] != "t-acme" {
		t.Fatalf("grant = %v", g)
	}
	// Account email verification.
	f.want(t, "POST", "/api/v1/me/email/resend", "", nil, 401)
	sent := f.want(t, "POST", "/api/v1/me/email/resend", "dave", nil, 202)
	if sent["email"] != "dave@example.com" || sent["delivery"] != iam.DeliveryEmail {
		t.Fatalf("sent = %v", sent)
	}
	acct := f.want(t, "POST", "/api/v1/me/email/verify", "dave", map[string]any{"token": f.mailer.lastCode(t)}, 200)
	if acct["email_verified_at"] == nil || acct["username"] != "dave" {
		t.Fatalf("account = %v", acct)
	}
	if !auditHas(f.audit, "tenant.email") || !auditHas(f.audit, "tenant.email_verified") || !auditHas(f.audit, "account.email_verified") {
		t.Fatalf("audit = %+v", f.audit.events)
	}
}

func TestTenantLifecycleRenameRoutes(t *testing.T) {
	f := newLCFixture(t, true)
	f.want(t, "POST", "/api/v1/tenants/acme/rename", "dave", map[string]any{"slug": "acme-inc"}, 403)
	f.want(t, "POST", "/api/v1/tenants/acme/rename", "adam", map[string]any{"slug": "acme-inc"}, 403) // admin, not owner
	f.want(t, "POST", "/api/v1/tenants/acme/rename", "olga", map[string]any{"slug": "Bad Slug"}, 400)
	m := f.want(t, "POST", "/api/v1/tenants/acme/rename", "olga", map[string]any{"slug": "partner"}, 409)
	if lcErrCode(m) != iam.CodeSlugTaken {
		t.Fatalf("taken: %v", m)
	}
	m = f.want(t, "POST", "/api/v1/tenants/olga/rename", "olga", map[string]any{"slug": "olga2"}, 409)
	if lcErrCode(m) != iam.CodePersonalTenant {
		t.Fatalf("personal: %v", m)
	}
	out := f.want(t, "POST", "/api/v1/tenants/acme/rename", "olga", map[string]any{"slug": "acme-inc"}, 200)
	if ten, _ := out["tenant"].(map[string]any); ten["slug"] != "acme-inc" {
		t.Fatalf("renamed = %v", out)
	}
	if res, _ := out["reservation"].(map[string]any); res["slug"] != "acme" || res["renamed_to"] != "acme-inc" {
		t.Fatalf("reservation = %v", out)
	}
	// The old slug answers 301 tenant_moved with the new slug.
	rec := f.do(t, "GET", "/api/v1/tenants/acme/members", "dave", nil)
	if rec.Code != http.StatusMovedPermanently || rec.Header().Get("X-WT-Tenant-Moved") != "acme-inc" ||
		rec.Header().Get("Location") != "" || !strings.Contains(rec.Body.String(), `"new_slug":"acme-inc"`) ||
		!strings.Contains(rec.Body.String(), `"code":"tenant_moved"`) {
		t.Fatalf("old slug: %d %v %s", rec.Code, rec.Header(), rec.Body.String())
	}
	f.want(t, "GET", "/api/v1/tenants/acme-inc", "dave", nil, 200)
	// Nobody else can claim the old slug.
	m = f.want(t, "POST", "/api/v1/tenants", "dave", map[string]any{"slug": "acme"}, 409)
	if lcErrCode(m) != iam.CodeSlugReserved {
		t.Fatalf("reserved: %v", m)
	}
	// Unknown slugs stay 404.
	f.want(t, "GET", "/api/v1/tenants/nobody", "dave", nil, 404)
	if !auditHas(f.audit, "tenant.rename") {
		t.Fatal("rename not audited")
	}
}

func TestTenantLifecycleDeleteRestoreRoutes(t *testing.T) {
	f := newLCFixture(t, true)
	ctx := context.Background()
	sa := &domain.ServiceAccount{TenantID: "t-acme", Name: "ci", Role: domain.RoleDeveloper}
	must(t, f.store.CreateServiceAccount(ctx, sa))
	must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "k1", ServiceAccountID: sa.ID, Kind: domain.TokenService, Name: "k", TokenHash: "h", CreatedAt: time.Now()}))

	f.want(t, "GET", "/api/v1/tenants/acme/deletion", "dave", nil, 404)
	f.want(t, "DELETE", "/api/v1/tenants/acme", "adam", nil, 403)
	f.want(t, "DELETE", "/api/v1/tenants/olga", "olga", nil, 409)
	d := f.want(t, "DELETE", "/api/v1/tenants/acme", "olga", nil, 202)
	if d["slug"] != "acme" || d["revoked_keys"] != float64(1) || d["purge_at"] == nil {
		t.Fatalf("deletion = %v", d)
	}
	f.want(t, "DELETE", "/api/v1/tenants/acme", "olga", nil, 409)
	got := f.want(t, "GET", "/api/v1/tenants/acme/deletion", "dave", nil, 200)
	if got["purge_at"] != d["purge_at"] {
		t.Fatalf("deletion = %v", got)
	}
	// Suspended: reads work, writes do not.
	if ten := f.want(t, "GET", "/api/v1/tenants/acme", "dave", nil, 200); ten["status"] != domain.TenantSuspended {
		t.Fatalf("tenant = %v", ten)
	}
	f.want(t, "PATCH", "/api/v1/tenants/acme", "olga", map[string]any{"name": "Acme"}, 403)
	f.want(t, "POST", "/api/v1/tenants/acme/restore", "dave", nil, 403)
	restored := f.want(t, "POST", "/api/v1/tenants/acme/restore", "olga", nil, 200)
	if restored["status"] != nil {
		t.Fatalf("restored = %v", restored)
	}
	f.want(t, "POST", "/api/v1/tenants/acme/restore", "olga", nil, 404)
	// Server admins may delete too.
	f.want(t, "DELETE", "/api/v1/tenants/acme", "root", nil, 202)
	f.want(t, "POST", "/api/v1/tenants/acme/restore", "root", nil, 200)
	if !auditHas(f.audit, "tenant.delete") || !auditHas(f.audit, "tenant.restore") {
		t.Fatal("not audited")
	}
}

func TestSuspensionRevokesKeysAndPausesTenantGrants(t *testing.T) {
	f := newLCFixture(t, true)
	ctx := context.Background()
	sa := &domain.ServiceAccount{TenantID: "t-acme", Name: "ci", Role: domain.RoleDeveloper}
	must(t, f.store.CreateServiceAccount(ctx, sa))
	must(t, f.store.CreateToken(ctx, &domain.APIToken{ID: "k1", ServiceAccountID: sa.ID, Kind: domain.TokenService, Name: "k", TokenHash: "h", CreatedAt: time.Now()}))
	f.want(t, "PUT", "/api/v1/trees/partner/web/grants", "pete", map[string]any{"subject_type": "tenant", "subject": "acme", "role": "viewer"}, 200)
	dave := &domain.Principal{AccountID: "a-dave", Username: "dave", Kind: domain.PrincipalUser}
	tree, err := f.store.GetTree(ctx, "tr-web")
	must(t, err)
	if err := f.az.Authorize(ctx, dave, domain.ActTreeRead, "t-partner", tree); err != nil {
		t.Fatalf("tenant grant before suspension: %v", err)
	}
	f.want(t, "PATCH", "/api/v1/admin/tenants/acme", "root", map[string]any{"status": "suspended", "status_reason": "unpaid"}, 200)
	if keys, _ := f.store.ListServiceAccountTokens(ctx, sa.ID); len(keys) != 0 {
		t.Fatalf("keys after suspension: %+v", keys)
	}
	if err := f.az.Authorize(ctx, dave, domain.ActTreeRead, "t-partner", tree); err == nil {
		t.Fatal("tenant grant of a suspended tenant still applies")
	}
	d, err := f.az.Evaluate(ctx, dave, domain.ActTreeRead, domain.Resource{TenantID: "t-partner", Tree: tree}, domain.RequestEnv{})
	must(t, err)
	if !strings.Contains(reasonsText(d), "paused") {
		t.Fatalf("decision reasons = %+v", d.Reasons)
	}
	f.want(t, "PATCH", "/api/v1/admin/tenants/acme", "root", map[string]any{"status": "active"}, 200)
	if err := f.az.Authorize(ctx, dave, domain.ActTreeRead, "t-partner", tree); err != nil {
		t.Fatalf("tenant grant after resume: %v", err)
	}
}

func auditHas(a *fakeAudit, action string) bool {
	a.mu.Lock()
	defer a.mu.Unlock()
	for _, e := range a.events {
		if e.Action == action {
			return true
		}
	}
	return false
}

func reasonsText(d domain.Decision) string {
	var b strings.Builder
	for _, r := range d.Reasons {
		b.WriteString(r.Detail + "\n")
	}
	return b.String()
}

func TestTenantLifecycleRoutesWithoutService(t *testing.T) {
	f := newLCFixture(t, false)
	f.want(t, "POST", "/api/v1/tenants/acme/rename", "olga", map[string]any{"slug": "x"}, 501)
	f.want(t, "PATCH", "/api/v1/tenants/acme", "olga", map[string]any{"email": "a@b.io"}, 501)
	f.want(t, "DELETE", "/api/v1/tenants/acme", "olga", nil, 501)
	f.want(t, "PATCH", "/api/v1/tenants/acme", "olga", map[string]any{"name": "Acme"}, 200)
}
