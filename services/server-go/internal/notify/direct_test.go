package notify

import (
	"context"
	"strings"
	"testing"
	"time"
)

// tenant-lifecycle (§16.4): transactional mail through the notify mailer.
func TestSendMail(t *testing.T) {
	off := newEnv(t)
	if off.svc.SendMail(off.ctx, "ops@acme.io", "", "s", "t", "h") {
		t.Fatal("SendMail without SMTP reported success")
	}
	var nilSvc *Service
	if nilSvc.SendMail(context.Background(), "a@b.io", "", "", "", "") {
		t.Fatal("nil service sent mail")
	}

	smtpd := newFakeSMTP(t, 0, false)
	e := newEnv(t, func(o *Options) {
		o.SMTPURL = "smtp://" + smtpd.addr()
		o.PublicURL = "https://wt.example/"
		o.EmailBackoff = []time.Duration{time.Millisecond}
	})
	ctx, cancel := context.WithCancel(e.ctx)
	defer cancel()
	e.svc.Start(ctx)
	if !e.svc.SendMail(e.ctx, "ops@acme.io", "Ops", "Verify the contact email", "code wte_x", "<p>code</p>") {
		t.Fatal("SendMail with SMTP reported failure")
	}
	if sent, failed := waitEmails(t, e.svc); sent != 1 || failed != 0 {
		t.Fatalf("sent %d failed %d", sent, failed)
	}
	msgs := smtpd.list()
	if len(msgs) != 1 || msgs[0].To[0] != "ops@acme.io" || !strings.Contains(msgs[0].Data, "Verify the contact email") {
		t.Fatalf("messages = %+v", msgs)
	}
}
