package iam

import (
	"strings"
	"testing"
	"time"
)

// RFC 6238 Appendix B (SHA-1, 8 digits, secret "12345678901234567890").
func TestTOTPRFC6238Vectors(t *testing.T) {
	key := []byte("12345678901234567890")
	vectors := []struct {
		unix int64
		want string
	}{
		{59, "94287082"},
		{1111111109, "07081804"},
		{1111111111, "14050471"},
		{1234567890, "89005924"},
		{2000000000, "69279037"},
		{20000000000, "65353130"},
	}
	for _, v := range vectors {
		at := time.Unix(v.unix, 0).UTC()
		if got := hotp(key, uint64(totpStep(at)), 8); got != v.want {
			t.Errorf("T=%d: got %s, want %s", v.unix, got, v.want)
		}
		// The 6-digit code is the same truncation modulo 10^6.
		if got := TOTPCode(key, at); got != v.want[2:] {
			t.Errorf("T=%d: 6-digit got %s, want %s", v.unix, got, v.want[2:])
		}
	}
}

// RFC 4226 Appendix D.
func TestHOTPRFC4226Vectors(t *testing.T) {
	key := []byte("12345678901234567890")
	want := []string{"755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871", "520489"}
	for i, w := range want {
		if got := hotp(key, uint64(i), 6); got != w {
			t.Errorf("counter %d: got %s, want %s", i, got, w)
		}
	}
}

func TestVerifyTOTP(t *testing.T) {
	key := []byte("12345678901234567890")
	now := time.Unix(1111111111, 0)
	cur := totpStep(now)
	at := func(d int64) string { return hotp(key, uint64(cur+d), 6) }

	tests := []struct {
		name     string
		code     string
		last     int64
		ok       bool
		wantStep int64
	}{
		{"current", at(0), 0, true, cur},
		{"previous step", at(-1), 0, true, cur - 1},
		{"next step", at(1), 0, true, cur + 1},
		{"two steps back", at(-2), 0, false, 0},
		{"two steps ahead", at(2), 0, false, 0},
		{"spaces", at(0)[:3] + " " + at(0)[3:], 0, true, cur},
		{"replay of current step", at(0), cur, false, 0},
		{"older step than last used", at(-1), cur - 1, false, 0},
		{"newer than last used", at(1), cur, true, cur + 1},
		{"too short", at(0)[:5], 0, false, 0},
		{"too long", at(0) + "1", 0, false, 0},
		{"letters", "12a456", 0, false, 0},
		{"empty", "", 0, false, 0},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			step, ok := verifyTOTP(key, tt.code, now, tt.last)
			if ok != tt.ok || (ok && step != tt.wantStep) {
				t.Fatalf("verifyTOTP(%q) = %d, %v; want %d, %v", tt.code, step, ok, tt.wantStep, tt.ok)
			}
		})
	}
	if _, ok := verifyTOTP(nil, at(0), now, 0); ok {
		t.Fatal("empty secret verified")
	}
}

func TestTOTPSecretAndURL(t *testing.T) {
	s1, err := newTOTPSecret()
	if err != nil {
		t.Fatal(err)
	}
	s2, _ := newTOTPSecret()
	if s1 == s2 {
		t.Fatal("secrets repeat")
	}
	if len(s1) != 32 || strings.ContainsAny(s1, "=") {
		t.Fatalf("secret %q: want 32 base32 chars without padding", s1)
	}
	raw, err := decodeTOTPSecret(strings.ToLower(s1))
	if err != nil || len(raw) != TOTPSecretBytes {
		t.Fatalf("decode = %d bytes, %v", len(raw), err)
	}
	if _, err := decodeTOTPSecret("not base32!"); err == nil {
		t.Fatal("malformed secret decoded")
	}
	want := "otpauth://totp/W0rkTree:alice?secret=" + s1 + "&issuer=W0rkTree"
	if got := totpURL("alice", s1); got != want {
		t.Fatalf("totpURL = %q, want %q", got, want)
	}
}
