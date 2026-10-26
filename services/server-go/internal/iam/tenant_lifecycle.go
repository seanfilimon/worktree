package iam

// Tenant lifecycle (docs/ENGINEERING_PLAN.md §16.4): tenant and account
// email verification, slug renames with reservation of the old slug, and
// deletion with a recovery window followed by a purge.
//
// Documents (DocStore collections):
//
//	email_tokens      key: hex sha256 of the one-time code   -> EmailToken
//	tenant_slugs      key: reserved (old) slug               -> SlugReservation
//	tenant_deletions  key: tenant id                         -> TenantDeletion

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	htmltemplate "html/template"
	"log/slog"
	"net/url"
	"regexp"
	"strings"
	"sync"
	texttemplate "text/template"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Collections of the tenant lifecycle.
const (
	CollEmailTokens     = "email_tokens"
	CollTenantSlugs     = "tenant_slugs"
	CollTenantDeletions = "tenant_deletions"
)

// Lifecycle parameters.
const (
	// EmailTokenPrefix starts email verification codes ("wte_" + 32 random
	// bytes, base64url).
	EmailTokenPrefix = "wte_"
	// EmailTokenTTL is how long a verification code stays valid.
	EmailTokenTTL = 24 * time.Hour
	// SlugReservationPeriod is how long a renamed (or purged) tenant's old
	// slug stays reserved.
	SlugReservationPeriod = 90 * 24 * time.Hour
	// DefaultDeletionWindow is the recovery window of a deleted tenant.
	DefaultDeletionWindow = 30 * 24 * time.Hour
	// DefaultPurgeInterval is how often the purge job runs.
	DefaultPurgeInterval = time.Hour
)

// Email verification subjects (EmailToken.Subject).
const (
	EmailSubjectTenant  = "tenant"
	EmailSubjectAccount = "account"
)

// Verification delivery (VerificationSent.Delivery).
const (
	DeliveryEmail = "email" // queued on the SMTP mailer
	DeliveryLog   = "log"   // SMTP is not configured: the code was logged
)

// Error codes of LifecycleError (the REST envelope's "code").
const (
	CodePersonalTenant    = "personal_tenant"
	CodeSlugTaken         = "slug_taken"
	CodeSlugReserved      = "slug_reserved"
	CodeDeletionScheduled = "deletion_scheduled"
	CodeNotScheduled      = "deletion_not_scheduled"
	CodeNoEmail           = "no_email"
	CodeEmailVerified     = "email_already_verified"
	CodeEmailTaken        = "email_taken"
	CodeInvalidEmailToken = "invalid_email_token"
)

// LifecycleError is a tenant-lifecycle refusal with a stable code.
// errors.Is matches Kind (a domain sentinel).
type LifecycleError struct {
	Code string
	Msg  string
	Kind error
}

func (e *LifecycleError) Error() string { return e.Msg }
func (e *LifecycleError) Unwrap() error { return e.Kind }

func lifecycleErr(kind error, code, format string, args ...any) error {
	return &LifecycleError{Code: code, Msg: fmt.Sprintf(format, args...), Kind: kind}
}

// EmailToken is a stored one-time email verification code.
type EmailToken struct {
	Subject   string     `json:"subject"`    // EmailSubjectTenant | EmailSubjectAccount
	SubjectID string     `json:"subject_id"` // tenant or account id
	Email     string     `json:"email"`      // the address the code was sent to
	CreatedBy string     `json:"created_by,omitempty"`
	CreatedAt time.Time  `json:"created_at"`
	ExpiresAt time.Time  `json:"expires_at"`
	UsedAt    *time.Time `json:"used_at,omitempty"`
}

// VerificationSent reports where a verification code went.
type VerificationSent struct {
	Email     string    `json:"email"`
	Delivery  string    `json:"delivery"` // DeliveryEmail | DeliveryLog
	ExpiresAt time.Time `json:"expires_at"`
}

