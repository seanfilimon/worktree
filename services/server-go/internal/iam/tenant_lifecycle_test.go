package iam

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"regexp"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// GetDoc lets the shared fake store answer slug-reservation lookups made
// by registration (tenant-lifecycle, §16.4): it holds no documents.
func (f *fakeStore) GetDoc(context.Context, string, string, any) error {
	return domain.ErrNotFound
}

type sentMail struct{ to, subject, text, html string }

type fakeMailer struct {
	mu   sync.Mutex
	sent []sentMail
}

func (m *fakeMailer) SendMail(_ context.Context, to, _, subject, text, html string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.sent = append(m.sent, sentMail{to, subject, text, html})
	return true
}

func (m *fakeMailer) last(t *testing.T) sentMail {
	t.Helper()
	m.mu.Lock()
	defer m.mu.Unlock()
	if len(m.sent) == 0 {
		t.Fatal("no email sent")
	}
	return m.sent[len(m.sent)-1]
}

type recBus struct {
	mu     sync.Mutex
	events []domain.Event
}

func (b *recBus) Publish(e domain.Event) {
	b.mu.Lock()
	defer b.mu.Unlock()
	b.events = append(b.events, e)
}

func (b *recBus) Subscribe(domain.EventFilter) (<-chan domain.Event, func()) {
	return make(chan domain.Event), func() {}
}

func (b *recBus) types() []string {
	b.mu.Lock()
	defer b.mu.Unlock()
	var out []string
	for _, e := range b.events {
		out = append(out, e.Type)
	}
	return out
}

type recAudit struct {
	mu     sync.Mutex
	events []domain.AuditEvent
}

func (a *recAudit) Record(_ context.Context, e domain.AuditEvent) {
	a.mu.Lock()
	defer a.mu.Unlock()
	a.events = append(a.events, e)
}

func (a *recAudit) Query(context.Context, domain.AuditFilter) ([]domain.AuditEvent, error) {
	return nil, nil
}

var codeRe = regexp.MustCompile(`wte_[A-Za-z0-9_-]{43}`)

type lcEnv struct {
	ctx    context.Context
	store  *meta.Memory
	clock  *fakeClock
	mailer *fakeMailer
	bus    *recBus
	audit  *recAudit
	l      *TenantLifecycle
	acme   *domain.Tenant
	owner  *domain.Account
}

func newLCEnv(t *testing.T, withMailer bool, logs *bytes.Buffer) *lcEnv {
	t.Helper()
	e := &lcEnv{ctx: context.Background(), store: meta.NewMemory(), clock: newFakeClock(), bus: &recBus{}, audit: &recAudit{}}
	d := LifecycleDeps{Store: e.store, Bus: e.bus, Audit: e.audit}
	if withMailer {
		e.mailer = &fakeMailer{}
		d.Mailer = e.mailer
	}
	if logs != nil {
		d.Logger = slog.New(slog.NewTextHandler(logs, nil))
	}
	e.l = NewTenantLifecycle(d, LifecycleOptions{DeletionWindow: 48 * time.Hour, PublicURL: "https://wt.example.com/", Clock: e.clock.Now})
	e.acme = &domain.Tenant{ID: "t-acme", Slug: "acme", Name: "Acme", Kind: domain.TenantOrg, Plan: "free", CreatedAt: e.clock.Now()}
	e.must(t, e.store.CreateTenant(e.ctx, e.acme))
	e.owner = &domain.Account{ID: "a-olga", Username: "olga", Email: "olga@example.com", TenantID: "t-olga", CreatedAt: e.clock.Now()}
	e.must(t, e.store.CreateTenant(e.ctx, &domain.Tenant{ID: "t-olga", Slug: "olga", Name: "olga", Kind: domain.TenantUser, CreatedAt: e.clock.Now()}))
	e.must(t, e.store.CreateAccount(e.ctx, e.owner))
	e.must(t, e.store.PutMembership(e.ctx, &domain.Membership{TenantID: e.acme.ID, AccountID: e.owner.ID, Role: domain.RoleOwner, CreatedAt: e.clock.Now()}))
	return e
}

func (e *lcEnv) must(t *testing.T, err error) {
	t.Helper()
	if err != nil {
		t.Fatal(err)
	}
}

func (e *lcEnv) tenant(t *testing.T, id string) *domain.Tenant {
	t.Helper()
	ten, err := e.store.GetTenant(e.ctx, id)
	e.must(t, err)
	return ten
}

