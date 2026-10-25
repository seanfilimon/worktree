// Package webhooks delivers bus events to tenant and tree webhooks with
// HMAC signatures, retries and a delivery log (docs/ENGINEERING_PLAN.md
// §14.2).
//
// A webhook belongs to a tenant (TreeID "" = every tree of the tenant) or
// to one tree. The Service subscribes to the event bus; for every event of
// the hook's tenant/tree whose type matches one of the hook's patterns
// ("branch.pushed", "mr.*", "*") and that the hook's creator may still read
// (tree:read for tree events, tenant:read otherwise), it POSTs a JSON
// envelope
//
//	{"id","event","tenant","tree","actor","at","payload"}
//
// signed with X-WT-Signature: sha256=<hex HMAC-SHA256(secret, body)>.
// Failed attempts are retried after 1 m, 5 m, 30 m and 2 h; after 20
// consecutive failed attempts the hook is deactivated and webhook.failing
// is published. Deliveries are kept 30 days / the last 200 per hook.
//
// Targets resolving to loopback, link-local or private addresses are
// refused (at create/update time and again when dialing) unless
// Options.AllowPrivate is set.
//
// Storage (§14): collection "webhooks", key DocKey(tenantID, id), value
// hookDoc (the domain.Webhook plus its secret and the creator's account
// id); collection "webhook_deliveries", key DocKey(webhookID, created_at
// as fixed-width RFC 3339 with nanoseconds, id), value
// domain.WebhookDelivery whose Payload is the exact request body sent.
package webhooks

import (
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"net/netip"
	"net/url"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Document collections owned by this package.
const (
	CollHooks      = "webhooks"
	CollDeliveries = "webhook_deliveries"
)

// EventPing is the event type of test deliveries (POST .../test).
const EventPing = "ping"

// Defaults and limits.
const (
	DefaultTimeout      = 10 * time.Second
	DefaultFailureLimit = 20
	DefaultRetention    = 30 * 24 * time.Hour
	DefaultKeep         = 200
	DefaultPollInterval = 5 * time.Second
	DefaultPruneEvery   = time.Hour
	DefaultWorkers      = 4

	MaxResponse      = 2048 // bytes of the response body kept per delivery
	MaxHooksPerScope = 50
	MaxEvents        = 50
	MaxURL           = 2048
	MaxDescription   = 1000
	MaxSecret        = 256
)

// DefaultBackoff are the delays before the retries of a failed delivery.
var DefaultBackoff = []time.Duration{time.Minute, 5 * time.Minute, 30 * time.Minute, 2 * time.Hour}

// Deps are the collaborators of the webhooks package. Store is required;
// Authz is needed for the delivery-time permission re-check (without it
// every event of the hook's scope is delivered); Bus for event delivery
// (Start) and webhook.failing.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Options tune delivery. The zero value selects the defaults.
type Options struct {
	// AllowPrivate disables the SSRF guard (--webhook-allow-private).
	AllowPrivate bool
	// Version is reported in the User-Agent (W0rkTree-Hookshot/<version>).
	Version string
	// Client sends the deliveries. nil = a client that refuses private
	// addresses when dialing (unless AllowPrivate), ignores proxy
	// environment variables and does not follow redirects.
	Client *http.Client
	// Resolver resolves target hosts for the create/update-time SSRF
	// check. nil = net.DefaultResolver.
	Resolver func(ctx context.Context, host string) ([]netip.Addr, error)
	// Now is the clock (nil = time.Now().UTC()).
	Now func() time.Time
	// Timeout bounds one attempt (0 = DefaultTimeout).
	Timeout time.Duration
	// Backoff are the retry delays (nil = DefaultBackoff); a delivery gets
	// 1+len(Backoff) attempts.
	Backoff []time.Duration
	// FailureLimit is the number of consecutive failed attempts that
	// deactivates a hook (0 = DefaultFailureLimit).
	FailureLimit int
	// Keep and Retention bound the delivery log per hook (0 = defaults).
	Keep      int
	Retention time.Duration
	// Workers is the number of concurrent deliveries once started.
	Workers int
	// PollInterval is how often due retries are checked once started.
	PollInterval time.Duration
}

func (o Options) withDefaults() Options {
	if o.Version == "" {
		o.Version = "dev"
	}
	if o.Now == nil {
		o.Now = func() time.Time { return time.Now().UTC() }
	}
	if o.Timeout <= 0 {
		o.Timeout = DefaultTimeout
	}
	if o.Backoff == nil {
		o.Backoff = DefaultBackoff
	}
	if o.FailureLimit <= 0 {
		o.FailureLimit = DefaultFailureLimit
	}
	if o.Keep <= 0 {
		o.Keep = DefaultKeep
	}
	if o.Retention <= 0 {
		o.Retention = DefaultRetention
	}
	if o.Workers <= 0 {
		o.Workers = DefaultWorkers
	}
	if o.PollInterval <= 0 {
		o.PollInterval = DefaultPollInterval
	}
	if o.Resolver == nil {
		o.Resolver = defaultResolve
	}
	if o.Client == nil {
		o.Client = newGuardedClient(o.AllowPrivate, o.Timeout)
	}
	return o
}

// Service is the webhooks component.
type Service struct {
	d Deps
	o Options

	mu      sync.Mutex
	pending map[string]pendingRetry // delivery id -> scheduled retry
	jobs    chan job                // nil until Start
}

// New returns a Service with the default Options.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, o: Options{}.withDefaults(), pending: map[string]pendingRetry{}}
}

