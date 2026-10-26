package notify

import "context"

// ---- tenant-lifecycle (docs/ENGINEERING_PLAN.md §16.4) ----
//
// SendMail queues a transactional email (email verification) through the
// notification mailer, independent of notification settings. It reports
// false, sending nothing, when SMTP is not configured (the caller then
// logs the message instead).
func (s *Service) SendMail(ctx context.Context, to, toName, subject, text, html string) bool {
	if s == nil {
		return false
	}
	s.mu.Lock()
	m := s.mailer
	s.mu.Unlock()
	if m == nil || to == "" {
		return false
	}
	m.enqueue(context.WithoutCancel(ctx), Email{To: to, ToName: toName, Subject: subject, Text: text, HTML: html})
	return true
}

// ---- end tenant-lifecycle ----