// SlugReservation keeps an old slug from being claimed by anyone else.
// TenantID is the tenant that used it (renamed, or purged).
type SlugReservation struct {
	Slug       string    `json:"slug"`
	TenantID   string    `json:"tenant_id"`
	RenamedTo  string    `json:"renamed_to,omitempty"` // "" when the tenant was purged
	ReservedAt time.Time `json:"reserved_at"`
	ExpiresAt  time.Time `json:"expires_at"`
}

// TenantDeletion is a scheduled deletion (the recovery window).
type TenantDeletion struct {
	TenantID       string    `json:"tenant_id"`
	Slug           string    `json:"slug"`
	RequestedBy    string    `json:"requested_by"`
	RequestedAt    time.Time `json:"requested_at"`
	PurgeAt        time.Time `json:"purge_at"`
	PreviousStatus string    `json:"previous_status,omitempty"`
	PreviousReason string    `json:"previous_status_reason,omitempty"`
	// RevokedKeys is the number of service-account keys revoked when the
	// tenant was suspended for deletion (they are not restored).
	RevokedKeys int `json:"revoked_keys"`
}

// Mailer delivers transactional email; it reports false when email is
// not configured. Implemented by notify.Service.SendMail.
type Mailer interface {
	SendMail(ctx context.Context, to, toName, subject, text, html string) bool
}

// TenantDeleter is implemented by stores that can remove a tenant record
// (internal/meta: Memory and Bolt).
type TenantDeleter interface {
	DeleteTenant(ctx context.Context, id string) error
}

// LifecycleDeps are the collaborators of TenantLifecycle. Store is
// required; Bus, Audit and Mailer may be nil.
type LifecycleDeps struct {
	Store  domain.MetaStore
	Bus    domain.EventBus
	Audit  domain.AuditLog
	Mailer Mailer
	Logger *slog.Logger
}

// LifecycleOptions tune TenantLifecycle (zero values = defaults).
type LifecycleOptions struct {
	DeletionWindow time.Duration // recovery window (default 30 days)
	PurgeInterval  time.Duration // purge job period (default 1 hour)
	PublicURL      string        // external base URL for verification links
	Clock          func() time.Time
}

// TenantLifecycle implements the tenant lifecycle. Authorization is the
// caller's job (internal/httpapi); the service validates, stores,
// publishes events and runs the purge job.
type TenantLifecycle struct {
	d LifecycleDeps
	o LifecycleOptions

	mu sync.Mutex // serializes renames, deletions and verifications
}

// NewTenantLifecycle returns the lifecycle service.
func NewTenantLifecycle(d LifecycleDeps, o LifecycleOptions) *TenantLifecycle {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	if o.DeletionWindow <= 0 {
		o.DeletionWindow = DefaultDeletionWindow
	}
	if o.PurgeInterval <= 0 {
		o.PurgeInterval = DefaultPurgeInterval
	}
	if o.Clock == nil {
		o.Clock = time.Now
	}
	return &TenantLifecycle{d: d, o: o}
}

// DeletionWindow is the configured recovery window.
func (l *TenantLifecycle) DeletionWindow() time.Duration { return l.o.DeletionWindow }

func (l *TenantLifecycle) now() time.Time { return l.o.Clock().UTC() }

func (l *TenantLifecycle) publish(e domain.Event) {
	if l.d.Bus == nil {
		return
	}
	if e.At.IsZero() {
		e.At = l.now()
	}
	l.d.Bus.Publish(e)
}

func (l *TenantLifecycle) audit(ctx context.Context, e domain.AuditEvent) {
	if l.d.Audit == nil {
		return
	}
	if e.At.IsZero() {
		e.At = l.now()
	}
	if e.Actor == "" {
		e.Actor = "system"
	}
	l.d.Audit.Record(context.WithoutCancel(ctx), e)
}

// ---- slugs --------------------------------------------------------------

var tenantSlugRe = regexp.MustCompile(`^[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?$`)

// ValidateTenantSlug applies the organization slug rules (ErrInvalid).
func ValidateTenantSlug(slug string) error {
	if !tenantSlugRe.MatchString(slug) {
		return fmt.Errorf("slug must be 1-63 characters of lowercase letters, digits and '-', not starting or ending with '-': %w", domain.ErrInvalid)
	}
	return nil
}

