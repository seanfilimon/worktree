package sso

import (
	"net/url"
	"strings"
)

// DefaultRedirect is where a completed sign-in lands when the start
// request named no redirect.
const DefaultRedirect = "/admin/"

// maxRedirectLen bounds the post-login redirect path.
const maxRedirectLen = 1024

// SafeRedirect validates a post-login redirect target. Only same-origin
// paths inside the admin panel ("/admin" or "/admin/...") are accepted:
// the session token is appended as a URL fragment, so the landing page
// must be trusted code, not e.g. user content served from the same origin.
// Absolute and scheme-relative URLs, backslashes, control characters and
// dot segments (which browsers normalise, escaping the /admin prefix) are
// rejected. Any fragment is dropped. "" yields DefaultRedirect.
func SafeRedirect(raw string) (string, bool) {
	if raw == "" {
		return DefaultRedirect, true
	}
	if len(raw) > maxRedirectLen || raw[0] != '/' {
		return "", false
	}
	for i := 0; i < len(raw); i++ {
		if c := raw[i]; c < 0x21 || c == 0x7f || c == '\\' {
			return "", false
		}
	}
	if i := strings.IndexByte(raw, '#'); i >= 0 {
		raw = raw[:i]
	}
	u, err := url.Parse(raw)
	if err != nil || u.Scheme != "" || u.Host != "" || u.User != nil || u.Opaque != "" {
		return "", false
	}
	p := u.EscapedPath()
	if strings.Contains(strings.ToLower(p), "%2e") || strings.Contains(strings.ToLower(p), "%2f") ||
		strings.Contains(strings.ToLower(p), "%5c") {
		return "", false
	}
	for _, seg := range strings.Split(p, "/") {
		if seg == "." || seg == ".." {
			return "", false
		}
	}
	if p != "/admin" && !strings.HasPrefix(p, "/admin/") {
		return "", false
	}
	return raw, true
}