func wantCode(t *testing.T, err error, code string) {
	t.Helper()
	var le *LifecycleError
	if !errors.As(err, &le) || le.Code != code {
		t.Fatalf("error = %v, want code %s", err, code)
	}
}

func TestTenantEmailVerification(t *testing.T) {
	e := newLCEnv(t, true, nil)
	out, sent, err := e.l.SetTenantEmail(e.ctx, "olga", e.acme, "ops@acme.io")
	e.must(t, err)
	if out.Email != "ops@acme.io" || out.EmailVerifiedAt != nil || sent == nil || sent.Delivery != DeliveryEmail {
		t.Fatalf("SetTenantEmail = %+v, %+v", out, sent)
	}
	if !sent.ExpiresAt.Equal(e.clock.Now().UTC().Add(EmailTokenTTL)) {
		t.Fatalf("expires_at = %s", sent.ExpiresAt)
	}
	mail := e.mailer.last(t)
	code := codeRe.FindString(mail.text)
	if mail.to != "ops@acme.io" || code == "" || !strings.Contains(mail.subject, "acme") ||
		!strings.Contains(mail.text, "https://wt.example.com/admin/tenants/acme?tab=settings&email_token=") ||
		!strings.Contains(mail.html, code) {
		t.Fatalf("mail = %+v", mail)
	}
	// Unverified addresses never resolve.
	if _, err := TenantByVerifiedEmail(e.ctx, e.store, "ops@acme.io"); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("unverified email resolved: %v", err)
	}
	// Garbage and wrong codes are refused with one error.
	_, err = e.l.VerifyTenantEmail(e.ctx, e.acme, "wte_nope")
	wantCode(t, err, CodeInvalidEmailToken)
	_, err = e.l.VerifyTenantEmail(e.ctx, e.acme, "wte_"+strings.Repeat("A", 43))
	wantCode(t, err, CodeInvalidEmailToken)

	got, err := e.l.VerifyTenantEmail(e.ctx, e.acme, code)
	e.must(t, err)
	if got.EmailVerifiedAt == nil {
		t.Fatal("not verified")
	}
	// Single use.
	_, err = e.l.VerifyTenantEmail(e.ctx, e.acme, code)
	wantCode(t, err, CodeInvalidEmailToken)
	// Grantee references by verified email resolve.
	for _, ref := range []string{"ops@acme.io", "OPS@acme.io", "acme", e.acme.ID} {
		r, err := ResolveTenantRef(e.ctx, e.store, ref)
		if err != nil || r.ID != e.acme.ID {
			t.Fatalf("ResolveTenantRef(%q) = %v, %v", ref, r, err)
		}
	}
	// Resending for a verified address is refused.
	_, err = e.l.ResendTenantVerification(e.ctx, "olga", e.acme)
	wantCode(t, err, CodeEmailVerified)

	// Changing the address drops the verification; old codes die.
	out, sent, err = e.l.SetTenantEmail(e.ctx, "olga", e.acme, "infra@acme.io")
	e.must(t, err)
	if out.EmailVerifiedAt != nil || sent == nil {
		t.Fatalf("after change: %+v %+v", out, sent)
	}
	first := codeRe.FindString(e.mailer.last(t).text)
	resent, err := e.l.ResendTenantVerification(e.ctx, "olga", e.acme)
	e.must(t, err)
	if resent.Email != "infra@acme.io" {
		t.Fatalf("resent = %+v", resent)
	}
	second := codeRe.FindString(e.mailer.last(t).text)
	_, err = e.l.VerifyTenantEmail(e.ctx, e.acme, first)
	wantCode(t, err, CodeInvalidEmailToken)

	// Codes expire after 24 hours.
	e.clock.Advance(EmailTokenTTL + time.Second)
	_, err = e.l.VerifyTenantEmail(e.ctx, e.acme, second)
	wantCode(t, err, CodeInvalidEmailToken)

	// Clearing the email.
	out, sent, err = e.l.SetTenantEmail(e.ctx, "olga", e.acme, "")
	e.must(t, err)
	if out.Email != "" || sent != nil {
		t.Fatalf("cleared = %+v %+v", out, sent)
	}
	_, err = e.l.ResendTenantVerification(e.ctx, "olga", e.acme)
	wantCode(t, err, CodeNoEmail)
	if _, _, err := e.l.SetTenantEmail(e.ctx, "olga", e.acme, "not an email"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("invalid email: %v", err)
	}
	// The pruner drops expired codes.
	if _, err := e.l.RunOnce(e.ctx); err != nil {
		t.Fatal(err)
	}
	if keys, _ := e.l.tokenKeys(e.ctx, func(EmailToken) bool { return true }); len(keys) != 0 {
		t.Fatalf("codes left after pruning: %d", len(keys))
	}
}