// Configure replaces the Options; call it before Start.
func (s *Service) Configure(o Options) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.o = o.withDefaults()
}

func (s *Service) now() time.Time { return s.o.Now().UTC() }

// Error is a domain error with a client-facing message. errors.Is matches
// its Kind (domain.ErrNotFound, ErrInvalid, ErrConflict, ...).
type Error struct {
	Kind error
	Msg  string
}

func (e *Error) Error() string { return e.Msg }
func (e *Error) Unwrap() error { return e.Kind }

func errorf(kind error, format string, args ...any) error {
	return &Error{Kind: kind, Msg: fmt.Sprintf(format, args...)}
}

// ---- storage -----------------------------------------------------------

// hookDoc is the stored form of a webhook (domain.Webhook never encodes
// its secret).
type hookDoc struct {
	domain.Webhook
	Secret    string `json:"secret"`
	CreatorID string `json:"creator_id,omitempty"` // account id ("" for service accounts)
}

// public is the API view of the hook (no secret).
func (h *hookDoc) public() domain.Webhook {
	w := h.Webhook
	w.Secret = ""
	w.HasSecret = h.Secret != ""
	w.Events = slices.Clone(w.Events)
	return w
}

// HookKey is the "webhooks" document key of hook id of tenantID.
func HookKey(tenantID, id string) string { return domain.DocKey(tenantID, id) }

// tsLayout is RFC 3339 with fixed-width nanoseconds, so keys sort by time.
const tsLayout = "2006-01-02T15:04:05.000000000Z07:00"

// DeliveryKey is the "webhook_deliveries" document key of d.
func DeliveryKey(d *domain.WebhookDelivery) string {
	return domain.DocKey(d.WebhookID, d.CreatedAt.UTC().Format(tsLayout), d.ID)
}

// Scope is the owner of a set of hooks: a tenant (Tree nil) or one of its
// trees.
type Scope struct {
	Tenant *domain.Tenant
	Tree   *domain.Tree
}

func (sc Scope) treeID() string {
	if sc.Tree == nil {
		return ""
	}
	return sc.Tree.ID
}

func (sc Scope) String() string {
	if sc.Tree != nil {
		return sc.Tenant.Slug + "/" + sc.Tree.Name
	}
	return sc.Tenant.Slug
}

func (s *Service) hooksOfTenant(ctx context.Context, tenantID string) ([]hookDoc, error) {
	return meta.ListDocsT[hookDoc](ctx, s.d.Store, CollHooks, meta.DocPrefix(tenantID))
}

// loadHook returns the stored hook id of sc, or a not-found error.
func (s *Service) loadHook(ctx context.Context, sc Scope, id string) (*hookDoc, error) {
	h, err := meta.GetDocT[hookDoc](ctx, s.d.Store, CollHooks, HookKey(sc.Tenant.ID, id))
	if errors.Is(err, domain.ErrNotFound) || (err == nil && h.TreeID != sc.treeID()) {
		return nil, errorf(domain.ErrNotFound, "webhook %q not found in %s", id, sc)
	}
	if err != nil {
		return nil, err
	}
	return h, nil
}

// ---- validation ----------------------------------------------------------

var patternRe = regexp.MustCompile(`^(\*|[a-z0-9_-]+(\.[a-z0-9_-]+)*(\.\*)?)$`)

