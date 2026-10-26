// Package notify turns bus events into per-account notifications, keeps
// watch settings and sends optional emails (docs/ENGINEERING_PLAN.md
// §15.3).
//
// The Service subscribes to the event bus (Start) and, for every event it
// understands, works out who should hear about it:
//
//   - participating reasons, which involve you directly: a review request
//     (mr.review_requested, mr.ready with reviewers; teams are expanded to
//     their members), an @mention of your username (or of a team you are in)
//     in a merge request description, comment or review, a failed CI status
//     on the source tip of an open merge request that you authored or pushed,
//     a TODO newly assigned to you, and your own merge request being merged,
//     approved, reviewed or commented on;
//   - watching: every merge request event, failed CI status and published
//     release of a tree you watch (directly, or through its tenant).
//
// The actor of an event is never notified; a recipient must hold tree:read
// on the tree when the notification is created; the effective watch level
// "ignoring" silences a tree entirely. Identical notifications (same
// account, reason, target and actor) within DedupWindow are dropped. Each
// account keeps at most Keep notifications for at most Retention.
//
// Every created notification is published as notification.created with an
// empty TenantID (so tenant-scoped subscribers such as webhooks never see
// it); the SSE endpoint delivers it only to its account. When SMTP is
// configured an email is queued and sent asynchronously with retries.
//
// Storage (§15): collection "notifications", key DocKey(accountID,
// created_at as fixed-width RFC 3339 with nanoseconds, id), value
// domain.Notification; collection "watch_settings", key DocKey(accountID,
// tenantID, treeID) (treeID "" for a tenant), value domain.WatchSetting;
// collection "notification_settings", key accountID, value Settings;
// collection "notify_state", key DocKey(kind, treeID, item) — the last
// notified TODO assignee (kind "todo", item %010d number) and announced
// releases (kind "release", item tag) — so updates do not notify twice.
package notify

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/google/uuid"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Document collections owned by this package.
const (
	CollNotifications = "notifications"
	CollWatch         = "watch_settings"
	CollSettings      = "notification_settings"
	CollState         = "notify_state"
)

// Notification reasons (domain.Notification.Reason).
const (
	ReasonReviewRequested = "review_requested"
	ReasonMentioned       = "mentioned"
	ReasonCIFailed        = "ci_failed"
	ReasonTodoAssigned    = "todo_assigned"
	ReasonMRMerged        = "mr_merged"
	ReasonMRCommented     = "mr_commented"
	ReasonRelease         = "release"
	ReasonWatching        = "watching"
)

// Reasons lists every reason, most specific first (a recipient with
// several reasons for one event gets the first).
var Reasons = []string{
	ReasonReviewRequested, ReasonMentioned, ReasonCIFailed, ReasonTodoAssigned,
	ReasonMRMerged, ReasonMRCommented, ReasonRelease, ReasonWatching,
}

// Defaults and limits.
const (
	DefaultDedupWindow = time.Minute
	DefaultRetention   = 90 * 24 * time.Hour
	DefaultKeep        = 1000
	DefaultPruneEvery  = time.Hour
	DefaultListLimit   = 50
	MaxListLimit       = DefaultKeep
)

// DefaultEmailBackoff are the delays before the retries of a failed email.
var DefaultEmailBackoff = []time.Duration{30 * time.Second, 2 * time.Minute, 10 * time.Minute, 30 * time.Minute}

