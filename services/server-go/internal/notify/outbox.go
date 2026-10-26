package notify

// Durable email outbox (docs/ENGINEERING_PLAN.md §16.11,
// crates/worktree-protocol/specs/server/Notifications.md §7).
//
// Every email is first written to the doc collection OutboxCollection
// (key = the email id, "em-<UTC time, fixed width>-<hex>", so keys sort by
// creation) and then sent by a background worker. A failed attempt is
// retried after the next delay of Options.EmailBackoff (the last delay
// repeats); an email that still fails once Options.EmailDeadAfter (default
// 24 h) has passed since it was queued (or last retried by an admin) is
// dead-lettered. Queued emails survive restarts: the worker picks up every
// pending document when the server starts. Delivery is at least once (a
// crash between the SMTP DATA and the status write resends the email).
// Sent emails are kept 7 days and dead letters 30 days for the admin view.

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// OutboxCollection is the doc collection of queued emails (key = id).
const OutboxCollection = "outbox"

// Outbox email statuses.
const (
	OutboxPending = "pending"
	OutboxSent    = "sent"
	OutboxDead    = "dead"
)

// Outbox defaults.
const (
	DefaultEmailDeadAfter  = 24 * time.Hour
	DefaultOutboxPoll      = 30 * time.Second
	OutboxSentRetention    = 7 * 24 * time.Hour
	OutboxDeadRetention    = 30 * 24 * time.Hour
	DefaultOutboxListLimit = 100
	MaxOutboxListLimit     = 1000
	maxOutboxError         = 500
)

// ErrAlreadySent is returned by RetryEmail for an email that was sent.
var ErrAlreadySent = errors.New("notify: email already sent")

// OutboxEmail is one queued email as reported by the admin API (bodies
// omitted).
type OutboxEmail struct {
	ID string `json:"id"`
	// To is the recipient address.
	To      string `json:"to"`
	Subject string `json:"subject"`
	// NotificationID is the notification the email belongs to.
	NotificationID string `json:"notification_id,omitempty"`
	// Status is pending, sent or dead.
	Status   string `json:"status"`
	Attempts int    `json:"attempts"`
	// LastError is why the last attempt failed.
	LastError string    `json:"last_error,omitempty"`
	CreatedAt time.Time `json:"created_at"`
	UpdatedAt time.Time `json:"updated_at"`
	// NextAttemptAt is when a pending email is tried next.
	NextAttemptAt *time.Time `json:"next_attempt_at,omitempty"`
	// DeadlineAt is when a still-failing email is dead-lettered.
	DeadlineAt time.Time  `json:"deadline_at"`
	SentAt     *time.Time `json:"sent_at,omitempty"`
	DeadAt     *time.Time `json:"dead_at,omitempty"`
}

// OutboxCounts counts the outbox by status.
type OutboxCounts struct {
	Pending int `json:"pending"`
	Sent    int `json:"sent"`
	Dead    int `json:"dead"`
}

// OutboxList is the response of GET /api/v1/admin/outbox.
type OutboxList struct {
	// Enabled reports whether SMTP is configured (emails are sent).
	Enabled bool `json:"enabled"`
	// DeadAfter is the dead-letter age (Go duration) and DeadAfterSeconds
	// the same in seconds.
	DeadAfter        string       `json:"dead_after"`
	DeadAfterSeconds int64        `json:"dead_after_seconds"`
	Counts           OutboxCounts `json:"counts"`
	// Emails are newest first.
	Emails []OutboxEmail `json:"emails"`
}

// outboxDoc is the stored document: the public fields plus the message.
type outboxDoc struct {
	OutboxEmail
	ToName string `json:"to_name,omitempty"`
	Text   string `json:"text,omitempty"`
	HTML   string `json:"html,omitempty"`
}

func (d *outboxDoc) email() Email {
	return Email{To: d.To, ToName: d.ToName, Subject: d.Subject, Text: d.Text, HTML: d.HTML, ID: d.NotificationID}
}

// outbox persists emails and delivers them in the background.
type outbox struct {
	store     domain.DocStore
	send      func(context.Context, Email) error // nil = email disabled
	log       *slog.Logger
	now       func() time.Time
	backoff   []time.Duration
	deadAfter time.Duration
	poll      time.Duration

	wake  chan struct{}
	flush sync.Mutex // serializes scans and admin changes

	mu      sync.Mutex
	started bool
	lastAt  time.Time       // creation time of the last queued email
	tracked map[string]bool // ids queued by this process, not yet final
	pending sync.WaitGroup
	sent    int
	failed  int
}

