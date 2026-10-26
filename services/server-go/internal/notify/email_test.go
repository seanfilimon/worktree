package notify

import (
	"context"
	"encoding/base64"
	"io"
	"mime"
	"mime/multipart"
	"net/mail"
	"strings"
	"testing"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestParseSMTPURL(t *testing.T) {
	for raw, host := range map[string]string{
		"smtp://mail.test":              "mail.test:587",
		"smtps://mail.test":             "mail.test:465",
		"smtp://u:p@mail.test:2525":     "mail.test:2525",
		"smtp://[::1]:25":               "[::1]:25",
		"  smtp://mail.test:25/  ":      "mail.test:25",
		"smtps://user%40x:p%3Ass@m.t:1": "m.t:1",
	} {
		u, err := ParseSMTPURL(raw)
		if err != nil || u.Host != host {
			t.Errorf("ParseSMTPURL(%q) = %v, %v; want host %s", raw, u, err, host)
		}
	}
	for _, raw := range []string{"", "mail.test", "http://mail.test", "smtp://", "smtp://h/path", "smtp:opaque"} {
		if _, err := ParseSMTPURL(raw); err == nil {
			t.Errorf("ParseSMTPURL(%q) accepted", raw)
		}
	}
	if _, err := ParseFrom("not an address"); err == nil {
		t.Error("ParseFrom accepted garbage")
	}
	if a, err := ParseFrom("WT <wt@x.test>"); err != nil || a.Address != "wt@x.test" {
		t.Errorf("ParseFrom = %v, %v", a, err)
	}
	if err := New(Deps{}).Configure(Options{SMTPURL: "ftp://x"}); err == nil {
		t.Error("Configure accepted a bad smtp url")
	}
}

// parsed is a received email decoded into headers and its two parts.
type parsed struct {
	header mail.Header
	text   string
	html   string
}

func parseMessage(t *testing.T, data string) parsed {
	t.Helper()
	m, err := mail.ReadMessage(strings.NewReader(data))
	if err != nil {
		t.Fatalf("read message: %v\n%s", err, data)
	}
	mt, params, err := mime.ParseMediaType(m.Header.Get("Content-Type"))
	if err != nil || mt != "multipart/alternative" {
		t.Fatalf("content type %q: %v", m.Header.Get("Content-Type"), err)
	}
	out := parsed{header: m.Header}
	mr := multipart.NewReader(m.Body, params["boundary"])
	for {
		p, err := mr.NextPart()
		if err == io.EOF {
			break
		}
		if err != nil {
			t.Fatal(err)
		}
		b, _ := io.ReadAll(p) // quoted-printable is decoded by the reader
		switch {
		case strings.HasPrefix(p.Header.Get("Content-Type"), "text/plain"):
			out.text = string(b)
		case strings.HasPrefix(p.Header.Get("Content-Type"), "text/html"):
			out.html = string(b)
		}
	}
	return out
}

func waitEmails(t *testing.T, svc *Service) (int, int) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	return svc.WaitEmails(ctx)
}