// NormalizeEvents validates event-type patterns ("branch.pushed", "mr.*",
// "*"), dropping duplicates.
func NormalizeEvents(events []string) ([]string, error) {
	if len(events) == 0 {
		return nil, errorf(domain.ErrInvalid, "events must list at least one event type pattern (e.g. \"branch.pushed\", \"mr.*\", \"*\")")
	}
	if len(events) > MaxEvents {
		return nil, errorf(domain.ErrInvalid, "at most %d event patterns", MaxEvents)
	}
	out := make([]string, 0, len(events))
	for _, e := range events {
		e = strings.TrimSpace(e)
		if !patternRe.MatchString(e) {
			return nil, errorf(domain.ErrInvalid, "invalid event pattern %q (want a type like \"branch.pushed\", a prefix like \"mr.*\" or \"*\")", e)
		}
		if !slices.Contains(out, e) {
			out = append(out, e)
		}
	}
	return out, nil
}

// Matches reports whether event type typ matches one of patterns.
func Matches(patterns []string, typ string) bool {
	for _, p := range patterns {
		switch {
		case p == "*", p == typ:
			return true
		case strings.HasSuffix(p, ".*") && strings.HasPrefix(typ, p[:len(p)-1]):
			return true
		}
	}
	return false
}

// CheckTarget validates a webhook URL and, unless AllowPrivate, refuses
// hosts that are or resolve to loopback, link-local or private addresses.
func (s *Service) CheckTarget(ctx context.Context, raw string) (string, error) {
	raw = strings.TrimSpace(raw)
	if raw == "" {
		return "", errorf(domain.ErrInvalid, "url is required")
	}
	if len(raw) > MaxURL {
		return "", errorf(domain.ErrInvalid, "url must be at most %d bytes", MaxURL)
	}
	u, err := url.Parse(raw)
	if err != nil || (u.Scheme != "http" && u.Scheme != "https") || u.Hostname() == "" || u.Opaque != "" {
		return "", errorf(domain.ErrInvalid, "url must be an absolute http:// or https:// URL")
	}
	if s.o.AllowPrivate {
		return raw, nil
	}
	host := u.Hostname()
	var addrs []netip.Addr
	if ip, err := netip.ParseAddr(host); err == nil {
		addrs = []netip.Addr{ip}
	} else {
		rctx, cancel := context.WithTimeout(ctx, 5*time.Second)
		addrs, err = s.o.Resolver(rctx, host)
		cancel()
		if err != nil || len(addrs) == 0 {
			return "", errorf(domain.ErrInvalid, "cannot resolve webhook host %q", host)
		}
	}
	for _, ip := range addrs {
		if Blocked(ip) {
			return "", errorf(domain.ErrInvalid,
				"webhook target %q resolves to the non-public address %s; the server refuses loopback, link-local and private targets unless started with --webhook-allow-private",
				host, ip.Unmap())
		}
	}
	return raw, nil
}

func validDescription(d string) error {
	if len(d) > MaxDescription || !utf8.ValidString(d) {
		return errorf(domain.ErrInvalid, "description must be valid UTF-8 of at most %d bytes", MaxDescription)
	}
	return nil
}

func validSecret(sec string) error {
	if sec == "" || len(sec) > MaxSecret || !utf8.ValidString(sec) {
		return errorf(domain.ErrInvalid, "secret must be 1-%d bytes of UTF-8", MaxSecret)
	}
	return nil
}

// GenerateSecret returns a random signing secret.
func GenerateSecret() string {
	var b [32]byte
	_, _ = rand.Read(b[:])
	return "whsec_" + hex.EncodeToString(b[:])
}

// Sign returns the X-WT-Signature value of body: "sha256=<hex HMAC>".
func Sign(secret string, body []byte) string {
	m := hmac.New(sha256.New, []byte(secret))
	m.Write(body)
	return "sha256=" + hex.EncodeToString(m.Sum(nil))
}

// Verify reports whether signature (an X-WT-Signature value) signs body.
func Verify(secret string, body []byte, signature string) bool {
	return hmac.Equal([]byte(Sign(secret, body)), []byte(signature))
}

// ---- CRUD ------------------------------------------------------------------

// CreateInput is the body of POST .../webhooks.
type CreateInput struct {
	URL         string   `json:"url"`
	Events      []string `json:"events"`
	Secret      string   `json:"secret,omitempty"`
	Description string   `json:"description,omitempty"`
	Active      *bool    `json:"active,omitempty"`
}

// Patch is the body of PATCH .../webhooks/{id}; nil fields are unchanged.
type Patch struct {
	URL         *string  `json:"url,omitempty"`
	Events      []string `json:"events,omitempty"`
	Secret      *string  `json:"secret,omitempty"`
	Description *string  `json:"description,omitempty"`
	Active      *bool    `json:"active,omitempty"`
}