// Deps are the collaborators of the notify package. Store is required;
// Authz enables the tree:read check (without it every recipient may read);
// Bus is needed for Start and notification.created.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Options tune the service. The zero value selects the defaults (and no
// email).
type Options struct {
	// SMTPURL configures email: smtp://[user:pass@]host[:port] (STARTTLS
	// when the server offers it, default port 587) or smtps://... (implicit
	// TLS, default port 465). "" disables email.
	SMTPURL string
	// SMTPFrom is the From address (default "W0rkTree <noreply@<host>>").
	SMTPFrom string
	// PublicURL is the server's external base URL; emails link to
	// <PublicURL>/admin<notification url>.
	PublicURL string
	// Now is the clock (nil = time.Now().UTC()).
	Now func() time.Time
	// DedupWindow, Retention and Keep (0 = defaults).
	DedupWindow time.Duration
	Retention   time.Duration
	Keep        int
	// EmailBackoff are the delays between email attempts (nil = default);
	// server-ops (§16.11): the last delay repeats until EmailDeadAfter.
	EmailBackoff []time.Duration
	// server-ops (§16.11, outbox.go): EmailDeadAfter dead-letters a queued
	// email that still fails this long after it was queued (0 = 24 h);
	// OutboxPoll bounds the worker's sleep (0 = 30 s); OutboxNow is the
	// outbox clock (nil = wall clock; notification times use Now).
	EmailDeadAfter time.Duration
	OutboxPoll     time.Duration
	OutboxNow      func() time.Time
	// EmailTimeout bounds one SMTP conversation (0 = 30 s).
	EmailTimeout time.Duration
	// SkipVerify disables TLS certificate verification for SMTP (tests).
	SkipVerify bool
}

func (o Options) withDefaults() Options {
	if o.Now == nil {
		o.Now = func() time.Time { return time.Now().UTC() }
	}
	if o.DedupWindow <= 0 {
		o.DedupWindow = DefaultDedupWindow
	}
	if o.Retention <= 0 {
		o.Retention = DefaultRetention
	}
	if o.Keep <= 0 {
		o.Keep = DefaultKeep
	}
	if o.EmailBackoff == nil {
		o.EmailBackoff = DefaultEmailBackoff
	}
	if o.EmailTimeout <= 0 {
		o.EmailTimeout = 30 * time.Second
	}
	// server-ops (§16.11)
	if o.EmailDeadAfter <= 0 {
		o.EmailDeadAfter = DefaultEmailDeadAfter
	}
	if o.OutboxPoll <= 0 {
		o.OutboxPoll = DefaultOutboxPoll
	}
	return o
}

// Service is the notify component.
type Service struct {
	d Deps

	mu     sync.Mutex
	o      Options
	mailer *mailer              // nil = email disabled
	outbox *outbox              // server-ops (§16.11): durable email queue
	recent map[string]time.Time // dedup key -> last created
	create sync.Mutex           // serializes creation (dedup + prune)
}

// New returns a Service with the default Options (no email).
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	o := Options{}.withDefaults()
	s := &Service{d: d, o: o, recent: map[string]time.Time{}}
	s.outbox = newOutbox(s.docStore(), nil, o, d.Logger) // server-ops (§16.11)
	return s
}

// docStore is Deps.Store, or an in-memory store when unset (server-ops
// §16.11: the outbox needs somewhere to queue).
func (s *Service) docStore() domain.DocStore {
	if s.d.Store != nil {
		return s.d.Store
	}
	return meta.NewMemory()
}

// Configure replaces the Options; call it before Start. It fails when
// SMTPURL or SMTPFrom is invalid.
func (s *Service) Configure(o Options) error {
	o = o.withDefaults()
	var m *mailer
	if strings.TrimSpace(o.SMTPURL) != "" {
		var err error
		if m, err = newMailer(o, s.d.Logger); err != nil {
			return err
		}
	}
	// server-ops (§16.11): the outbox sends through the mailer.
	var send func(context.Context, Email) error
	if m != nil {
		send = m.send
	}
	ob := newOutbox(s.docStore(), send, o, s.d.Logger)
	if m != nil {
		m.outbox = ob
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	s.o = o
	s.mailer = m
	s.outbox = ob
	return nil
}

func (s *Service) opts() Options {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.o
}

func (s *Service) now() time.Time { return s.opts().Now().UTC() }

// EmailEnabled reports whether SMTP is configured.
func (s *Service) EmailEnabled() bool {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.mailer != nil
}

// Error is a domain error with a client-facing message. errors.Is matches
// its Kind.
type Error struct {
	Kind error
	Msg  string
}

func (e *Error) Error() string { return e.Msg }
func (e *Error) Unwrap() error { return e.Kind }

func errorf(kind error, format string, args ...any) error {
	return &Error{Kind: kind, Msg: fmt.Sprintf(format, args...)}
}

// ---- background loop -----------------------------------------------------

// Start subscribes to the bus and handles events in order, starts the
// email sender and prunes old notifications hourly, until ctx is
// cancelled. It returns once subscribed.
func (s *Service) Start(ctx context.Context) {
	s.mu.Lock()
	m := s.mailer
	s.mu.Unlock()
	if m != nil {
		m.start(ctx)
	}
	var events <-chan domain.Event
	cancel := func() {}
	if s.d.Bus != nil {
		events, cancel = s.d.Bus.Subscribe(domain.EventFilter{})
	}
	// The bus drops events for slow subscribers; buffer generously so that
	// a burst (a merge publishing several events) is not lost while
	// notifications are being written.
	queue := make(chan domain.Event, 1024)
	go func() {
		defer cancel()
		defer close(queue)
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-events:
				if !ok {
					return
				}
				select {
				case queue <- e:
				case <-ctx.Done():
					return
				}
			}
		}
	}()
	go func() {
		prune := time.NewTicker(DefaultPruneEvery)
		defer prune.Stop()
		for {
			select {
			case <-ctx.Done():
				return
			case e, ok := <-queue:
				if !ok {
					return
				}
				if _, err := s.HandleEvent(ctx, e); err != nil {
					s.d.Logger.WarnContext(ctx, "notify: handling event failed", "type", e.Type, "tenant", e.TenantID, "err", err)
				}
			case <-prune.C:
				if err := s.Prune(ctx); err != nil {
					s.d.Logger.WarnContext(ctx, "notify: pruning failed", "err", err)
				}
			}
		}
	}()
}

