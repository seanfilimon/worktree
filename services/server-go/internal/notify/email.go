package notify

import (
	"bytes"
	"context"
	"crypto/rand"
	"crypto/tls"
	"encoding/hex"
	"errors"
	"fmt"
	htmltemplate "html/template"
	"log/slog"
	"mime"
	"mime/quotedprintable"
	"net"
	"net/mail"
	"net/smtp"
	"net/url"
	"strings"
	texttemplate "text/template"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// ParseSMTPURL validates an --smtp-url value and returns its parts.
// Accepted forms: smtp://[user[:pass]@]host[:port] (default port 587,
// STARTTLS when offered) and smtps://... (implicit TLS, default 465).
func ParseSMTPURL(raw string) (*url.URL, error) {
	u, err := url.Parse(strings.TrimSpace(raw))
	if err != nil || (u.Scheme != "smtp" && u.Scheme != "smtps") || u.Hostname() == "" ||
		(u.Path != "" && u.Path != "/") || u.Opaque != "" {
		return nil, fmt.Errorf("smtp-url %q: want smtp://[user:pass@]host[:port] or smtps://...", raw)
	}
	if u.Port() == "" {
		port := "587"
		if u.Scheme == "smtps" {
			port = "465"
		}
		u.Host = net.JoinHostPort(u.Hostname(), port)
	}
	return u, nil
}

// ParseFrom validates an --smtp-from value ("" is accepted: the default).
func ParseFrom(from string) (*mail.Address, error) {
	if strings.TrimSpace(from) == "" {
		return nil, nil
	}
	a, err := mail.ParseAddress(from)
	if err != nil {
		return nil, fmt.Errorf("smtp-from %q: %v", from, err)
	}
	return a, nil
}

// Email is one message to send.
type Email struct {
	To      string // address
	ToName  string
	Subject string
	Text    string
	HTML    string
	ID      string // notification id (X-WT-Notification)
}

// mailer sends Emails over SMTP. server-ops (§16.11): queueing, retries
// and dead-lettering live in the durable outbox (outbox.go).
type mailer struct {
	u          *url.URL
	from       mail.Address
	timeout    time.Duration
	skipVerify bool
	log        *slog.Logger

	outbox *outbox // set by Service.Configure
}

func newMailer(o Options, log *slog.Logger) (*mailer, error) {
	u, err := ParseSMTPURL(o.SMTPURL)
	if err != nil {
		return nil, err
	}
	from, err := ParseFrom(o.SMTPFrom)
	if err != nil {
		return nil, err
	}
	if from == nil {
		host := "localhost"
		if pu, err := url.Parse(o.PublicURL); err == nil && pu.Hostname() != "" {
			host = pu.Hostname()
		}
		from = &mail.Address{Name: "W0rkTree", Address: "noreply@" + host}
	}
	return &mailer{u: u, from: *from, timeout: o.EmailTimeout, skipVerify: o.SkipVerify, log: log}, nil
}

// start starts the outbox worker (server-ops §16.11).
func (m *mailer) start(ctx context.Context) {
	if m.outbox != nil {
		m.outbox.start(ctx)
	}
}

// enqueue writes msg to the durable outbox, which sends it in the
// background (server-ops §16.11).
func (m *mailer) enqueue(ctx context.Context, msg Email) {
	if m.outbox != nil {
		m.outbox.enqueue(ctx, msg)
	}
}

// send runs one SMTP conversation.
func (m *mailer) send(ctx context.Context, msg Email) error {
	host := m.u.Hostname()
	tlsCfg := &tls.Config{ServerName: host, MinVersion: tls.VersionTLS12, InsecureSkipVerify: m.skipVerify} //nolint:gosec // opt-in for tests
	d := net.Dialer{Timeout: m.timeout}
	var conn net.Conn
	var err error
	if m.u.Scheme == "smtps" {
		conn, err = tls.DialWithDialer(&d, "tcp", m.u.Host, tlsCfg)
	} else {
		conn, err = d.DialContext(ctx, "tcp", m.u.Host)
	}
	if err != nil {
		return err
	}
	_ = conn.SetDeadline(time.Now().Add(m.timeout))
	c, err := smtp.NewClient(conn, host)
	if err != nil {
		conn.Close()
		return err
	}
	defer c.Close()
	if err := c.Hello("worktree"); err != nil {
		return err
	}
	if m.u.Scheme == "smtp" {
		if ok, _ := c.Extension("STARTTLS"); ok {
			if err := c.StartTLS(tlsCfg); err != nil {
				return fmt.Errorf("starttls: %w", err)
			}
		}
	}
	if user := m.u.User; user != nil && user.Username() != "" {
		if ok, _ := c.Extension("AUTH"); !ok {
			return errors.New("smtp server does not support AUTH but credentials were configured")
		}
		pass, _ := user.Password()
		if err := c.Auth(smtp.PlainAuth("", user.Username(), pass, host)); err != nil {
			return fmt.Errorf("auth: %w", err)
		}
	}
	if err := c.Mail(m.from.Address); err != nil {
		return err
	}
	if err := c.Rcpt(msg.To); err != nil {
		return err
	}
	w, err := c.Data()
	if err != nil {
		return err
	}
	if _, err := w.Write(m.render(msg)); err != nil {
		return err
	}
	if err := w.Close(); err != nil {
		return err
	}
	return c.Quit()
}

func randomHex(n int) string {
	b := make([]byte, n)
	_, _ = rand.Read(b)
	return hex.EncodeToString(b)
}

func qp(s string) string {
	var b bytes.Buffer
	w := quotedprintable.NewWriter(&b)
	_, _ = w.Write([]byte(strings.ReplaceAll(s, "\n", "\r\n")))
	_ = w.Close()
	return b.String()
}

// render builds the RFC 5322 message: multipart/alternative with a
// plain-text and an HTML part, both quoted-printable UTF-8.
func (m *mailer) render(msg Email) []byte {
	boundary := "wt-" + randomHex(12)
	to := mail.Address{Name: msg.ToName, Address: msg.To}
	var b strings.Builder
	h := func(k, v string) { b.WriteString(k + ": " + v + "\r\n") }
	h("From", m.from.String())
	h("To", to.String())
	subject := strings.Join(strings.Fields(msg.Subject), " ") // single line: no header injection
	h("Subject", mime.QEncoding.Encode("utf-8", subject))
	h("Date", time.Now().UTC().Format(time.RFC1123Z))
	domainPart := m.from.Address[strings.LastIndex(m.from.Address, "@")+1:]
	h("Message-ID", "<"+randomHex(16)+"@"+domainPart+">")
	h("MIME-Version", "1.0")
	h("Auto-Submitted", "auto-generated")
	if msg.ID != "" {
		h("X-WT-Notification", msg.ID)
	}
	h("Content-Type", `multipart/alternative; boundary="`+boundary+`"`)
	b.WriteString("\r\n")
	for _, part := range []struct{ typ, body string }{{"text/plain", msg.Text}, {"text/html", msg.HTML}} {
		b.WriteString("--" + boundary + "\r\n")
		b.WriteString("Content-Type: " + part.typ + "; charset=utf-8\r\n")
		b.WriteString("Content-Transfer-Encoding: quoted-printable\r\n\r\n")
		b.WriteString(qp(part.body))
		b.WriteString("\r\n")
	}
	b.WriteString("--" + boundary + "--\r\n")
	return []byte(b.String())
}

// ---- templates -----------------------------------------------------------------

var reasonText = map[string]string{
	ReasonReviewRequested: "your review was requested",
	ReasonMentioned:       "you were mentioned",
	ReasonCIFailed:        "CI failed on a merge request you authored",
	ReasonTodoAssigned:    "a TODO was assigned to you",
	ReasonMRMerged:        "your merge request was merged",
	ReasonMRCommented:     "there is activity on your merge request",
	ReasonRelease:         "you are watching this tree",
	ReasonWatching:        "you are watching this tree",
}

type emailData struct {
	N      domain.Notification
	Name   string
	Link   string
	Why    string
	Inbox  string
	Prefix string
}

var textTmpl = texttemplate.Must(texttemplate.New("text").Parse(`Hi {{.Name}},

{{.N.Title}}
{{if .N.Body}}
{{.N.Body}}
{{end}}{{if .Link}}
Open: {{.Link}}
{{end}}
--
You are receiving this because {{.Why}}.{{if .Inbox}}
Manage notifications: {{.Inbox}}{{end}}
`))

var htmlTmpl = htmltemplate.Must(htmltemplate.New("html").Parse(`<!doctype html>
<html><body style="font-family:-apple-system,Segoe UI,Helvetica,Arial,sans-serif;color:#111;background:#fff">
<p>Hi {{.Name}},</p>
<p style="font-size:16px;font-weight:600">{{if .Link}}<a href="{{.Link}}" style="color:#111">{{.N.Title}}</a>{{else}}{{.N.Title}}{{end}}</p>
{{if .N.Body}}<blockquote style="margin:0;padding:8px 12px;border-left:3px solid #ddd;color:#333;white-space:pre-wrap">{{.N.Body}}</blockquote>{{end}}
{{if .Link}}<p><a href="{{.Link}}" style="display:inline-block;padding:6px 12px;border:1px solid #111;border-radius:6px;color:#111;text-decoration:none">Open in W0rkTree</a></p>{{end}}
<hr style="border:none;border-top:1px solid #eee">
<p style="font-size:12px;color:#666">You are receiving this because {{.Why}}.{{if .Inbox}} <a href="{{.Inbox}}" style="color:#666">Manage notifications</a>.{{end}}</p>
</body></html>
`))

// buildEmail renders the email of n for acct. Links are absolute when
// publicURL is set.
func buildEmail(n domain.Notification, acct *domain.Account, publicURL string) Email {
	base := strings.TrimRight(publicURL, "/")
	d := emailData{N: n, Name: acct.DisplayName, Why: reasonText[n.Reason]}
	if d.Name == "" {
		d.Name = acct.Username
	}
	if d.Why == "" {
		d.Why = "of your notification settings"
	}
	if base != "" {
		if n.URL != "" {
			d.Link = base + "/admin" + n.URL
		}
		d.Inbox = base + "/admin/notifications"
	}
	var text, html bytes.Buffer
	_ = textTmpl.Execute(&text, d)
	_ = htmlTmpl.Execute(&html, d)
	subject := n.Title
	if i := strings.Index(n.URL, "/trees/"); i == 0 {
		parts := strings.SplitN(strings.TrimPrefix(n.URL, "/trees/"), "/", 3)
		if len(parts) >= 2 {
			subject = "[" + parts[0] + "/" + parts[1] + "] " + subject
		}
	}
	return Email{To: acct.Email, ToName: acct.DisplayName, Subject: subject, Text: text.String(), HTML: html.String(), ID: n.ID}
}

// WaitEmails blocks until every email queued by this process was sent or
// dead-lettered, or ctx ends (tests, shutdown). It returns the sent and
// dead-lettered totals.
func (s *Service) WaitEmails(ctx context.Context) (sent, failed int) {
	s.mu.Lock()
	m, ob := s.mailer, s.outbox
	s.mu.Unlock()
	if m == nil || ob == nil {
		return 0, 0
	}
	done := make(chan struct{})
	go func() { ob.pending.Wait(); close(done) }()
	select {
	case <-done:
	case <-ctx.Done():
	}
	ob.mu.Lock()
	defer ob.mu.Unlock()
	return ob.sent, ob.failed
}