// List returns the hooks of sc (oldest first).
func (s *Service) List(ctx context.Context, sc Scope) ([]domain.Webhook, error) {
	docs, err := s.hooksOfTenant(ctx, sc.Tenant.ID)
	if err != nil {
		return nil, err
	}
	out := []domain.Webhook{}
	for i := range docs {
		if docs[i].TreeID == sc.treeID() {
			out = append(out, docs[i].public())
		}
	}
	slices.SortFunc(out, func(a, b domain.Webhook) int {
		if c := a.CreatedAt.Compare(b.CreatedAt); c != 0 {
			return c
		}
		return strings.Compare(a.ID, b.ID)
	})
	return out, nil
}

// Get returns hook id of sc.
func (s *Service) Get(ctx context.Context, sc Scope, id string) (*domain.Webhook, error) {
	h, err := s.loadHook(ctx, sc, id)
	if err != nil {
		return nil, err
	}
	w := h.public()
	return &w, nil
}

// Create adds a hook to sc on behalf of p. generated is the secret when
// the server generated it ("" when the caller supplied one).
func (s *Service) Create(ctx context.Context, p *domain.Principal, sc Scope, in CreateInput) (hook *domain.Webhook, generated string, err error) {
	if p == nil {
		return nil, "", errorf(domain.ErrUnauthenticated, "authentication required")
	}
	target, err := s.CheckTarget(ctx, in.URL)
	if err != nil {
		return nil, "", err
	}
	events, err := NormalizeEvents(in.Events)
	if err != nil {
		return nil, "", err
	}
	if err := validDescription(in.Description); err != nil {
		return nil, "", err
	}
	secret := in.Secret
	if secret == "" {
		secret = GenerateSecret()
		generated = secret
	} else if err := validSecret(secret); err != nil {
		return nil, "", err
	}
	existing, err := s.List(ctx, sc)
	if err != nil {
		return nil, "", err
	}
	if len(existing) >= MaxHooksPerScope {
		return nil, "", errorf(domain.ErrConflict, "%s already has %d webhooks (the maximum)", sc, MaxHooksPerScope)
	}
	now := s.now()
	h := hookDoc{
		Webhook: domain.Webhook{
			ID:          uuid.NewString(),
			TenantID:    sc.Tenant.ID,
			TreeID:      sc.treeID(),
			URL:         target,
			Description: in.Description,
			Events:      events,
			Active:      in.Active == nil || *in.Active,
			CreatedBy:   p.Username,
			CreatedAt:   now,
			UpdatedAt:   now,
		},
		Secret:    secret,
		CreatorID: p.AccountID,
	}
	if err := s.d.Store.CreateDoc(ctx, CollHooks, HookKey(h.TenantID, h.ID), h); err != nil {
		return nil, "", err
	}
	w := h.public()
	return &w, generated, nil
}

// Update applies patch to hook id of sc. changed lists the fields set
// (for the audit log). Reactivating a hook resets its failure count.
func (s *Service) Update(ctx context.Context, sc Scope, id string, patch Patch) (hook *domain.Webhook, changed []string, err error) {
	if _, err := s.loadHook(ctx, sc, id); err != nil {
		return nil, nil, err
	}
	var target string
	if patch.URL != nil {
		if target, err = s.CheckTarget(ctx, *patch.URL); err != nil {
			return nil, nil, err
		}
	}
	var events []string
	if patch.Events != nil {
		if events, err = NormalizeEvents(patch.Events); err != nil {
			return nil, nil, err
		}
	}
	if patch.Description != nil {
		if err := validDescription(*patch.Description); err != nil {
			return nil, nil, err
		}
	}
	if patch.Secret != nil {
		if err := validSecret(*patch.Secret); err != nil {
			return nil, nil, err
		}
	}
	var out domain.Webhook
	err = meta.UpdateDocT(ctx, s.d.Store, CollHooks, HookKey(sc.Tenant.ID, id), func(cur *hookDoc) (*hookDoc, error) {
		if cur == nil || cur.TreeID != sc.treeID() {
			return nil, errorf(domain.ErrNotFound, "webhook %q not found in %s", id, sc)
		}
		changed = changed[:0]
		if patch.URL != nil {
			cur.URL = target
			changed = append(changed, "url")
		}
		if patch.Events != nil {
			cur.Events = events
			changed = append(changed, "events")
		}
		if patch.Description != nil {
			cur.Description = *patch.Description
			changed = append(changed, "description")
		}
		if patch.Secret != nil {
			cur.Secret = *patch.Secret
			changed = append(changed, "secret")
		}
		if patch.Active != nil {
			if *patch.Active && !cur.Active {
				cur.FailureCount = 0
			}
			cur.Active = *patch.Active
			changed = append(changed, "active")
		}
		cur.UpdatedAt = s.now()
		out = cur.public()
		return cur, nil
	})
	if err != nil {
		return nil, nil, err
	}
	if !out.Active {
		s.dropPending(out.ID)
	}
	return &out, changed, nil
}

