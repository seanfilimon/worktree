package iam

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"fmt"
	"strings"
)

const (
	// TokenPrefix starts every raw API token.
	TokenPrefix = "wt_"
	tokenBytes  = 32
	secretBody  = 43 // len(base64url(32 bytes)) without padding
	rawTokenLen = len(TokenPrefix) + secretBody
)

// newToken returns a fresh raw token and its storage hash.
func newToken() (raw, hashHex string, err error) {
	return newSecret(TokenPrefix)
}

// newSecret returns prefix followed by the unpadded base64url encoding of
// 32 bytes from crypto/rand, and its hex SHA-256 (see HashToken). It is
// used for API tokens and the one-time codes (MFA tokens, invitation and
// password-reset codes).
func newSecret(prefix string) (raw, hashHex string, err error) {
	b := make([]byte, tokenBytes)
	if _, err := rand.Read(b); err != nil {
		return "", "", fmt.Errorf("iam: generating secret: %w", err)
	}
	raw = prefix + base64.RawURLEncoding.EncodeToString(b)
	return raw, HashToken(raw), nil
}

// HashToken returns the hex SHA-256 of a raw token, as stored in
// domain.APIToken.TokenHash. Callers that need the stored record for the
// current request's token (e.g. logout) can look it up with
// MetaStore.GetTokenByHash(HashToken(raw)), or use TokenHashFrom. The same
// hash is used for invitation and password-reset codes.
func HashToken(raw string) string {
	sum := sha256.Sum256([]byte(raw))
	return hex.EncodeToString(sum[:])
}

// wellFormedToken reports whether raw has the shape of a token issued by
// this package. It lets Authenticate reject garbage without a store lookup.
func wellFormedToken(raw string) bool {
	return wellFormedSecret(TokenPrefix, raw)
}

// wellFormedSecret reports whether raw has the shape produced by
// newSecret(prefix).
func wellFormedSecret(prefix, raw string) bool {
	if len(raw) != len(prefix)+secretBody || !strings.HasPrefix(raw, prefix) {
		return false
	}
	for i := len(prefix); i < len(raw); i++ {
		c := raw[i]
		if !('a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' || '0' <= c && c <= '9' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}