// ---- storage -----------------------------------------------------------

// tsLayout is RFC 3339 with fixed-width nanoseconds, so keys sort by time.
const tsLayout = "2006-01-02T15:04:05.000000000Z07:00"

// Key is the "notifications" document key of n.
func Key(n *domain.Notification) string {
	return domain.DocKey(n.AccountID, n.CreatedAt.UTC().Format(tsLayout), n.ID)
}

// WatchKey is the "watch_settings" document key (treeID "" = the tenant).
func WatchKey(accountID, tenantID, treeID string) string {
	return domain.DocKey(accountID, tenantID, treeID)
}

func (s *Service) listOf(ctx context.Context, accountID string) ([]domain.Notification, error) {
	return meta.ListDocsT[domain.Notification](ctx, s.d.Store, CollNotifications, meta.DocPrefix(accountID))
}

// ListResult is the answer of List.
type ListResult struct {
	Notifications []domain.Notification `json:"notifications"`
	UnreadCount   int                   `json:"unread_count"`
}

// List returns up to limit notifications of accountID, newest first
// (unreadOnly drops read ones), and the account's unread count.
func (s *Service) List(ctx context.Context, accountID string, unreadOnly bool, limit int) (*ListResult, error) {
	if accountID == "" {
		return nil, errorf(domain.ErrUnauthenticated, "notifications belong to user accounts; sign in as a user")
	}
	if limit <= 0 {
		limit = DefaultListLimit
	}
	all, err := s.listOf(ctx, accountID)
	if err != nil {
		return nil, err
	}
	res := &ListResult{Notifications: []domain.Notification{}}
	cutoff := s.now().Add(-s.opts().Retention)
	for i := len(all) - 1; i >= 0; i-- {
		n := all[i]
		if n.CreatedAt.Before(cutoff) {
			continue
		}
		if n.ReadAt == nil {
			res.UnreadCount++
		} else if unreadOnly {
			continue
		}
		if len(res.Notifications) < limit {
			res.Notifications = append(res.Notifications, n)
		}
	}
	return res, nil
}

func (s *Service) find(ctx context.Context, accountID, id string) (*domain.Notification, error) {
	all, err := s.listOf(ctx, accountID)
	if err != nil {
		return nil, err
	}
	for i := range all {
		if all[i].ID == id {
			return &all[i], nil
		}
	}
	return nil, errorf(domain.ErrNotFound, "notification %q not found", id)
}

// MarkRead marks notification id of accountID read and returns it.
func (s *Service) MarkRead(ctx context.Context, accountID, id string) (*domain.Notification, error) {
	n, err := s.find(ctx, accountID, id)
	if err != nil {
		return nil, err
	}
	var out *domain.Notification
	err = meta.UpdateDocT(ctx, s.d.Store, CollNotifications, Key(n), func(cur *domain.Notification) (*domain.Notification, error) {
		if cur == nil {
			return nil, errorf(domain.ErrNotFound, "notification %q not found", id)
		}
		if cur.ReadAt == nil {
			at := s.now()
			cur.ReadAt = &at
		}
		out = cur
		return cur, nil
	})
	return out, err
}

