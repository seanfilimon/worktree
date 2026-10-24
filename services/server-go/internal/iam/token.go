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
	rawTokenLen = len(TokenPrefix) + 43 // base64url(32 bytes) without padding
)

// newToken returns a fresh raw token and its storage hash.
func newToken() (raw, hashHex string, err error) {
	b := make([]byte, tokenBytes)
	if _, err := rand.Read(b); err != nil {
		return "", "", fmt.Errorf("iam: generating token: %w", err)
	}
	raw = TokenPrefix + base64.RawURLEncoding.EncodeToString(b)
	return raw, HashToken(raw), nil
}

// HashToken returns the hex SHA-256 of a raw token, as stored in
// domain.APIToken.TokenHash. Callers that need the stored record for the
// current request's token (e.g. logout) can look it up with
// MetaStore.GetTokenByHash(HashToken(raw)), or use TokenHashFrom.
func HashToken(raw string) string {
	sum := sha256.Sum256([]byte(raw))
	return hex.EncodeToString(sum[:])
}

// wellFormedToken reports whether raw has the shape of a token issued by
// this package. It lets Authenticate reject garbage without a store lookup.
func wellFormedToken(raw string) bool {
	if len(raw) != rawTokenLen || !strings.HasPrefix(raw, TokenPrefix) {
		return false
	}
	for i := len(TokenPrefix); i < len(raw); i++ {
		c := raw[i]
		if !('a' <= c && c <= 'z' || 'A' <= c && c <= 'Z' || '0' <= c && c <= '9' || c == '-' || c == '_') {
			return false
		}
	}
	return true
}
