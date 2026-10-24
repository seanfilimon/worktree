package audit

import (
	"context"
	"net"
	"net/http"
	"strings"
	"unicode"

	"google.golang.org/grpc/peer"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Common action names ("<resource>.<verb>"). Callers may use any string;
// these exist to keep the vocabulary consistent across packages.
const (
	ActionAuthLogin        = "auth.login"
	ActionAuthLoginFailed  = "auth.login_failed"
	ActionAuthLogout       = "auth.logout"
	ActionAuthRegister     = "auth.register"
	ActionTokenCreate      = "token.create"
	ActionTokenRevoke      = "token.revoke"
	ActionTenantCreate     = "tenant.create"
	ActionMemberPut        = "member.put"
	ActionMemberRemove     = "member.remove"
	ActionTreeCreate       = "tree.create"
	ActionTreeUpdate       = "tree.update"
	ActionTreeDelete       = "tree.delete"
	ActionBranchPush       = "branch.push"
	ActionBranchCreate     = "branch.create"
	ActionBranchDelete     = "branch.delete"
	ActionTagCreate        = "tag.create"
	ActionTagDelete        = "tag.delete"
	ActionProtectionPut    = "protection.put"
	ActionProtectionDelete = "protection.delete"
	ActionLicensePut       = "license.put"
	ActionLicenseDelete    = "license.delete"
	ActionLicenseDenied    = "license.denied"
	ActionMROpen           = "mr.open"
	ActionMRApprove        = "mr.approve"
	ActionMRMerge          = "mr.merge"
	ActionMRClose          = "mr.close"
	ActionAccountUpdate    = "account.update"
	ActionAccessDenied     = "access.denied"
)

// FromPrincipal builds an event attributed to p (Actor "anonymous" with an
// empty ActorID when p is nil). TenantID is left empty: set it to the
// tenant that owns the resource (not the actor's personal tenant) so tenant
// admins can query it. details is copied, never retained.
func FromPrincipal(p *domain.Principal, action, resourceType, resourceID string, details map[string]string) domain.AuditEvent {
	e := domain.AuditEvent{
		Actor:        ActorAnonymous,
		Action:       action,
		ResourceType: resourceType,
		ResourceID:   resourceID,
	}
	if p != nil {
		e.ActorID = p.AccountID
		if p.Username != "" {
			e.Actor = p.Username
		}
	}
	if len(details) > 0 {
		e.Details = make(map[string]string, len(details))
		for k, v := range details {
			e.Details[k] = v
		}
	}
	return e
}

// System builds an event attributed to the server itself (Actor "system").
func System(action, resourceType, resourceID string, details map[string]string) domain.AuditEvent {
	e := FromPrincipal(nil, action, resourceType, resourceID, details)
	e.Actor = ActorSystem
	return e
}

type remoteAddrKey struct{}

// WithRemoteAddr returns a context carrying the client address used to fill
// AuditEvent.RemoteAddr. A "host:port" address is reduced to its host.
func WithRemoteAddr(ctx context.Context, addr string) context.Context {
	return context.WithValue(ctx, remoteAddrKey{}, hostOnly(addr))
}

// RemoteAddr returns the client address for ctx: the value set by
// WithRemoteAddr, else the gRPC peer address, else "".
func RemoteAddr(ctx context.Context) string {
	if ctx == nil {
		return ""
	}
	if v, ok := ctx.Value(remoteAddrKey{}).(string); ok && v != "" {
		return v
	}
	if p, ok := peer.FromContext(ctx); ok && p.Addr != nil {
		return hostOnly(p.Addr.String())
	}
	return ""
}

// Middleware attaches r.RemoteAddr to each request context (see
// WithRemoteAddr). Forwarding headers are deliberately ignored: they are
// client-controlled and must only be trusted behind a known proxy.
func Middleware(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		next.ServeHTTP(w, r.WithContext(WithRemoteAddr(r.Context(), r.RemoteAddr)))
	})
}

func hostOnly(addr string) string {
	addr = strings.TrimSpace(addr)
	if host, _, err := net.SplitHostPort(addr); err == nil {
		return host
	}
	return addr
}

// Redacted replaces the value of sensitive detail keys.
const Redacted = "[REDACTED]"

// Redact returns a copy of details in which values of sensitive keys (see
// IsSensitiveKey) and bearer credentials are replaced with Redacted. It
// returns nil for an empty map.
func Redact(details map[string]string) map[string]string {
	if len(details) == 0 {
		return nil
	}
	out := make(map[string]string, len(details))
	for k, v := range details {
		if IsSensitiveKey(k) || looksLikeCredential(v) {
			v = Redacted
		}
		out[k] = v
	}
	return out
}

// IsSensitiveKey reports whether a detail key names secret material. Keys
// are split into words on non-alphanumerics and camelCase boundaries
// ("newPassword", "access_token", "X-Api-Key"). A key is sensitive when any
// word contains "password", "passwd", "secret", "authorization",
// "credential" or "cookie", or when its last word is (or ends with) "token",
// or is "pwd"/"apikey"/"bearer", or is "key" preceded by "api", "private",
// "secret", "access", "signing" or "encryption". Identifiers such as
// "token_id" or "token_name" are not sensitive.
func IsSensitiveKey(key string) bool {
	words := splitWords(key)
	if len(words) == 0 {
		return false
	}
	for _, w := range words {
		for _, s := range [...]string{"password", "passwd", "secret", "authorization", "credential", "cookie"} {
			if strings.Contains(w, s) {
				return true
			}
		}
	}
	last := words[len(words)-1]
	switch {
	case strings.HasSuffix(last, "token"), last == "pwd", last == "apikey", last == "bearer":
		return true
	case last == "key" && len(words) >= 2:
		switch words[len(words)-2] {
		case "api", "private", "secret", "access", "signing", "encryption":
			return true
		}
	}
	return false
}

func looksLikeCredential(v string) bool {
	v = strings.TrimSpace(v)
	return len(v) > len("bearer ") && strings.EqualFold(v[:len("bearer ")], "bearer ")
}

// splitWords lower-cases key and splits it into words.
func splitWords(key string) []string {
	var words []string
	var cur []rune
	flush := func() {
		if len(cur) > 0 {
			words = append(words, strings.ToLower(string(cur)))
			cur = cur[:0]
		}
	}
	rs := []rune(key)
	for i, r := range rs {
		switch {
		case !unicode.IsLetter(r) && !unicode.IsDigit(r):
			flush()
			continue
		case unicode.IsUpper(r) && len(cur) > 0:
			prev := rs[i-1]
			nextLower := i+1 < len(rs) && unicode.IsLower(rs[i+1])
			// "newPassword" -> new|Password; "APIKey" -> API|Key.
			if unicode.IsLower(prev) || unicode.IsDigit(prev) || (unicode.IsUpper(prev) && nextLower) {
				flush()
			}
		}
		cur = append(cur, r)
	}
	flush()
	return words
}