// Delete removes hook id of sc and its delivery log.
func (s *Service) Delete(ctx context.Context, sc Scope, id string) error {
	if _, err := s.loadHook(ctx, sc, id); err != nil {
		return err
	}
	if err := s.d.Store.DeleteDoc(ctx, CollHooks, HookKey(sc.Tenant.ID, id)); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return errorf(domain.ErrNotFound, "webhook %q not found in %s", id, sc)
		}
		return err
	}
	s.dropPending(id)
	_, err := s.d.Store.DeleteDocs(ctx, CollDeliveries, meta.DocPrefix(id))
	return err
}

// deleteTreeHooks removes the hooks (and logs) of a deleted tree.
func (s *Service) deleteTreeHooks(ctx context.Context, tenantID, treeID string) error {
	docs, err := s.hooksOfTenant(ctx, tenantID)
	if err != nil {
		return err
	}
	var errs []error
	for i := range docs {
		if docs[i].TreeID != treeID {
			continue
		}
		if err := s.d.Store.DeleteDoc(ctx, CollHooks, HookKey(tenantID, docs[i].ID)); err != nil && !errors.Is(err, domain.ErrNotFound) {
			errs = append(errs, err)
		}
		s.dropPending(docs[i].ID)
		if _, err := s.d.Store.DeleteDocs(ctx, CollDeliveries, meta.DocPrefix(docs[i].ID)); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

// ---- delivery log ----------------------------------------------------------

func (s *Service) deliveriesOf(ctx context.Context, hookID string) ([]domain.WebhookDelivery, error) {
	return meta.ListDocsT[domain.WebhookDelivery](ctx, s.d.Store, CollDeliveries, meta.DocPrefix(hookID))
}

// Deliveries returns up to limit deliveries of hook id of sc, newest
// first (limit <= 0 = all kept).
func (s *Service) Deliveries(ctx context.Context, sc Scope, id string, limit int) ([]domain.WebhookDelivery, error) {
	if _, err := s.loadHook(ctx, sc, id); err != nil {
		return nil, err
	}
	list, err := s.deliveriesOf(ctx, id)
	if err != nil {
		return nil, err
	}
	slices.Reverse(list)
	if limit > 0 && len(list) > limit {
		list = list[:limit]
	}
	return list, nil
}

// Delivery returns delivery did of hook id of sc.
func (s *Service) Delivery(ctx context.Context, sc Scope, id, did string) (*domain.WebhookDelivery, error) {
	if _, err := s.loadHook(ctx, sc, id); err != nil {
		return nil, err
	}
	return s.findDelivery(ctx, id, did)
}

func (s *Service) findDelivery(ctx context.Context, hookID, did string) (*domain.WebhookDelivery, error) {
	list, err := s.deliveriesOf(ctx, hookID)
	if err != nil {
		return nil, err
	}
	for i := range list {
		if list[i].ID == did {
			return &list[i], nil
		}
	}
	return nil, errorf(domain.ErrNotFound, "delivery %q of webhook %q not found", did, hookID)
}

// prune drops the deliveries of hookID beyond Keep or older than
// Retention.
func (s *Service) prune(ctx context.Context, hookID string) error {
	list, err := s.deliveriesOf(ctx, hookID)
	if err != nil {
		return err
	}
	cutoff := s.now().Add(-s.o.Retention)
	excess := len(list) - s.o.Keep
	var errs []error
	for i := range list {
		if i >= excess && !list[i].CreatedAt.Before(cutoff) {
			continue
		}
		if err := s.d.Store.DeleteDoc(ctx, CollDeliveries, DeliveryKey(&list[i])); err != nil && !errors.Is(err, domain.ErrNotFound) {
			errs = append(errs, err)
		}
		s.mu.Lock()
		delete(s.pending, list[i].ID)
		s.mu.Unlock()
	}
	return errors.Join(errs...)
}

// Prune applies the delivery-log retention to every hook (the background
// loop runs it hourly; new deliveries prune their own hook).
func (s *Service) Prune(ctx context.Context) error {
	docs, err := meta.ListDocsT[hookDoc](ctx, s.d.Store, CollHooks, "")
	if err != nil {
		return err
	}
	var errs []error
	for i := range docs {
		if err := s.prune(ctx, docs[i].ID); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}