func TestVerifiedEmailIsUniqueAcrossTenants(t *testing.T) {
	e := newLCEnv(t, true, nil)
	other := &domain.Tenant{ID: "t-other", Slug: "other", Name: "Other", Kind: domain.TenantOrg, CreatedAt: e.clock.Now()}
	e.must(t, e.store.CreateTenant(e.ctx, other))
	_, _, err := e.l.SetTenantEmail(e.ctx, "olga", e.acme, "ops@acme.io")
	e.must(t, err)
	_, err = e.l.VerifyTenantEmail(e.ctx, e.acme, codeRe.FindString(e.mailer.last(t).text))
	e.must(t, err)
	_, _, err = e.l.SetTenantEmail(e.ctx, "x", other, "ops@acme.io")
	e.must(t, err)
	_, err = e.l.VerifyTenantEmail(e.ctx, other, codeRe.FindString(e.mailer.last(t).text))
	wantCode(t, err, CodeEmailTaken)
}

func TestVerificationIsLoggedWithoutSMTP(t *testing.T) {
	var logs bytes.Buffer
	e := newLCEnv(t, false, &logs)
	_, sent, err := e.l.SetTenantEmail(e.ctx, "olga", e.acme, "ops@acme.io")
	e.must(t, err)
	if sent.Delivery != DeliveryLog {
		t.Fatalf("delivery = %s", sent.Delivery)
	}
	code := codeRe.FindString(logs.String())
	if code == "" || !strings.Contains(logs.String(), "SMTP is not configured") {
		t.Fatalf("logs:\n%s", logs.String())
	}
	if _, err := e.l.VerifyTenantEmail(e.ctx, e.acme, code); err != nil {
		t.Fatal(err)
	}
}

func TestAccountEmailVerification(t *testing.T) {
	e := newLCEnv(t, true, nil)
	sent, err := e.l.ResendAccountVerification(e.ctx, e.owner.ID)
	e.must(t, err)
	mail := e.mailer.last(t)
	code := codeRe.FindString(mail.text)
	if sent.Email != "olga@example.com" || mail.to != "olga@example.com" || code == "" {
		t.Fatalf("sent %+v mail %+v", sent, mail)
	}
	// A tenant code does not verify an account and vice versa.
	_, err = e.l.VerifyTenantEmail(e.ctx, e.acme, code)
	wantCode(t, err, CodeInvalidEmailToken)
	acct, err := e.l.VerifyAccountEmail(e.ctx, e.owner.ID, code)
	e.must(t, err)
	if acct.EmailVerifiedAt == nil {
		t.Fatal("account not verified")
	}
	_, err = e.l.ResendAccountVerification(e.ctx, e.owner.ID)
	wantCode(t, err, CodeEmailVerified)
}