func TestEmailDeliveryWithRetries(t *testing.T) {
	smtpd := newFakeSMTP(t, 1, true)
	e := newEnv(t, func(o *Options) {
		o.SMTPURL = "smtp://wt:s3cret@" + smtpd.addr()
		o.PublicURL = "https://wt.example/"
		o.EmailBackoff = []time.Duration{10 * time.Millisecond, 10 * time.Millisecond}
	})
	if !e.svc.EmailEnabled() {
		t.Fatal("email not enabled")
	}
	ctx, cancel := context.WithCancel(e.ctx)
	defer cancel()
	e.svc.Start(ctx)

	// carol turned email off; alice keeps the default.
	off := false
	_, err := e.svc.PutSettings(e.ctx, "a-carol", SettingsPatch{Email: &off})
	must(t, err)
	ev := e.event(domain.EventMRReviewRequested, "bob", map[string]any{"number": 1, "reviewers": []string{"team:core", "user:alice"}, "requested_by": "bob"})
	want(t, "review requested", e.handle(ev), "alice:review_requested", "carol:review_requested")
	if sent, failed := waitEmails(t, e.svc); sent != 1 || failed != 0 {
		t.Fatalf("sent %d failed %d", sent, failed)
	}
	msgs := smtpd.list()
	if len(msgs) != 1 {
		t.Fatalf("messages = %+v", msgs)
	}
	m := msgs[0]
	if m.From != "noreply@wt.example" || len(m.To) != 1 || m.To[0] != "alice@x.test" {
		t.Fatalf("envelope = %+v", m)
	}
	if auth, _ := base64.StdEncoding.DecodeString(m.Auth); string(auth) != "\x00wt\x00s3cret" {
		t.Fatalf("AUTH PLAIN = %q", auth)
	}
	p := parseMessage(t, m.Data)
	subj, _ := new(mime.WordDecoder).DecodeHeader(p.header.Get("Subject"))
	if subj != "[acme/web] bob requested a review on !1 Add login" {
		t.Fatalf("subject = %q", subj)
	}
	if p.header.Get("Auto-Submitted") != "auto-generated" || p.header.Get("X-WT-Notification") == "" {
		t.Fatalf("headers = %v", p.header)
	}
	link := "https://wt.example/admin/trees/acme/web/merge-requests/1"
	if !strings.Contains(p.text, "Hi Alice,") || !strings.Contains(p.text, "Open: "+link) ||
		!strings.Contains(p.text, "your review was requested") || !strings.Contains(p.text, "https://wt.example/admin/notifications") {
		t.Fatalf("text part:\n%s", p.text)
	}
	if !strings.Contains(p.html, `href="`+link+`"`) || !strings.Contains(p.html, "Open in W0rkTree") {
		t.Fatalf("html part:\n%s", p.html)
	}

	// Watching notifications follow the watch setting's email flag.
	email := true
	_, err = e.svc.PutWatch(e.ctx, "a-carol", e.acme.ID, e.web.ID, WatchPatch{Level: domain.WatchWatching, Email: &email})
	must(t, err)
	_, err = e.svc.PutWatch(e.ctx, "a-alice", e.acme.ID, e.web.ID, WatchPatch{Level: domain.WatchWatching})
	must(t, err)
	mr := e.mr
	mr.Description = "<script>alert(1)</script>"
	want(t, "opened", e.handle(e.event(domain.EventMROpened, "bob", mr)), "alice:watching", "carol:watching")
	if sent, failed := waitEmails(t, e.svc); sent != 2 || failed != 0 {
		t.Fatalf("sent %d failed %d", sent, failed)
	}
	msgs = smtpd.list()
	if len(msgs) != 2 || msgs[1].To[0] != "carol@x.test" {
		t.Fatalf("messages = %+v", msgs)
	}
	if p := parseMessage(t, msgs[1].Data); strings.Contains(p.html, "<script>") || !strings.Contains(p.html, "&lt;script&gt;") {
		t.Fatalf("html not escaped:\n%s", p.html)
	}
}

func TestEmailGivesUp(t *testing.T) {
	smtpd := newFakeSMTP(t, 100, false)
	e := newEnv(t, func(o *Options) {
		o.SMTPURL = "smtp://" + smtpd.addr()
		o.SMTPFrom = "W0rkTree <wt@example.test>"
		// server-ops (§16.11): emails are dead-lettered by age, not attempts.
		o.EmailDeadAfter = 20 * time.Millisecond
		o.EmailBackoff = []time.Duration{5 * time.Millisecond}
	})
	ctx, cancel := context.WithCancel(e.ctx)
	defer cancel()
	e.svc.Start(ctx)
	want(t, "merged", e.handle(e.event(domain.EventMRMerged, "alice", e.mr)), "bob:mr_merged")
	if sent, failed := waitEmails(t, e.svc); sent != 0 || failed != 1 {
		t.Fatalf("sent %d failed %d", sent, failed)
	}
	if n := len(smtpd.list()); n != 0 {
		t.Fatalf("%d messages accepted", n)
	}
}

func TestNoEmailWithoutSMTP(t *testing.T) {
	e := newEnv(t)
	want(t, "merged", e.handle(e.event(domain.EventMRMerged, "alice", e.mr)), "bob:mr_merged")
	if sent, failed := waitEmails(t, e.svc); sent != 0 || failed != 0 {
		t.Fatalf("sent %d failed %d", sent, failed)
	}
}