// reservation returns the live (unexpired) reservation of slug, or nil.
func reservation(ctx context.Context, store domain.DocStore, slug string, now time.Time) (*SlugReservation, error) {
	if slug == "" {
		return nil, nil
	}
	var r SlugReservation
	if err := store.GetDoc(ctx, CollTenantSlugs, slug, &r); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, nil
		}
		return nil, fmt.Errorf("iam: reading slug reservation: %w", err)
	}
	if !now.Before(r.ExpiresAt) {
		return nil, nil
	}
	return &r, nil
}

// MovedSlug reports the current slug of the tenant that was renamed away
// from slug, while the old slug is reserved (90 days). ok is false when
// slug is not a reserved old slug, its tenant no longer exists or still
// uses it.
func MovedSlug(ctx context.Context, store domain.MetaStore, slug string) (newSlug string, ok bool, err error) {
	return movedSlugAt(ctx, store, slug, time.Now().UTC())
}

func movedSlugAt(ctx context.Context, store domain.MetaStore, slug string, now time.Time) (string, bool, error) {
	r, err := reservation(ctx, store, slug, now)
	if err != nil || r == nil {
		return "", false, err
	}
	t, err := store.GetTenant(ctx, r.TenantID)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return "", false, nil
		}
		return "", false, err
	}
	if t.Slug == slug {
		return "", false, nil
	}
	return t.Slug, true, nil
}

// SlugReserved reports whether slug is reserved for a tenant other than
// exceptTenantID ("" = any tenant): new tenants, registrations and renames
// cannot claim it.
func SlugReserved(ctx context.Context, store domain.DocStore, slug, exceptTenantID string) (bool, error) {
	return slugReservedAt(ctx, store, slug, exceptTenantID, time.Now().UTC())
}

func slugReservedAt(ctx context.Context, store domain.DocStore, slug, exceptTenantID string, now time.Time) (bool, error) {
	r, err := reservation(ctx, store, slug, now)
	if err != nil || r == nil {
		return false, err
	}
	return r.TenantID != exceptTenantID || exceptTenantID == "", nil
}

// TenantByVerifiedEmail returns the tenant whose verified contact email is
// email (case-insensitive), or ErrNotFound. Unverified addresses never
// resolve.
func TenantByVerifiedEmail(ctx context.Context, store domain.MetaStore, email string) (*domain.Tenant, error) {
	email = strings.TrimSpace(email)
	if email == "" || !strings.Contains(email, "@") {
		return nil, fmt.Errorf("iam: tenant email %q: %w", email, domain.ErrNotFound)
	}
	ts, err := store.ListTenants(ctx)
	if err != nil {
		return nil, err
	}
	for i := range ts {
		if ts[i].EmailVerifiedAt != nil && strings.EqualFold(ts[i].Email, email) {
			t := ts[i]
			return &t, nil
		}
	}
	return nil, fmt.Errorf("iam: no tenant has the verified email %q: %w", email, domain.ErrNotFound)
}

// ResolveTenantRef resolves a tenant reference: a slug, a tenant id or a
// verified tenant email (grantee references such as tenant =
// "ops@acme.io"). Renamed slugs are not followed (see MovedSlug).
func ResolveTenantRef(ctx context.Context, store domain.MetaStore, ref string) (*domain.Tenant, error) {
	ref = strings.TrimSpace(ref)
	if strings.Contains(ref, "@") {
		return TenantByVerifiedEmail(ctx, store, ref)
	}
	t, err := store.GetTenantBySlug(ctx, ref)
	if err == nil || !errors.Is(err, domain.ErrNotFound) {
		return t, err
	}
	return store.GetTenant(ctx, ref)
}

// ---- email verification ------------------------------------------------