func newOutbox(store domain.DocStore, send func(context.Context, Email) error, o Options, log *slog.Logger) *outbox {
	now := o.OutboxNow
	if now == nil {
		now = func() time.Time { return time.Now().UTC() }
	}
	return &outbox{
		store: store, send: send, log: log, now: now,
		backoff: o.EmailBackoff, deadAfter: o.EmailDeadAfter, poll: o.OutboxPoll,
		wake: make(chan struct{}, 1), tracked: map[string]bool{},
	}
}

func newOutboxID(at time.Time) string {
	var b [4]byte
	_, _ = rand.Read(b[:])
	return "em-" + at.UTC().Format("20060102T150405.000000000Z") + "-" + hex.EncodeToString(b[:])
}

// validOutboxID reports whether id looks like an outbox id (also a safe
// doc key).
func validOutboxID(id string) bool {
	if len(id) != len("em-20060102T150405.000000000Z-")+8 || !strings.HasPrefix(id, "em-") {
		return false
	}
	for _, c := range id[3:] {
		if !(c >= '0' && c <= '9' || c >= 'a' && c <= 'f' || c == 'T' || c == 'Z' || c == '.' || c == '-') {
			return false
		}
	}
	return true
}

func (o *outbox) signal() {
	select {
	case o.wake <- struct{}{}:
	default:
	}
}

// start runs the delivery worker until ctx ends.
func (o *outbox) start(ctx context.Context) {
	o.mu.Lock()
	if o.started || o.send == nil {
		o.mu.Unlock()
		return
	}
	o.started = true
	o.mu.Unlock()
	go func() {
		for {
			next := o.deliverDue(ctx)
			wait := o.poll
			if !next.IsZero() {
				if d := next.Sub(o.now()); d < wait {
					wait = max(d, 0)
				}
			}
			t := time.NewTimer(wait)
			select {
			case <-ctx.Done():
				t.Stop()
				return
			case <-o.wake:
			case <-t.C:
			}
			t.Stop()
		}
	}()
}

// enqueue stores msg as a pending email and wakes the worker. Without a
// running worker one attempt is made synchronously (tests); retries then
// wait for Start.
func (o *outbox) enqueue(ctx context.Context, msg Email) {
	// Strictly increasing creation times keep ids (and listings) in queue
	// order even when the clock is coarse.
	o.mu.Lock()
	now := o.now()
	if !now.After(o.lastAt) {
		now = o.lastAt.Add(time.Microsecond)
	}
	o.lastAt = now
	o.mu.Unlock()
	next := now
	d := outboxDoc{
		OutboxEmail: OutboxEmail{
			ID: newOutboxID(now), To: msg.To, Subject: msg.Subject, NotificationID: msg.ID,
			Status: OutboxPending, CreatedAt: now, UpdatedAt: now, NextAttemptAt: &next,
			DeadlineAt: now.Add(o.deadAfter),
		},
		ToName: msg.ToName, Text: msg.Text, HTML: msg.HTML,
	}
	if err := o.store.CreateDoc(context.WithoutCancel(ctx), OutboxCollection, d.ID, d); err != nil {
		o.log.ErrorContext(ctx, "notify: cannot queue email", "to", msg.To, "err", err)
		return
	}
	o.mu.Lock()
	o.tracked[d.ID] = true
	o.pending.Add(1)
	started := o.started
	o.mu.Unlock()
	if started {
		o.signal()
		return
	}
	if o.send != nil {
		o.deliverDue(ctx)
	}
}

// finish marks a tracked email final (sent or dead or deleted).
func (o *outbox) finish(id string, sent bool) {
	o.mu.Lock()
	defer o.mu.Unlock()
	if sent {
		o.sent++
	} else {
		o.failed++
	}
	if o.tracked[id] {
		delete(o.tracked, id)
		o.pending.Done()
	}
}

func (o *outbox) docs(ctx context.Context) ([]outboxDoc, error) {
	raws, err := o.store.ListDocs(ctx, OutboxCollection, "")
	if err != nil {
		return nil, fmt.Errorf("notify: list outbox: %w", err)
	}
	out := make([]outboxDoc, 0, len(raws))
	for _, raw := range raws {
		var d outboxDoc
		if json.Unmarshal(raw, &d) == nil && d.ID != "" {
			out = append(out, d)
		}
	}
	return out, nil
}

