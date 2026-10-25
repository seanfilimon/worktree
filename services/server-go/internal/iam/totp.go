package iam

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha1"
	"crypto/subtle"
	"encoding/base32"
	"encoding/binary"
	"fmt"
	"net/url"
	"strings"
	"time"
)

// TOTP parameters (RFC 6238 defaults, as understood by every authenticator
// app): HMAC-SHA-1, 6 digits, 30 second steps, 160-bit secrets.
const (
	TOTPDigits      = 6
	TOTPPeriod      = 30 * time.Second
	TOTPSecretBytes = 20
	TOTPIssuer      = "W0rkTree"
	// totpSkew is the number of steps accepted before and after the
	// current one, to tolerate clock drift between server and device.
	totpSkew = 1
)

// totpEncoding is RFC 4648 base32 without padding, the form used in
// otpauth:// URIs.
var totpEncoding = base32.StdEncoding.WithPadding(base32.NoPadding)

var digitsPow = [...]uint32{1, 10, 100, 1000, 10000, 100000, 1000000, 10000000, 100000000, 1000000000}

// hotp computes the RFC 4226 HOTP value of key at counter with the given
// number of decimal digits (1..9), zero-padded.
func hotp(key []byte, counter uint64, digits int) string {
	var msg [8]byte
	binary.BigEndian.PutUint64(msg[:], counter)
	m := hmac.New(sha1.New, key)
	m.Write(msg[:])
	sum := m.Sum(nil)
	// Dynamic truncation (RFC 4226 §5.3).
	off := sum[len(sum)-1] & 0x0f
	bin := binary.BigEndian.Uint32(sum[off:off+4]) & 0x7fffffff
	return fmt.Sprintf("%0*d", digits, bin%digitsPow[digits])
}

// totpStep returns the RFC 6238 time step (T) containing t.
func totpStep(t time.Time) int64 {
	return t.Unix() / int64(TOTPPeriod/time.Second)
}

// TOTPCode returns the 6-digit TOTP code for the raw secret at time t.
func TOTPCode(secret []byte, t time.Time) string {
	return hotp(secret, uint64(totpStep(t)), TOTPDigits)
}

// newTOTPSecret returns a fresh random secret, base32-encoded.
func newTOTPSecret() (string, error) {
	b := make([]byte, TOTPSecretBytes)
	if _, err := rand.Read(b); err != nil {
		return "", fmt.Errorf("iam: generating totp secret: %w", err)
	}
	return totpEncoding.EncodeToString(b), nil
}

// decodeTOTPSecret decodes a base32 secret (case-insensitive, optional
// padding and spaces).
func decodeTOTPSecret(s string) ([]byte, error) {
	s = strings.ToUpper(strings.ReplaceAll(s, " ", ""))
	s = strings.TrimRight(s, "=")
	b, err := totpEncoding.DecodeString(s)
	if err != nil || len(b) == 0 {
		return nil, fmt.Errorf("iam: malformed totp secret")
	}
	return b, nil
}

// totpURL builds the otpauth:// provisioning URI shown as a QR code.
func totpURL(username, secret string) string {
	return "otpauth://totp/" + TOTPIssuer + ":" + url.PathEscape(username) +
		"?secret=" + secret + "&issuer=" + TOTPIssuer
}

// normalizeTOTPCode strips spaces and reports whether the result is a
// TOTPDigits-digit decimal string.
func normalizeTOTPCode(code string) (string, bool) {
	code = strings.Map(func(r rune) rune {
		if r == ' ' || r == '\t' {
			return -1
		}
		return r
	}, code)
	if len(code) != TOTPDigits {
		return "", false
	}
	for i := 0; i < len(code); i++ {
		if code[i] < '0' || code[i] > '9' {
			return "", false
		}
	}
	return code, true
}

// verifyTOTP checks code against secret at now, accepting totpSkew steps of
// drift either way. Steps at or before lastUsedStep are rejected (replay
// protection). It returns the matched step. Every candidate step is
// compared in constant time and all of them are evaluated, so the timing
// does not reveal which one matched.
func verifyTOTP(secret []byte, code string, now time.Time, lastUsedStep int64) (int64, bool) {
	code, ok := normalizeTOTPCode(code)
	if !ok || len(secret) == 0 {
		return 0, false
	}
	cur := totpStep(now)
	var matched int64
	found := 0
	for d := int64(-totpSkew); d <= totpSkew; d++ {
		step := cur + d
		if step < 0 {
			continue
		}
		want := hotp(secret, uint64(step), TOTPDigits)
		eq := subtle.ConstantTimeCompare([]byte(want), []byte(code))
		fresh := 0
		if step > lastUsedStep {
			fresh = 1
		}
		if eq&fresh == 1 && found == 0 {
			matched, found = step, 1
		}
	}
	return matched, found == 1
}
