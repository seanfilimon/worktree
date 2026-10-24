package iam

import (
	"errors"
	"strings"
	"testing"
)

func TestHashPasswordRoundTrip(t *testing.T) {
	tests := []struct {
		name     string
		password string
	}{
		{"ascii", "hunter2hunter2"},
		{"unicode", "pässwörd-日本語"},
		{"max length", strings.Repeat("x", MaxPasswordLen)},
		{"empty", ""},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			h, err := HashPassword(tt.password, fastParams)
			if err != nil {
				t.Fatal(err)
			}
			if !strings.HasPrefix(h, "$argon2id$v=19$m=64,t=1,p=1$") {
				t.Fatalf("unexpected PHC prefix: %s", h)
			}
			ok, err := VerifyPassword(tt.password, h)
			if err != nil || !ok {
				t.Fatalf("VerifyPassword(correct) = %v, %v", ok, err)
			}
			ok, err = VerifyPassword(tt.password+"!", h)
			if err != nil || ok {
				t.Fatalf("VerifyPassword(wrong) = %v, %v", ok, err)
			}
		})
	}
}

func TestHashPasswordUniqueSalt(t *testing.T) {
	a, err := HashPassword("same password", fastParams)
	if err != nil {
		t.Fatal(err)
	}
	b, err := HashPassword("same password", fastParams)
	if err != nil {
		t.Fatal(err)
	}
	if a == b {
		t.Fatal("two hashes of the same password are identical; salt not random")
	}
}

func TestHashPasswordDefaults(t *testing.T) {
	h, err := HashPassword("default-params", PasswordParams{})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.HasPrefix(h, "$argon2id$v=19$m=65536,t=3,p=4$") {
		t.Fatalf("defaults not applied: %s", h)
	}
	if ok, err := VerifyPassword("default-params", h); err != nil || !ok {
		t.Fatalf("verify: %v %v", ok, err)
	}
}

func TestVerifyPasswordMalformed(t *testing.T) {
	good, err := HashPassword("pw", fastParams)
	if err != nil {
		t.Fatal(err)
	}
	parts := strings.Split(good, "$")
	salt, key := parts[4], parts[5]
	tests := []struct {
		name string
		hash string
	}{
		{"empty", ""},
		{"bcrypt", "$2a$10$abcdefghijklmnopqrstuuABCDEFGHIJKLMNOPQRSTUVWXYZ01234"},
		{"argon2i", "$argon2i$v=19$m=64,t=1,p=1$" + salt + "$" + key},
		{"bad version", "$argon2id$v=16$m=64,t=1,p=1$" + salt + "$" + key},
		{"missing part", "$argon2id$v=19$m=64,t=1,p=1$" + salt},
		{"extra part", good + "$x"},
		{"param order", "$argon2id$v=19$t=1,m=64,p=1$" + salt + "$" + key},
		{"param not number", "$argon2id$v=19$m=x,t=1,p=1$" + salt + "$" + key},
		{"zero time", "$argon2id$v=19$m=64,t=0,p=1$" + salt + "$" + key},
		{"huge memory", "$argon2id$v=19$m=4294967295,t=1,p=1$" + salt + "$" + key},
		{"huge time", "$argon2id$v=19$m=64,t=1000,p=1$" + salt + "$" + key},
		{"zero threads", "$argon2id$v=19$m=64,t=1,p=0$" + salt + "$" + key},
		{"threads overflow", "$argon2id$v=19$m=64,t=1,p=256$" + salt + "$" + key},
		{"memory below lanes", "$argon2id$v=19$m=8,t=1,p=4$" + salt + "$" + key},
		{"bad salt b64", "$argon2id$v=19$m=64,t=1,p=1$!!!$" + key},
		{"short salt", "$argon2id$v=19$m=64,t=1,p=1$AAAA$" + key},
		{"short key", "$argon2id$v=19$m=64,t=1,p=1$" + salt + "$AAAA"},
		{"padded b64", "$argon2id$v=19$m=64,t=1,p=1$" + salt + "==$" + key},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			ok, err := VerifyPassword("pw", tt.hash)
			if ok || !errors.Is(err, ErrMalformedHash) {
				t.Fatalf("VerifyPassword = %v, %v; want false, ErrMalformedHash", ok, err)
			}
		})
	}
}

func TestHashPasswordInvalidParams(t *testing.T) {
	if _, err := HashPassword("pw", PasswordParams{SaltLen: 4}); err == nil {
		t.Fatal("expected error for too-short salt")
	}
	if _, err := HashPassword("pw", PasswordParams{Time: 1000}); err == nil {
		t.Fatal("expected error for excessive time cost")
	}
}

func TestNeedsRehash(t *testing.T) {
	want := fastParams.withDefaults()
	h, err := HashPassword("pw", fastParams)
	if err != nil {
		t.Fatal(err)
	}
	if needsRehash(h, want) {
		t.Fatal("hash with current params should not need rehash")
	}
	stronger := want
	stronger.Time = 2
	if !needsRehash(h, stronger) {
		t.Fatal("hash with outdated params should need rehash")
	}
	if !needsRehash("garbage", want) {
		t.Fatal("malformed hash should need rehash")
	}
}