// MarkAllRead marks every unread notification of accountID read and
// returns how many changed.
func (s *Service) MarkAllRead(ctx context.Context, accountID string) (int, error) {
	all, err := s.listOf(ctx, accountID)
	if err != nil {
		return 0, err
	}
	now := s.now()
	n := 0
	var errs []error
	for i := range all {
		if all[i].ReadAt != nil {
			continue
		}
		all[i].ReadAt = &now
		if err := s.d.Store.PutDoc(ctx, CollNotifications, Key(&all[i]), &all[i]); err != nil {
			errs = append(errs, err)
			continue
		}
		n++
	}
	return n, errors.Join(errs...)
}

// Delete removes notification id of accountID.
func (s *Service) Delete(ctx context.Context, accountID, id string) error {
	n, err := s.find(ctx, accountID, id)
	if err != nil {
		return err
	}
	if err := s.d.Store.DeleteDoc(ctx, CollNotifications, Key(n)); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return errorf(domain.ErrNotFound, "notification %q not found", id)
		}
		return err
	}
	return nil
}

// prune drops the notifications of accountID beyond Keep or older than
// Retention.
func (s *Service) prune(ctx context.Context, accountID string) error {
	all, err := s.listOf(ctx, accountID)
	if err != nil {
		return err
	}
	o := s.opts()
	cutoff := s.now().Add(-o.Retention)
	excess := len(all) - o.Keep
	var errs []error
	for i := range all {
		if i >= excess && !all[i].CreatedAt.Before(cutoff) {
			continue
		}
		if err := s.d.Store.DeleteDoc(ctx, CollNotifications, Key(&all[i])); err != nil && !errors.Is(err, domain.ErrNotFound) {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

// Prune applies the retention to every account (the background loop runs
// it hourly; new notifications prune their own account).
func (s *Service) Prune(ctx context.Context) error {
	all, err := meta.ListDocsT[domain.Notification](ctx, s.d.Store, CollNotifications, "")
	if err != nil {
		return err
	}
	seen := map[string]bool{}
	var errs []error
	for i := range all {
		if id := all[i].AccountID; !seen[id] {
			seen[id] = true
			if err := s.prune(ctx, id); err != nil {
				errs = append(errs, err)
			}
		}
	}
	s.mu.Lock()
	cutoff := s.o.Now().Add(-s.o.DedupWindow)
	for k, at := range s.recent {
		if at.Before(cutoff) {
			delete(s.recent, k)
		}
	}
	s.mu.Unlock()
	return errors.Join(errs...)
}

// ---- settings --------------------------------------------------------------

// Settings are an account's notification preferences.
type Settings struct {
	// Email sends participating notifications (review requests, mentions,
	// CI failures, TODO assignments, your merge requests) by email when the
	// server has SMTP configured and the account an email address.
	Email bool `json:"email"`
	// EmailConfigured reports whether the server can send email (read-only).
	EmailConfigured bool       `json:"email_configured"`
	UpdatedAt       *time.Time `json:"updated_at,omitempty"`
}

// SettingsPatch is the body of PUT /notifications/settings.
type SettingsPatch struct {
	Email *bool `json:"email"`
}

type settingsDoc struct {
	Email     bool      `json:"email"`
	UpdatedAt time.Time `json:"updated_at"`
}

// GetSettings returns the settings of accountID (defaults: email on).
func (s *Service) GetSettings(ctx context.Context, accountID string) (*Settings, error) {
	if accountID == "" {
		return nil, errorf(domain.ErrUnauthenticated, "notification settings belong to user accounts; sign in as a user")
	}
	out := &Settings{Email: true, EmailConfigured: s.EmailEnabled()}
	d, err := meta.GetDocT[settingsDoc](ctx, s.d.Store, CollSettings, accountID)
	switch {
	case errors.Is(err, domain.ErrNotFound):
	case err != nil:
		return nil, err
	default:
		at := d.UpdatedAt
		out.Email, out.UpdatedAt = d.Email, &at
	}
	return out, nil
}

// PutSettings applies patch to the settings of accountID.
func (s *Service) PutSettings(ctx context.Context, accountID string, patch SettingsPatch) (*Settings, error) {
	cur, err := s.GetSettings(ctx, accountID)
	if err != nil {
		return nil, err
	}
	if patch.Email != nil {
		cur.Email = *patch.Email
	}
	d := settingsDoc{Email: cur.Email, UpdatedAt: s.now()}
	if err := s.d.Store.PutDoc(ctx, CollSettings, accountID, d); err != nil {
		return nil, err
	}
	return s.GetSettings(ctx, accountID)
}

// ---- watch settings ----------------------------------------------------------

// ValidLevel reports whether level is a watch level.
func ValidLevel(level string) bool {
	switch level {
	case domain.WatchParticipating, domain.WatchWatching, domain.WatchIgnoring:
		return true
	}
	return false
}

// WatchPatch is the body of PUT .../watch.
type WatchPatch struct {
	Level string `json:"level"`
	Email *bool  `json:"email,omitempty"`
}

// GetWatch returns the stored watch setting of accountID for the tenant
// (treeID "") or tree, or the default (participating, no email, zero At).
func (s *Service) GetWatch(ctx context.Context, accountID, tenantID, treeID string) (*domain.WatchSetting, error) {
	if accountID == "" {
		return nil, errorf(domain.ErrUnauthenticated, "watch settings belong to user accounts; sign in as a user")
	}
	w, err := meta.GetDocT[domain.WatchSetting](ctx, s.d.Store, CollWatch, WatchKey(accountID, tenantID, treeID))
	if errors.Is(err, domain.ErrNotFound) {
		return &domain.WatchSetting{AccountID: accountID, TenantID: tenantID, TreeID: treeID, Level: domain.WatchParticipating}, nil
	}
	return w, err
}

// PutWatch stores the watch setting of accountID for the tenant (treeID
// "") or tree. A missing Email keeps the current value.
func (s *Service) PutWatch(ctx context.Context, accountID, tenantID, treeID string, patch WatchPatch) (*domain.WatchSetting, error) {
	if accountID == "" {
		return nil, errorf(domain.ErrUnauthenticated, "watch settings belong to user accounts; sign in as a user")
	}
	level := strings.TrimSpace(patch.Level)
	if !ValidLevel(level) {
		return nil, errorf(domain.ErrInvalid, "level must be %q, %q or %q", domain.WatchParticipating, domain.WatchWatching, domain.WatchIgnoring)
	}
	cur, err := s.GetWatch(ctx, accountID, tenantID, treeID)
	if err != nil {
		return nil, err
	}
	cur.Level = level
	if patch.Email != nil {
		cur.Email = *patch.Email
	}
	cur.At = s.now()
	if err := s.d.Store.PutDoc(ctx, CollWatch, WatchKey(accountID, tenantID, treeID), cur); err != nil {
		return nil, err
	}
	return cur, nil
}

// Watches returns every stored watch setting of accountID.
func (s *Service) Watches(ctx context.Context, accountID string) ([]domain.WatchSetting, error) {
	if accountID == "" {
		return nil, errorf(domain.ErrUnauthenticated, "watch settings belong to user accounts; sign in as a user")
	}
	return meta.ListDocsT[domain.WatchSetting](ctx, s.d.Store, CollWatch, meta.DocPrefix(accountID))
}

// effectiveWatch is the setting that applies to accountID for tree: the
// tree's own setting, else the tenant's, else the default.
func (s *Service) effectiveWatch(ctx context.Context, accountID string, tree *domain.Tree) domain.WatchSetting {
	for _, treeID := range []string{tree.ID, ""} {
		w, err := meta.GetDocT[domain.WatchSetting](ctx, s.d.Store, CollWatch, WatchKey(accountID, tree.TenantID, treeID))
		if err == nil && ValidLevel(w.Level) {
			return *w
		}
	}
	return domain.WatchSetting{AccountID: accountID, TenantID: tree.TenantID, Level: domain.WatchParticipating}
}

// watchers returns the accounts whose effective level for tree is
// watching.
func (s *Service) watchers(ctx context.Context, tree *domain.Tree) ([]string, error) {
	all, err := meta.ListDocsT[domain.WatchSetting](ctx, s.d.Store, CollWatch, "")
	if err != nil {
		return nil, err
	}
	treeLevel := map[string]string{}
	tenantLevel := map[string]string{}
	for _, w := range all {
		if w.TenantID != tree.TenantID {
			continue
		}
		switch w.TreeID {
		case tree.ID:
			treeLevel[w.AccountID] = w.Level
		case "":
			tenantLevel[w.AccountID] = w.Level
		}
	}
	var out []string
	for acct, level := range tenantLevel {
		if _, ok := treeLevel[acct]; !ok && level == domain.WatchWatching {
			out = append(out, acct)
		}
	}
	for acct, level := range treeLevel {
		if level == domain.WatchWatching {
			out = append(out, acct)
		}
	}
	slices.Sort(out)
	return out, nil
}

// deleteTree drops the watch settings and state of a deleted tree.
func (s *Service) deleteTree(ctx context.Context, tenantID, treeID string) error {
	all, err := meta.ListDocsT[domain.WatchSetting](ctx, s.d.Store, CollWatch, "")
	if err != nil {
		return err
	}
	var errs []error
	for _, w := range all {
		if w.TenantID == tenantID && w.TreeID == treeID {
			if err := s.d.Store.DeleteDoc(ctx, CollWatch, WatchKey(w.AccountID, tenantID, treeID)); err != nil && !errors.Is(err, domain.ErrNotFound) {
				errs = append(errs, err)
			}
		}
	}
	for _, kind := range []string{"todo", "release"} {
		if _, err := s.d.Store.DeleteDocs(ctx, CollState, meta.DocPrefix(kind, treeID)); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

// ---- creation ------------------------------------------------------------------

// deliver creates the notification n (AccountID, Reason set) unless it is
// a duplicate, publishes notification.created and queues the email.
// watch is the recipient's effective watch setting.
func (s *Service) deliver(ctx context.Context, n domain.Notification, acct *domain.Account, watch domain.WatchSetting) (*domain.Notification, error) {
	s.create.Lock()
	defer s.create.Unlock()
	o := s.opts()
	now := s.now()
	// An identical notification (same event, target, actor, reason and
	// text: repeated pushes re-requesting reviews, double submits, CI
	// re-reporting the same failure) is created once within the window.
	dk := strings.Join([]string{n.AccountID, n.Reason, n.Event, n.TreeID, n.URL, n.Actor, n.Title, n.Body}, "\x00")
	s.mu.Lock()
	last, dup := s.recent[dk]
	if dup && now.Sub(last) < o.DedupWindow {
		s.mu.Unlock()
		return nil, nil
	}
	s.recent[dk] = now
	s.mu.Unlock()

	n.ID = uuid.NewString()
	n.CreatedAt = now
	if err := s.d.Store.CreateDoc(ctx, CollNotifications, Key(&n), &n); err != nil {
		return nil, err
	}
	if err := s.prune(ctx, n.AccountID); err != nil {
		s.d.Logger.WarnContext(ctx, "notify: pruning failed", "account", n.AccountID, "err", err)
	}
	if s.d.Bus != nil {
		s.d.Bus.Publish(domain.Event{Type: domain.EventNotification, Actor: n.Actor, Payload: n, At: now})
	}
	s.queueEmail(ctx, &n, acct, watch)
	return &n, nil
}

// queueEmail sends n by email when SMTP is configured, the account has an
// address and wants it: participating reasons follow the account's
// settings (or an email-enabled watch setting), watching/release
// notifications the watch setting's Email.
func (s *Service) queueEmail(ctx context.Context, n *domain.Notification, acct *domain.Account, watch domain.WatchSetting) {
	s.mu.Lock()
	m := s.mailer
	s.mu.Unlock()
	if m == nil || acct == nil || strings.TrimSpace(acct.Email) == "" {
		return
	}
	want := watch.Email
	if !want && n.Reason != ReasonWatching && n.Reason != ReasonRelease {
		st, err := s.GetSettings(ctx, acct.ID)
		want = err == nil && st.Email
	}
	if !want {
		return
	}
	m.enqueue(ctx, buildEmail(*n, acct, s.opts().PublicURL))
}
