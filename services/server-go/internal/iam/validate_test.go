package iam

import (
	"errors"
	"strings"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

func TestValidateUsername(t *testing.T) {
	tests := []struct {
		in         string
		ok         bool
		okForAdmin bool
	}{
		{"alice", true, true},
		{"a1", true, true},
		{"0day", true, true},
		{"my-name", true, true},
		{"trailing-", true, true},
		{strings.Repeat("a", 39), true, true},
		{strings.Repeat("a", 40), false, false},
		{"a", false, false},
		{"", false, false},
		{"-lead", false, false},
		{"Alice", false, false}, // validation expects normalized input
		{"al_ice", false, false},
		{"al.ice", false, false},
		{"al ice", false, false},
		{"ålice", false, false},
		{"admin", false, true},
		{"api", false, false},
		{"healthz", false, false},
		{"metrics", false, false},
		{"events", false, false},
		{"system", false, false},
		{"anonymous", false, false},
		{"admins", true, true},
	}
	for _, tt := range tests {
		t.Run(tt.in, func(t *testing.T) {
			err := ValidateUsername(tt.in)
			if (err == nil) != tt.ok {
				t.Errorf("ValidateUsername(%q) = %v, want ok=%v", tt.in, err, tt.ok)
			}
			if err != nil && !errors.Is(err, domain.ErrInvalid) {
				t.Errorf("error %v does not wrap ErrInvalid", err)
			}
			if err := validateUsername(tt.in, true); (err == nil) != tt.okForAdmin {
				t.Errorf("validateUsername(%q, admin) = %v, want ok=%v", tt.in, err, tt.okForAdmin)
			}
		})
	}
}

func TestNormalizeUsername(t *testing.T) {
	tests := map[string]string{
		"Alice":     "alice",
		"  BOB  ":   "bob",
		"mIxEd-123": "mixed-123",
		"Ålice":     "Ålice", // non-ASCII untouched
		"Kelvin":    "Kelvin",
	}
	for in, want := range tests {
		if got := NormalizeUsername(in); got != want {
			t.Errorf("NormalizeUsername(%q) = %q, want %q", in, got, want)
		}
	}
}

func TestValidateEmail(t *testing.T) {
	tests := []struct {
		in string
		ok bool
	}{
		{"a@example.com", true},
		{"first.last+tag@sub.example.org", true},
		{"admin@localhost", true},
		{"", false},
		{"plain", false},
		{"@example.com", false},
		{"a@", false},
		{"a@@example.com", false},
		{"a@b@c", false},
		{"Alice <a@example.com>", false},
		{"a @example.com", false},
		{"a@example.com ", false},
		{"a@.example.com", false},
		{"a@example.com.", false},
		{"a@exa..mple.com", false},
		{strings.Repeat("a", 250) + "@b.co", false},
	}
	for _, tt := range tests {
		t.Run(tt.in, func(t *testing.T) {
			err := ValidateEmail(tt.in)
			if (err == nil) != tt.ok {
				t.Errorf("ValidateEmail(%q) = %v, want ok=%v", tt.in, err, tt.ok)
			}
			if err != nil && !errors.Is(err, domain.ErrInvalid) {
				t.Errorf("error %v does not wrap ErrInvalid", err)
			}
		})
	}
}

func TestValidatePassword(t *testing.T) {
	tests := []struct {
		name string
		in   string
		ok   bool
	}{
		{"empty", "", false},
		{"7 chars", "1234567", false},
		{"8 chars", "12345678", true},
		{"8 multibyte chars", "ääääääää", true},
		{"7 multibyte chars", "äääääää", false},
		{"max bytes", strings.Repeat("x", MaxPasswordLen), true},
		{"over max bytes", strings.Repeat("x", MaxPasswordLen+1), false},
		{"multibyte over max bytes", strings.Repeat("ä", MaxPasswordLen/2+1), false},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			err := ValidatePassword(tt.in)
			if (err == nil) != tt.ok {
				t.Errorf("ValidatePassword = %v, want ok=%v", err, tt.ok)
			}
			if err != nil && !errors.Is(err, domain.ErrInvalid) {
				t.Errorf("error %v does not wrap ErrInvalid", err)
			}
		})
	}
}

func TestValidateTokenName(t *testing.T) {
	for _, tt := range []struct {
		in string
		ok bool
	}{
		{"ci", true},
		{"laptop token", true},
		{"", false},
		{"bad\nname", false},
		{strings.Repeat("n", maxTokenName), true},
		{strings.Repeat("n", maxTokenName+1), false},
		{"\xff", false},
	} {
		if err := validateTokenName(tt.in); (err == nil) != tt.ok {
			t.Errorf("validateTokenName(%q) = %v, want ok=%v", tt.in, err, tt.ok)
		}
	}
}

func TestWellFormedToken(t *testing.T) {
	raw, h, err := newToken()
	if err != nil {
		t.Fatal(err)
	}
	if !wellFormedToken(raw) {
		t.Fatalf("fresh token %q not well formed", raw)
	}
	if h != HashToken(raw) || len(h) != 64 {
		t.Fatalf("bad hash %q", h)
	}
	for _, bad := range []string{"", "wt_", "xx_" + raw[3:], raw + "A", raw[:len(raw)-1], raw[:10] + "=" + raw[11:], raw[:10] + "+" + raw[11:]} {
		if wellFormedToken(bad) {
			t.Errorf("wellFormedToken(%q) = true", bad)
		}
	}
}