// delay returns the wait after the given number of failed attempts.
func (o *outbox) delay(attempts int) time.Duration {
	if len(o.backoff) == 0 {
		return o.poll
	}
	return o.backoff[min(attempts, len(o.backoff))-1]
}

// deliverDue attempts every due pending email, prunes old sent and dead
// emails, and returns the earliest next attempt of the remaining pending
// emails (zero when none).
func (o *outbox) deliverDue(ctx context.Context) time.Time {
	o.flush.Lock()
	defer o.flush.Unlock()
	docs, err := o.docs(ctx)
	if err != nil {
		o.log.WarnContext(ctx, "notify: outbox scan failed", "err", err)
		return o.now().Add(o.poll)
	}
	var next time.Time
	for i := range docs {
		if ctx.Err() != nil {
			return time.Time{}
		}
		d := &docs[i]
		now := o.now()
		switch d.Status {
		case OutboxSent:
			if d.SentAt != nil && now.Sub(*d.SentAt) > OutboxSentRetention {
				_ = o.store.DeleteDoc(ctx, OutboxCollection, d.ID)
			}
			continue
		case OutboxDead:
			if d.DeadAt != nil && now.Sub(*d.DeadAt) > OutboxDeadRetention {
				_ = o.store.DeleteDoc(ctx, OutboxCollection, d.ID)
			}
			continue
		}
		if d.NextAttemptAt != nil && d.NextAttemptAt.After(now) {
			if next.IsZero() || d.NextAttemptAt.Before(next) {
				next = *d.NextAttemptAt
			}
			continue
		}
		o.attempt(ctx, d)
		if d.Status == OutboxPending && d.NextAttemptAt != nil && (next.IsZero() || d.NextAttemptAt.Before(next)) {
			next = *d.NextAttemptAt
		}
	}
	return next
}

// attempt sends d once and stores the outcome.
func (o *outbox) attempt(ctx context.Context, d *outboxDoc) {
	err := o.send(ctx, d.email())
	if ctx.Err() != nil && err != nil {
		return // shutting down: the email stays pending for the next start
	}
	now := o.now()
	d.Attempts++
	d.UpdatedAt = now
	switch {
	case err == nil:
		d.Status, d.SentAt, d.NextAttemptAt, d.LastError = OutboxSent, &now, nil, ""
	case !now.Before(d.DeadlineAt):
		d.Status, d.DeadAt, d.NextAttemptAt = OutboxDead, &now, nil
		d.LastError = truncateError(err)
	default:
		next := now.Add(o.delay(d.Attempts))
		d.NextAttemptAt = &next
		d.LastError = truncateError(err)
	}
	if perr := o.store.PutDoc(context.WithoutCancel(ctx), OutboxCollection, d.ID, d); perr != nil {
		o.log.ErrorContext(ctx, "notify: cannot update queued email", "id", d.ID, "err", perr)
	}
	switch d.Status {
	case OutboxSent:
		o.finish(d.ID, true)
	case OutboxDead:
		o.log.WarnContext(ctx, "notify: email dead-lettered", "id", d.ID, "to", d.To, "attempts", d.Attempts, "err", err)
		o.finish(d.ID, false)
	default:
		o.log.InfoContext(ctx, "notify: email failed; will retry", "id", d.ID, "to", d.To,
			"attempt", d.Attempts, "retry_at", d.NextAttemptAt, "err", err)
	}
}

func truncateError(err error) string {
	s := strings.Join(strings.Fields(err.Error()), " ")
	if len(s) > maxOutboxError {
		s = s[:maxOutboxError] + "…"
	}
	return s
}

// list returns the outbox filtered by status ("" = all), newest first.
func (o *outbox) list(ctx context.Context, status string, limit int) (OutboxCounts, []OutboxEmail, error) {
	docs, err := o.docs(ctx)
	if err != nil {
		return OutboxCounts{}, nil, err
	}
	var c OutboxCounts
	out := []OutboxEmail{}
	for i := len(docs) - 1; i >= 0; i-- { // keys sort by creation
		d := docs[i]
		switch d.Status {
		case OutboxPending:
			c.Pending++
		case OutboxSent:
			c.Sent++
		case OutboxDead:
			c.Dead++
		}
		if (status == "" || d.Status == status) && len(out) < limit {
			out = append(out, d.OutboxEmail)
		}
	}
	return c, out, nil
}