// SetTenantEmail sets (or with "" clears) the tenant's contact email. A
// new address is unverified and receives a verification code; setting the
// current address again is a no-op (nil VerificationSent when nothing was
// sent).
func (l *TenantLifecycle) SetTenantEmail(ctx context.Context, actor string, t *domain.Tenant, email string) (*domain.Tenant, *VerificationSent, error) {
	email = strings.TrimSpace(email)
	if email != "" {
		if err := ValidateEmail(email); err != nil {
			return nil, nil, fmt.Errorf("iam: %w", err)
		}
	}
	l.mu.Lock()
	cur, err := l.d.Store.GetTenant(ctx, t.ID)
	if err != nil {
		l.mu.Unlock()
		return nil, nil, err
	}
	if cur.Email == email {
		l.mu.Unlock()
		return cur, nil, nil
	}
	cur.Email = email
	cur.EmailVerifiedAt = nil
	if err := l.d.Store.UpdateTenant(ctx, cur); err != nil {
		l.mu.Unlock()
		return nil, nil, err
	}
	l.mu.Unlock()
	if err := l.dropTokens(ctx, EmailSubjectTenant, cur.ID); err != nil {
		return nil, nil, err
	}
	if email == "" {
		return cur, nil, nil
	}
	sent, err := l.sendVerification(ctx, EmailSubjectTenant, cur.ID, cur.Slug, email, "", actor)
	if err != nil {
		return nil, nil, err
	}
	return cur, sent, nil
}