func TestTenantRename(t *testing.T) {
	e := newLCEnv(t, true, nil)
	partner := &domain.Tenant{ID: "t-partner", Slug: "partner", Name: "Partner", Kind: domain.TenantOrg, CreatedAt: e.clock.Now()}
	e.must(t, e.store.CreateTenant(e.ctx, partner))
	tree := &domain.Tree{ID: "tr-web", TenantID: partner.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	e.must(t, e.store.CreateTree(e.ctx, tree))
	own := &domain.Tree{ID: "tr-api", TenantID: e.acme.ID, Name: "api", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	e.must(t, e.store.CreateTree(e.ctx, own))
	e.must(t, e.store.PutGrant(e.ctx, &domain.Grant{TreeID: tree.ID, SubjectType: domain.SubjectTenant, SubjectID: e.acme.ID, Subject: "acme", Role: domain.RoleViewer}))
	e.must(t, e.store.PutLicenseRule(e.ctx, &domain.LicenseRule{TreeID: tree.ID, PathGlob: "lib/**", SPDX: "MIT", Grant: "read-only", Grantee: "acme"}))
	e.must(t, e.store.CreatePolicy(e.ctx, &domain.Policy{TenantID: e.acme.ID, Name: "deny-secrets", Enabled: true, Statements: []domain.PolicyStatement{{
		Effect: domain.EffectDeny, Actions: []string{"tree:read"}, Principals: []string{"*"},
		Resources: []string{"wt:path/acme/api/secrets/**", "wt:tenant/acme", "wt:tree/acmex/api"},
	}}}))
	e.must(t, e.store.CreatePolicy(e.ctx, &domain.Policy{TenantID: partner.ID, Name: "acme-read", Enabled: true, Statements: []domain.PolicyStatement{{
		Effect: domain.EffectAllow, Actions: []string{"tree:read"}, Principals: []string{"tenant:acme", "tenant:acmex"},
		Resources: []string{"wt:tree/partner/web"},
	}}}))

	out, res, err := e.l.Rename(e.ctx, "olga", e.acme, "acme-inc")
	e.must(t, err)
	if out.Slug != "acme-inc" || res.Slug != "acme" || res.RenamedTo != "acme-inc" ||
		!res.ExpiresAt.Equal(e.clock.Now().UTC().Add(SlugReservationPeriod)) {
		t.Fatalf("rename = %+v %+v", out, res)
	}
	if got, ok, err := movedSlugAt(e.ctx, e.store, "acme", e.clock.Now()); err != nil || !ok || got != "acme-inc" {
		t.Fatalf("MovedSlug = %q %v %v", got, ok, err)
	}
	if _, ok, _ := movedSlugAt(e.ctx, e.store, "acme-inc", e.clock.Now()); ok {
		t.Fatal("current slug reported as moved")
	}
	if r, _ := slugReservedAt(e.ctx, e.store, "acme", "", e.clock.Now()); !r {
		t.Fatal("old slug not reserved")
	}
	if r, _ := slugReservedAt(e.ctx, e.store, "acme", e.acme.ID, e.clock.Now()); r {
		t.Fatal("old slug reserved against its own tenant")
	}
	if types := e.bus.types(); len(types) != 1 || types[0] != domain.EventTenantRenamed {
		t.Fatalf("events = %v", types)
	}
	// References were rewritten.
	grants, _ := e.store.ListGrants(e.ctx, tree.ID)
	if len(grants) != 1 || grants[0].Subject != "acme-inc" {
		t.Fatalf("grants = %+v", grants)
	}
	rules, _ := e.store.ListLicenseRules(e.ctx, tree.ID)
	if len(rules) != 1 || rules[0].Grantee != "acme-inc" {
		t.Fatalf("license rules = %+v", rules)
	}
	pols, _ := e.store.ListPolicies(e.ctx, e.acme.ID)
	if got := strings.Join(pols[0].Statements[0].Resources, ","); got != "wt:path/acme-inc/api/secrets/**,wt:tenant/acme-inc,wt:tree/acmex/api" {
		t.Fatalf("own policy resources = %s", got)
	}
	pols, _ = e.store.ListPolicies(e.ctx, partner.ID)
	if got := strings.Join(pols[0].Statements[0].Principals, ","); got != "tenant:acme-inc,tenant:acmex" {
		t.Fatalf("partner policy principals = %s", got)
	}

	// Refusals.
	_, _, err = e.l.Rename(e.ctx, "olga", out, "partner")
	wantCode(t, err, CodeSlugTaken)
	_, _, err = e.l.Rename(e.ctx, "olga", out, "olga")
	wantCode(t, err, CodeSlugTaken)
	_, _, err = e.l.Rename(e.ctx, "x", partner, "acme")
	wantCode(t, err, CodeSlugReserved)
	if _, _, err := e.l.Rename(e.ctx, "olga", out, "Bad Slug"); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad slug: %v", err)
	}
	personal := e.tenant(t, "t-olga")
	_, _, err = e.l.Rename(e.ctx, "olga", personal, "olga2")
	wantCode(t, err, CodePersonalTenant)

	// Renaming back reclaims the old slug.
	back, _, err := e.l.Rename(e.ctx, "olga", out, "acme")
	e.must(t, err)
	if back.Slug != "acme" {
		t.Fatalf("back = %+v", back)
	}
	if _, ok, _ := movedSlugAt(e.ctx, e.store, "acme", e.clock.Now()); ok {
		t.Fatal("reclaimed slug still moved")
	}
	if got, ok, _ := movedSlugAt(e.ctx, e.store, "acme-inc", e.clock.Now()); !ok || got != "acme" {
		t.Fatalf("acme-inc -> %q %v", got, ok)
	}
	// Reservations expire after 90 days.
	e.clock.Advance(SlugReservationPeriod + time.Hour)
	if r, _ := slugReservedAt(e.ctx, e.store, "acme-inc", "", e.clock.Now()); r {
		t.Fatal("reservation did not expire")
	}
}

func TestRegistrationRefusesReservedSlugs(t *testing.T) {
	e := newLCEnv(t, true, nil)
	_, _, err := e.l.Rename(e.ctx, "olga", e.acme, "acme-inc")
	e.must(t, err)
	svc := New(e.store, Options{AllowRegistration: true, DisableRateLimit: true, Password: fastParams, Clock: e.clock.Now})
	if _, err := svc.Register(e.ctx, "acme", "acme@example.com", testPassword); !errors.Is(err, domain.ErrAlreadyExists) {
		t.Fatalf("Register(reserved slug) = %v", err)
	}
	if _, err := svc.Register(e.ctx, "fresh", "fresh@example.com", testPassword); err != nil {
		t.Fatal(err)
	}
}

func TestTenantDeleteRestoreAndPurge(t *testing.T) {
	e := newLCEnv(t, true, nil)
	sa := &domain.ServiceAccount{TenantID: e.acme.ID, Name: "ci", Role: domain.RoleDeveloper}
	e.must(t, e.store.CreateServiceAccount(e.ctx, sa))
	e.must(t, e.store.CreateToken(e.ctx, &domain.APIToken{ID: "k1", ServiceAccountID: sa.ID, Name: "key", Kind: domain.TokenService, TokenHash: "h1", CreatedAt: e.clock.Now()}))
	e.acme.Status, e.acme.StatusReason = "", ""

	d, err := e.l.Delete(e.ctx, "olga", e.acme)
	e.must(t, err)
	if d.RevokedKeys != 1 || !d.PurgeAt.Equal(e.clock.Now().UTC().Add(48*time.Hour)) || d.RequestedBy != "olga" {
		t.Fatalf("deletion = %+v", d)
	}
	cur := e.tenant(t, e.acme.ID)
	if cur.Status != domain.TenantSuspended || !strings.Contains(cur.StatusReason, "scheduled for deletion") {
		t.Fatalf("tenant after delete = %+v", cur)
	}
	if keys, _ := e.store.ListServiceAccountTokens(e.ctx, sa.ID); len(keys) != 0 {
		t.Fatalf("keys left: %+v", keys)
	}
	_, err = e.l.Delete(e.ctx, "olga", e.acme)
	wantCode(t, err, CodeDeletionScheduled)
	_, _, err = e.l.Rename(e.ctx, "olga", e.acme, "acme2")
	wantCode(t, err, CodeDeletionScheduled)
	_, err = e.l.Delete(e.ctx, "olga", e.tenant(t, "t-olga"))
	wantCode(t, err, CodePersonalTenant)

	// Restore returns to the previous status.
	restored, err := e.l.Restore(e.ctx, e.acme)
	e.must(t, err)
	if restored.Status != "" || restored.StatusReason != "" {
		t.Fatalf("restored = %+v", restored)
	}
	_, err = e.l.Restore(e.ctx, e.acme)
	wantCode(t, err, CodeNotScheduled)
	_, err = e.l.Deletion(e.ctx, e.acme.ID)
	wantCode(t, err, CodeNotScheduled)

	// A previously suspended tenant stays suspended after a restore.
	cur = e.tenant(t, e.acme.ID)
	cur.Status, cur.StatusReason = domain.TenantSuspended, "unpaid"
	e.must(t, e.store.UpdateTenant(e.ctx, cur))
	_, err = e.l.Delete(e.ctx, "olga", cur)
	e.must(t, err)
	restored, err = e.l.Restore(e.ctx, cur)
	e.must(t, err)
	if restored.Status != domain.TenantSuspended || restored.StatusReason != "unpaid" {
		t.Fatalf("restored suspended = %+v", restored)
	}
	restored.Status, restored.StatusReason = "", ""
	e.must(t, e.store.UpdateTenant(e.ctx, restored))

	// Purge after the window.
	tree := &domain.Tree{ID: "tr-api", TenantID: e.acme.ID, Name: "api", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	e.must(t, e.store.CreateTree(e.ctx, tree))
	other := &domain.Tenant{ID: "t-other", Slug: "other", Name: "Other", Kind: domain.TenantOrg, CreatedAt: e.clock.Now()}
	e.must(t, e.store.CreateTenant(e.ctx, other))
	otherTree := &domain.Tree{ID: "tr-other", TenantID: other.ID, Name: "web", DefaultBranch: "main", Visibility: domain.VisibilityPrivate}
	e.must(t, e.store.CreateTree(e.ctx, otherTree))
	e.must(t, e.store.PutGrant(e.ctx, &domain.Grant{TreeID: otherTree.ID, SubjectType: domain.SubjectTenant, SubjectID: e.acme.ID, Subject: "acme", Role: domain.RoleViewer}))
	team := &domain.Team{TenantID: e.acme.ID, Slug: "core", Name: "Core"}
	e.must(t, e.store.CreateTeam(e.ctx, team))
	e.must(t, e.store.PutDoc(e.ctx, "access_docs", domain.DocKey(e.acme.ID, ""), map[string]string{"x": "y"}))

	_, err = e.l.Delete(e.ctx, "olga", e.acme)
	e.must(t, err)
	if purged, err := e.l.RunOnce(e.ctx); err != nil || len(purged) != 0 {
		t.Fatalf("early purge = %v %v", purged, err)
	}
	e.clock.Advance(48*time.Hour + time.Minute)
	purged, err := e.l.RunOnce(e.ctx)
	e.must(t, err)
	if len(purged) != 1 || purged[0] != e.acme.ID {
		t.Fatalf("purged = %v", purged)
	}
	if _, err := e.store.GetTenant(e.ctx, e.acme.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("tenant still there: %v", err)
	}
	if _, err := e.store.GetTree(e.ctx, tree.ID); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("tree still there: %v", err)
	}
	if ms, _ := e.store.ListTenantMembers(e.ctx, e.acme.ID); len(ms) != 0 {
		t.Fatalf("members left: %+v", ms)
	}
	if ts, _ := e.store.ListTeams(e.ctx, e.acme.ID); len(ts) != 0 {
		t.Fatalf("teams left: %+v", ts)
	}
	if sas, _ := e.store.ListServiceAccounts(e.ctx, e.acme.ID); len(sas) != 0 {
		t.Fatalf("service accounts left: %+v", sas)
	}
	if gs, _ := e.store.ListGrants(e.ctx, otherTree.ID); len(gs) != 0 {
		t.Fatalf("tenant grants left: %+v", gs)
	}
	if docs, _ := e.store.ListDocs(e.ctx, "access_docs", ""); len(docs) != 0 {
		t.Fatalf("access docs left: %d", len(docs))
	}
	if r, _ := slugReservedAt(e.ctx, e.store, "acme", "", e.clock.Now()); !r {
		t.Fatal("purged slug is not reserved")
	}
	types := strings.Join(e.bus.types(), ",")
	if !strings.Contains(types, domain.EventTreeDeleted) || !strings.HasSuffix(types, domain.EventTenantDeleted) {
		t.Fatalf("events = %s", types)
	}
	found := false
	for _, a := range e.audit.events {
		found = found || a.Action == "tenant.purge"
	}
	if !found {
		t.Fatalf("no tenant.purge audit: %+v", e.audit.events)
	}
	// The other tenant is untouched.
	if _, err := e.store.GetTree(e.ctx, otherTree.ID); err != nil {
		t.Fatal(err)
	}
}

func TestRewriteResource(t *testing.T) {
	for in, want := range map[string]string{
		"wt:tenant/acme":           "wt:tenant/new",
		"wt:tree/acme/web":         "wt:tree/new/web",
		"wt:branch/acme/web/main":  "wt:branch/new/web/main",
		"wt:path/acme/web/src/**":  "wt:path/new/web/src/**",
		"wt:tree/acmecorp/web":     "wt:tree/acmecorp/web",
		"wt:tree/*/web":            "wt:tree/*/web",
		"*":                        "*",
		"wt:tenant/acme-inc":       "wt:tenant/acme-inc",
		"wt:tree/acme/web/../acme": "wt:tree/new/web/../acme",
	} {
		got, _ := rewriteResource(in, "acme", "new")
		if got != want {
			t.Errorf("rewriteResource(%q) = %q, want %q", in, got, want)
		}
	}
}