func (o *outbox) load(ctx context.Context, id string) (*outboxDoc, error) {
	if !validOutboxID(id) {
		return nil, errorf(domain.ErrNotFound, "email %q not found", id)
	}
	var d outboxDoc
	err := o.store.GetDoc(ctx, OutboxCollection, id, &d)
	if errors.Is(err, domain.ErrNotFound) {
		return nil, errorf(domain.ErrNotFound, "email %q not found", id)
	}
	if err != nil {
		return nil, err
	}
	return &d, nil
}

// requeue makes d pending again with a fresh dead-letter deadline.
func (o *outbox) requeue(d *outboxDoc) {
	now := o.now()
	d.Status, d.NextAttemptAt, d.DeadAt, d.UpdatedAt = OutboxPending, &now, nil, now
	d.DeadlineAt = now.Add(o.deadAfter)
}

// retry requeues one pending or dead email for an immediate attempt.
func (o *outbox) retry(ctx context.Context, id string) (*OutboxEmail, error) {
	o.flush.Lock()
	d, err := o.load(ctx, id)
	if err == nil && d.Status == OutboxSent {
		err = ErrAlreadySent
	}
	if err == nil {
		o.requeue(d)
		err = o.store.PutDoc(ctx, OutboxCollection, d.ID, d)
	}
	o.flush.Unlock()
	if err != nil {
		return nil, err
	}
	o.signal()
	return &d.OutboxEmail, nil
}

// retryDead requeues every dead email and returns how many.
func (o *outbox) retryDead(ctx context.Context) (int, error) {
	o.flush.Lock()
	docs, err := o.docs(ctx)
	n := 0
	for i := range docs {
		if err != nil {
			break
		}
		if docs[i].Status != OutboxDead {
			continue
		}
		o.requeue(&docs[i])
		if err = o.store.PutDoc(ctx, OutboxCollection, docs[i].ID, &docs[i]); err == nil {
			n++
		}
	}
	o.flush.Unlock()
	if n > 0 {
		o.signal()
	}
	return n, err
}

// remove deletes one email.
func (o *outbox) remove(ctx context.Context, id string) error {
	o.flush.Lock()
	defer o.flush.Unlock()
	if _, err := o.load(ctx, id); err != nil {
		return err
	}
	if err := o.store.DeleteDoc(ctx, OutboxCollection, id); err != nil {
		return err
	}
	o.mu.Lock()
	if o.tracked[id] {
		delete(o.tracked, id)
		o.pending.Done()
	}
	o.mu.Unlock()
	return nil
}

// ---- Service API ---------------------------------------------------------

// Outbox lists the email outbox (status "" = every status; limit 0 =
// DefaultOutboxListLimit, capped at MaxOutboxListLimit).
func (s *Service) Outbox(ctx context.Context, status string, limit int) (*OutboxList, error) {
	switch status {
	case "", OutboxPending, OutboxSent, OutboxDead:
	default:
		return nil, errorf(domain.ErrInvalid, "status %q: want pending, sent or dead", status)
	}
	if limit <= 0 {
		limit = DefaultOutboxListLimit
	}
	limit = min(limit, MaxOutboxListLimit)
	ob := s.outboxOf()
	counts, emails, err := ob.list(ctx, status, limit)
	if err != nil {
		return nil, err
	}
	return &OutboxList{
		Enabled: s.EmailEnabled(), DeadAfter: ob.deadAfter.String(), DeadAfterSeconds: int64(ob.deadAfter.Seconds()),
		Counts: counts, Emails: emails,
	}, nil
}

// RetryEmail requeues a pending or dead email for an immediate attempt
// (with a fresh dead-letter deadline). ErrAlreadySent for sent emails,
// domain.ErrNotFound for unknown ids.
func (s *Service) RetryEmail(ctx context.Context, id string) (*OutboxEmail, error) {
	return s.outboxOf().retry(ctx, id)
}

// RetryDeadEmails requeues every dead-lettered email.
func (s *Service) RetryDeadEmails(ctx context.Context) (int, error) {
	return s.outboxOf().retryDead(ctx)
}

// DeleteEmail removes an email from the outbox.
func (s *Service) DeleteEmail(ctx context.Context, id string) error {
	return s.outboxOf().remove(ctx, id)
}

// outboxOf returns the current outbox (created by New/Configure).
func (s *Service) outboxOf() *outbox {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.outbox
}