// ResendTenantVerification sends a new verification code for the tenant's
// unverified email (earlier codes stop working).
func (l *TenantLifecycle) ResendTenantVerification(ctx context.Context, actor string, t *domain.Tenant) (*VerificationSent, error) {
	cur, err := l.d.Store.GetTenant(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	switch {
	case cur.Email == "":
		return nil, lifecycleErr(domain.ErrConflict, CodeNoEmail, "tenant %s has no email address; set one first", cur.Slug)
	case cur.EmailVerifiedAt != nil:
		return nil, lifecycleErr(domain.ErrConflict, CodeEmailVerified, "the email of tenant %s is already verified", cur.Slug)
	}
	if err := l.dropTokens(ctx, EmailSubjectTenant, cur.ID); err != nil {
		return nil, err
	}
	return l.sendVerification(ctx, EmailSubjectTenant, cur.ID, cur.Slug, cur.Email, "", actor)
}

// VerifyTenantEmail redeems a tenant verification code. The code must have
// been sent to the tenant's current address, be unused and unexpired;
// every failure yields the same CodeInvalidEmailToken error.
func (l *TenantLifecycle) VerifyTenantEmail(ctx context.Context, t *domain.Tenant, code string) (*domain.Tenant, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	cur, err := l.d.Store.GetTenant(ctx, t.ID)
	if err != nil {
		return nil, err
	}
	if cur.Email == "" {
		return nil, errInvalidEmailToken()
	}
	if other, err := TenantByVerifiedEmail(ctx, l.d.Store, cur.Email); err == nil && other.ID != cur.ID {
		return nil, lifecycleErr(domain.ErrConflict, CodeEmailTaken, "another tenant already verified %s", cur.Email)
	}
	at, err := l.redeem(ctx, code, EmailSubjectTenant, cur.ID, cur.Email)
	if err != nil {
		return nil, err
	}
	cur.EmailVerifiedAt = &at
	if err := l.d.Store.UpdateTenant(ctx, cur); err != nil {
		return nil, err
	}
	return cur, nil
}

// ResendAccountVerification sends a verification code to the account's
// email (earlier codes stop working).
func (l *TenantLifecycle) ResendAccountVerification(ctx context.Context, accountID string) (*VerificationSent, error) {
	acct, err := l.d.Store.GetAccount(ctx, accountID)
	if err != nil {
		return nil, err
	}
	switch {
	case strings.TrimSpace(acct.Email) == "":
		return nil, lifecycleErr(domain.ErrConflict, CodeNoEmail, "account %s has no email address", acct.Username)
	case acct.EmailVerifiedAt != nil:
		return nil, lifecycleErr(domain.ErrConflict, CodeEmailVerified, "the email of account %s is already verified", acct.Username)
	}
	if err := l.dropTokens(ctx, EmailSubjectAccount, acct.ID); err != nil {
		return nil, err
	}
	name := acct.DisplayName
	if name == "" {
		name = acct.Username
	}
	return l.sendVerification(ctx, EmailSubjectAccount, acct.ID, acct.Username, acct.Email, name, acct.Username)
}

// VerifyAccountEmail redeems an account verification code for accountID.
func (l *TenantLifecycle) VerifyAccountEmail(ctx context.Context, accountID, code string) (*domain.Account, error) {
	l.mu.Lock()
	defer l.mu.Unlock()
	acct, err := l.d.Store.GetAccount(ctx, accountID)
	if err != nil {
		return nil, err
	}
	if acct.Email == "" {
		return nil, errInvalidEmailToken()
	}
	at, err := l.redeem(ctx, code, EmailSubjectAccount, acct.ID, acct.Email)
	if err != nil {
		return nil, err
	}
	acct.EmailVerifiedAt = &at
	if err := l.d.Store.UpdateAccount(ctx, acct); err != nil {
		return nil, err
	}
	return acct, nil
}

func errInvalidEmailToken() error {
	return lifecycleErr(domain.ErrInvalid, CodeInvalidEmailToken, "invalid, expired or already used verification code")
}

// redeem atomically marks the code used when it matches (subject,
// subjectID, email) and is unused and unexpired; it returns the time.
func (l *TenantLifecycle) redeem(ctx context.Context, code, subject, subjectID, email string) (time.Time, error) {
	code = strings.TrimSpace(code)
	if !wellFormedSecret(EmailTokenPrefix, code) {
		return time.Time{}, errInvalidEmailToken()
	}
	now := l.now()
	matched := false
	err := l.d.Store.UpdateDoc(ctx, CollEmailTokens, HashToken(code), func(raw []byte) ([]byte, error) {
		if raw == nil {
			return nil, errInvalidEmailToken()
		}
		var tok storedToken
		if err := json.Unmarshal(raw, &tok); err != nil {
			return nil, err
		}
		if tok.Subject != subject || tok.SubjectID != subjectID || !strings.EqualFold(tok.Email, email) ||
			tok.UsedAt != nil || !now.Before(tok.ExpiresAt) {
			return nil, errInvalidEmailToken()
		}
		tok.UsedAt = &now
		matched = true
		return json.Marshal(tok)
	})
	if err != nil {
		return time.Time{}, err
	}
	if !matched {
		return time.Time{}, errInvalidEmailToken()
	}
	return now, nil
}

// dropTokens deletes every stored code of (subject, subjectID).
func (l *TenantLifecycle) dropTokens(ctx context.Context, subject, subjectID string) error {
	keys, err := l.tokenKeys(ctx, func(t EmailToken) bool { return t.Subject == subject && t.SubjectID == subjectID })
	if err != nil {
		return err
	}
	for _, k := range keys {
		if err := l.d.Store.DeleteDoc(ctx, CollEmailTokens, k); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	return nil
}

// tokenKeys returns the keys of the stored codes matching keep (ListDocs
// returns no keys, so every document carries its own key in "_key").
func (l *TenantLifecycle) tokenKeys(ctx context.Context, keep func(EmailToken) bool) ([]string, error) {
	raws, err := l.d.Store.ListDocs(ctx, CollEmailTokens, "")
	if err != nil {
		return nil, err
	}
	var out []string
	for _, raw := range raws {
		var doc storedToken
		if json.Unmarshal(raw, &doc) != nil || doc.Key == "" {
			continue
		}
		if keep(doc.EmailToken) {
			out = append(out, doc.Key)
		}
	}
	return out, nil
}

// storedToken is the stored form of EmailToken (with its own key).
type storedToken struct {
	EmailToken
	Key string `json:"_key"`
}

// sendVerification issues a code and mails it (or logs it without SMTP).
func (l *TenantLifecycle) sendVerification(ctx context.Context, subject, subjectID, label, email, toName, actor string) (*VerificationSent, error) {
	raw, h, err := newSecret(EmailTokenPrefix)
	if err != nil {
		return nil, err
	}
	now := l.now()
	tok := storedToken{
		EmailToken: EmailToken{Subject: subject, SubjectID: subjectID, Email: email, CreatedBy: actor, CreatedAt: now, ExpiresAt: now.Add(EmailTokenTTL)},
		Key:        h,
	}
	if err := l.d.Store.PutDoc(ctx, CollEmailTokens, h, tok); err != nil {
		return nil, fmt.Errorf("iam: storing verification code: %w", err)
	}
	m := l.verificationMail(subject, label, raw, tok.ExpiresAt)
	out := &VerificationSent{Email: email, Delivery: DeliveryLog, ExpiresAt: tok.ExpiresAt}
	if l.d.Mailer != nil && l.d.Mailer.SendMail(ctx, email, toName, m.subject, m.text, m.html) {
		out.Delivery = DeliveryEmail
		l.d.Logger.InfoContext(ctx, "email verification sent", "subject", subject, "name", label, "email", email)
		return out, nil
	}
	l.d.Logger.InfoContext(ctx, "email verification (SMTP is not configured; the code is logged instead)",
		"subject", subject, "name", label, "email", email, "token", raw, "link", m.link, "expires_at", tok.ExpiresAt.Format(time.RFC3339))
	return out, nil
}

type verificationMessage struct {
	subject, text, html, link string
}

type verificationData struct {
	Kind, Name, Code, Link, Command string
	Expires                         string
}

var verifyText = texttemplate.Must(texttemplate.New("verify").Parse(`Hi,

{{if eq .Kind "tenant"}}Someone with admin access to the W0rkTree tenant "{{.Name}}" set this address as the tenant's contact email.{{else}}The W0rkTree account "{{.Name}}" uses this email address.{{end}}
To confirm it{{if .Link}}, open:

  {{.Link}}

or{{end}} run:

  {{.Command}}

The code expires at {{.Expires}}. If you did not expect this email, ignore it.
`))

var verifyHTML = htmltemplate.Must(htmltemplate.New("verify").Parse(`<!doctype html>
<html><body style="font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;color:#111;background:#fff">
<p>Hi,</p>
<p>{{if eq .Kind "tenant"}}Someone with admin access to the W0rkTree tenant <strong>{{.Name}}</strong> set this address as the tenant's contact email.{{else}}The W0rkTree account <strong>{{.Name}}</strong> uses this email address.{{end}}</p>
{{if .Link}}<p><a href="{{.Link}}" style="display:inline-block;padding:6px 12px;border:1px solid #111;border-radius:6px;color:#111;text-decoration:none">Verify email</a></p>{{end}}
<p>Or run:</p>
<pre style="padding:8px 12px;border:1px solid #ddd;border-radius:6px">{{.Command}}</pre>
<p style="font-size:12px;color:#666">The code expires at {{.Expires}}. If you did not expect this email, ignore it.</p>
</body></html>
`))

func (l *TenantLifecycle) verificationMail(subject, name, code string, expires time.Time) verificationMessage {
	d := verificationData{Kind: subject, Name: name, Code: code, Expires: expires.Format(time.RFC1123)}
	base := strings.TrimRight(l.o.PublicURL, "/")
	if subject == EmailSubjectTenant {
		d.Command = "wt tenant email verify " + name + " " + code
		if base != "" {
			d.Link = base + "/admin/tenants/" + url.PathEscape(name) + "?tab=settings&email_token=" + url.QueryEscape(code)
		}
	} else {
		d.Command = "wt api POST /api/v1/me/email/verify -d '{\"token\":\"" + code + "\"}'"
		if base != "" {
			d.Link = base + "/admin/settings?email_token=" + url.QueryEscape(code)
		}
	}
	var text, html bytes.Buffer
	_ = verifyText.Execute(&text, d)
	_ = verifyHTML.Execute(&html, d)
	title := "Verify your W0rkTree email address"
	if subject == EmailSubjectTenant {
		title = "Verify the contact email of " + name + " on W0rkTree"
	}
	return verificationMessage{subject: title, text: text.String(), html: html.String(), link: d.Link}
}
