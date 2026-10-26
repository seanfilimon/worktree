package notify

// server-ops (docs/ENGINEERING_PLAN.md §16.11): the durable email outbox.

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"net"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// deadSMTP returns the address of a port nothing listens on.
func deadSMTP(t *testing.T) string {
	t.Helper()
	ln, err := net.Listen("tcp", "127.0.0.1:0")
	must(t, err)
	addr := ln.Addr().String()
	must(t, ln.Close())
	return addr
}

func newOutboxService(t *testing.T, store domain.MetaStore, smtpAddr string, opt func(*Options)) *Service {
	t.Helper()
	svc := New(Deps{Store: store, Logger: slog.New(slog.NewTextHandler(io.Discard, nil))})
	o := Options{SMTPURL: "smtp://" + smtpAddr, SMTPFrom: "wt@example.test", EmailTimeout: 2 * time.Second}
	if opt != nil {
		opt(&o)
	}
	must(t, svc.Configure(o))
	return svc
}

func waitFor(t *testing.T, what string, cond func() bool) {
	t.Helper()
	deadline := time.Now().Add(10 * time.Second)
	for !cond() {
		if time.Now().After(deadline) {
			t.Fatalf("timed out waiting for %s", what)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func outboxOf(t *testing.T, svc *Service, status string) *OutboxList {
	t.Helper()
	l, err := svc.Outbox(context.Background(), status, 0)
	must(t, err)
	return l
}

func TestOutboxSurvivesRestart(t *testing.T) {
	store := meta.NewMemory()
	msg := Email{To: "alice@x.test", ToName: "Alice", Subject: "hello", Text: "hi", HTML: "<p>hi</p>", ID: "n-1"}

	// SMTP is down: the email stays pending with the error.
	ctx1, stop1 := context.WithCancel(context.Background())
	first := newOutboxService(t, store, deadSMTP(t), func(o *Options) { o.EmailBackoff = []time.Duration{time.Hour} })
	first.Start(ctx1)
	first.mailer.enqueue(ctx1, msg)
	waitFor(t, "a failed attempt", func() bool {
		l := outboxOf(t, first, OutboxPending)
		return len(l.Emails) == 1 && l.Emails[0].Attempts == 1
	})
	l := outboxOf(t, first, "")
	e := l.Emails[0]
	if !l.Enabled || l.Counts != (OutboxCounts{Pending: 1}) || e.To != "alice@x.test" || e.Subject != "hello" ||
		e.NotificationID != "n-1" || e.LastError == "" || e.NextAttemptAt == nil || time.Until(*e.NextAttemptAt) < 50*time.Minute ||
		e.DeadlineAt.Sub(e.CreatedAt) != DefaultEmailDeadAfter || l.DeadAfter != "24h0m0s" || l.DeadAfterSeconds != 86400 {
		t.Fatalf("outbox after a failure = %+v %+v", l, e)
	}
	if !validOutboxID(e.ID) {
		t.Fatalf("id %q", e.ID)
	}
	stop1()

	// A new server process on the same store with a working SMTP server
	// delivers it after an admin retry (its next attempt is an hour away).
	smtpd := newFakeSMTP(t, 0, false)
	ctx2, stop2 := context.WithCancel(context.Background())
	defer stop2()
	second := newOutboxService(t, store, smtpd.addr(), nil)
	second.Start(ctx2)
	if _, err := second.RetryEmail(ctx2, e.ID); err != nil {
		t.Fatal(err)
	}
	waitFor(t, "the queued email to be sent", func() bool { return len(smtpd.list()) == 1 })
	waitFor(t, "the sent status", func() bool { return outboxOf(t, second, OutboxSent).Counts.Sent == 1 })
	got := outboxOf(t, second, OutboxSent).Emails[0]
	if got.ID != e.ID || got.Attempts != 2 || got.SentAt == nil || got.LastError != "" || got.NextAttemptAt != nil {
		t.Fatalf("sent = %+v", got)
	}
	m := smtpd.list()[0]
	if m.To[0] != "alice@x.test" || !strings.Contains(m.Data, "Subject: hello") || !strings.Contains(m.Data, "X-WT-Notification: n-1") {
		t.Fatalf("message = %+v", m)
	}
	if _, err := second.RetryEmail(ctx2, e.ID); !errors.Is(err, ErrAlreadySent) {
		t.Fatalf("retry sent = %v", err)
	}
}

func TestOutboxPendingAtStartIsSent(t *testing.T) {
	store := meta.NewMemory()
	// Queued without a running worker and without SMTP reachable.
	queued := newOutboxService(t, store, deadSMTP(t), func(o *Options) { o.EmailBackoff = []time.Duration{time.Millisecond} })
	queued.mailer.enqueue(context.Background(), Email{To: "bob@x.test", Subject: "queued"})
	if l := outboxOf(t, queued, ""); l.Counts.Pending != 1 || l.Emails[0].Attempts != 1 {
		t.Fatalf("queued = %+v", l)
	}
	smtpd := newFakeSMTP(t, 0, false)
	ctx, stop := context.WithCancel(context.Background())
	defer stop()
	svc := newOutboxService(t, store, smtpd.addr(), func(o *Options) { o.OutboxPoll = 10 * time.Millisecond })
	svc.Start(ctx)
	waitFor(t, "the pending email to be sent on start", func() bool { return len(smtpd.list()) == 1 })
}

func TestOutboxRetriesThenDeadLetters(t *testing.T) {
	smtpd := newFakeSMTP(t, 1000, false)
	ctx, stop := context.WithCancel(context.Background())
	defer stop()
	svc := newOutboxService(t, meta.NewMemory(), smtpd.addr(), func(o *Options) {
		o.EmailBackoff = []time.Duration{2 * time.Millisecond, 4 * time.Millisecond}
		o.EmailDeadAfter = 100 * time.Millisecond
	})
	svc.Start(ctx)
	svc.mailer.enqueue(ctx, Email{To: "carol@x.test", Subject: "doomed"})
	svc.mailer.enqueue(ctx, Email{To: "dave@x.test", Subject: "doomed too"})
	sent, failed := waitEmails(t, svc)
	if sent != 0 || failed != 2 {
		t.Fatalf("sent %d failed %d", sent, failed)
	}
	dead := outboxOf(t, svc, OutboxDead)
	if dead.Counts != (OutboxCounts{Dead: 2}) || len(dead.Emails) != 2 || dead.Emails[0].Subject != "doomed too" {
		t.Fatalf("dead = %+v", dead)
	}
	for _, e := range dead.Emails {
		if e.Attempts < 1 || e.DeadAt == nil || e.NextAttemptAt != nil || !strings.Contains(e.LastError, "451") {
			t.Fatalf("dead email = %+v", e)
		}
	}

	// SMTP recovers; an admin requeues every dead letter.
	smtpd.mu.Lock()
	smtpd.failFirst = 0
	smtpd.mu.Unlock()
	n, err := svc.RetryDeadEmails(ctx)
	if err != nil || n != 2 {
		t.Fatalf("retry dead = %d, %v", n, err)
	}
	waitFor(t, "requeued emails to be sent", func() bool { return outboxOf(t, svc, "").Counts.Sent == 2 })
	if n, err := svc.RetryDeadEmails(ctx); err != nil || n != 0 {
		t.Fatalf("retry dead again = %d, %v", n, err)
	}

	// Filters, limits, delete and errors.
	if l, err := svc.Outbox(ctx, OutboxSent, 1); err != nil || len(l.Emails) != 1 || l.Counts.Sent != 2 {
		t.Fatalf("limit 1 = %+v, %v", l, err)
	}
	if _, err := svc.Outbox(ctx, "bogus", 0); !errors.Is(err, domain.ErrInvalid) {
		t.Fatalf("bad status = %v", err)
	}
	id := outboxOf(t, svc, "").Emails[0].ID
	must(t, svc.DeleteEmail(ctx, id))
	if err := svc.DeleteEmail(ctx, id); !errors.Is(err, domain.ErrNotFound) {
		t.Fatalf("second delete = %v", err)
	}
	for _, bad := range []string{"", "em-x", "../../x", strings.Repeat("a", 40)} {
		if _, err := svc.RetryEmail(ctx, bad); !errors.Is(err, domain.ErrNotFound) {
			t.Errorf("retry %q = %v", bad, err)
		}
	}
	if l := outboxOf(t, svc, ""); len(l.Emails) != 1 {
		t.Fatalf("after delete = %+v", l)
	}
}

func TestOutboxWithoutSMTP(t *testing.T) {
	svc := New(Deps{Store: meta.NewMemory()})
	l := outboxOf(t, svc, "")
	if l.Enabled || l.Counts != (OutboxCounts{}) || l.Emails == nil || l.DeadAfter != "24h0m0s" {
		t.Fatalf("outbox = %+v", l)
	}
	if n, err := svc.RetryDeadEmails(context.Background()); n != 0 || err != nil {
		t.Fatalf("retry = %d %v", n, err)
	}
	// Nil store: an in-memory queue.
	if l := outboxOf(t, New(Deps{}), ""); l.Enabled {
		t.Fatalf("outbox = %+v", l)
	}
}

func TestOutboxDelay(t *testing.T) {
	o := &outbox{backoff: []time.Duration{time.Second, time.Minute}, poll: time.Hour}
	for attempts, want := range map[int]time.Duration{1: time.Second, 2: time.Minute, 3: time.Minute, 10: time.Minute} {
		if got := o.delay(attempts); got != want {
			t.Errorf("delay(%d) = %s, want %s", attempts, got, want)
		}
	}
	if (&outbox{poll: time.Hour}).delay(1) != time.Hour {
		t.Error("no backoff = poll interval")
	}
	if s := truncateError(errors.New(strings.Repeat("x", 600) + "\n")); len(s) > maxOutboxError+4 {
		t.Errorf("truncated to %d", len(s))
	}
}
