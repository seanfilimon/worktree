package iam

import (
	"fmt"
	"net/mail"
	"regexp"
	"strings"
	"unicode"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Password length limits. The maximum bounds hashing work per request.
const (
	MinPasswordLen = 8    // characters
	MaxPasswordLen = 1024 // bytes
	maxEmailLen    = 254  // RFC 5321 path limit
	maxTokenName   = 100  // characters
)

var usernameRE = regexp.MustCompile(`^[a-z0-9][a-z0-9-]{1,38}$`)

// reservedUsernames cannot be registered. They collide with URL segments,
// well-known endpoints or audit actor pseudonyms. "admin" is claimable only
// via EnsureAdmin.
var reservedUsernames = map[string]bool{
	"admin":     true,
	"anonymous": true,
	"api":       true,
	"auth":      true,
	"events":    true,
	"healthz":   true,
	"metrics":   true,
	"system":    true,
}

// NormalizeUsername trims surrounding whitespace and lower-cases ASCII
// letters. Non-ASCII input is left as is (and then fails validation).
func NormalizeUsername(s string) string {
	s = strings.TrimSpace(s)
	b := []byte(s)
	for i, c := range b {
		if 'A' <= c && c <= 'Z' {
			b[i] = c + ('a' - 'A')
		}
	}
	return string(b)
}

// ValidateUsername checks a normalized username against the username
// syntax and the reserved-name list. Errors wrap domain.ErrInvalid.
func ValidateUsername(username string) error {
	return validateUsername(username, false)
}

func validateUsername(username string, allowAdmin bool) error {
	if !usernameRE.MatchString(username) {
		return fmt.Errorf("username must be 2-39 characters of a-z, 0-9 or '-' and start with a letter or digit: %w", domain.ErrInvalid)
	}
	if reservedUsernames[username] && !(allowAdmin && username == "admin") {
		return fmt.Errorf("username %q is reserved: %w", username, domain.ErrInvalid)
	}
	return nil
}

// ValidateEmail performs basic syntactic validation of a bare address
// ("local@domain", no display name). Errors wrap domain.ErrInvalid.
func ValidateEmail(email string) error {
	invalid := fmt.Errorf("invalid email address: %w", domain.ErrInvalid)
	if email == "" || len(email) > maxEmailLen || strings.ContainsFunc(email, unicode.IsSpace) {
		return invalid
	}
	addr, err := mail.ParseAddress(email)
	if err != nil || addr.Name != "" || addr.Address != email {
		return invalid
	}
	local, dom, ok := strings.Cut(email, "@")
	if !ok || local == "" || dom == "" || strings.Contains(dom, "@") ||
		strings.HasPrefix(dom, ".") || strings.HasSuffix(dom, ".") || strings.Contains(dom, "..") {
		return invalid
	}
	return nil
}

// ValidatePassword enforces the password length policy. Errors wrap
// domain.ErrInvalid.
func ValidatePassword(password string) error {
	if len(password) > MaxPasswordLen {
		return fmt.Errorf("password must be at most %d bytes: %w", MaxPasswordLen, domain.ErrInvalid)
	}
	if utf8.RuneCountInString(password) < MinPasswordLen {
		return fmt.Errorf("password must be at least %d characters: %w", MinPasswordLen, domain.ErrInvalid)
	}
	return nil
}

func validateTokenName(name string) error {
	if name == "" || utf8.RuneCountInString(name) > maxTokenName || !utf8.ValidString(name) ||
		strings.ContainsFunc(name, unicode.IsControl) {
		return fmt.Errorf("token name must be 1-%d printable characters: %w", maxTokenName, domain.ErrInvalid)
	}
	return nil
}
