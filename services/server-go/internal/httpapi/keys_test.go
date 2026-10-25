package httpapi

import (
	"crypto/ed25519"
	"net/http"
	"testing"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/signing"
)

func withSigning(d *Deps) {
	d.Signing = signing.New(signing.Deps{Store: meta.NewMemory()})
}

func testPublicKey(seed byte) []byte {
	s := make([]byte, ed25519.SeedSize)
	for i := range s {
		s[i] = seed
	}
	return ed25519.NewKeyFromSeed(s).Public().(ed25519.PublicKey)
}

func TestSigningKeyEndpoints(t *testing.T) {
	t.Parallel()
	f := newFixture(t, withSigning)
	pub := testPublicKey(1)

	// Create.
	rec := f.do(t, "POST", "/api/v1/auth/keys", "alice", map[string]any{"name": "laptop", "public_key": pub})
	if rec.Code != http.StatusCreated {
		t.Fatalf("create: %d %s", rec.Code, rec.Body.String())
	}
	key := decode[domain.SigningKey](t, rec)
	if key.Name != "laptop" || key.AccountID != "acct-alice" || key.Fingerprint != signing.Fingerprint(pub) || string(key.PublicKey) != string(pub) {
		t.Fatalf("key = %+v", key)
	}
	hasAudit(t, f, "signing_key.create")

	// Duplicate, bad input, anonymous.
	for _, tc := range []struct {
		as     string
		body   any
		status int
		code   string
	}{
		{"bob", map[string]any{"name": "copy", "public_key": pub}, http.StatusConflict, "conflict"},
		{"alice", map[string]any{"name": "short", "public_key": []byte{1, 2}}, http.StatusBadRequest, "bad_request"},
		{"alice", map[string]any{"name": "", "public_key": testPublicKey(2)}, http.StatusBadRequest, "bad_request"},
		{"alice", map[string]any{"name": "x", "algorithm": "rsa", "public_key": testPublicKey(2)}, http.StatusBadRequest, "bad_request"},
		{"alice", `{"name":"x","public_key":"not base64!"}`, http.StatusBadRequest, ""},
		{"", map[string]any{"name": "x", "public_key": testPublicKey(2)}, http.StatusUnauthorized, "unauthenticated"},
	} {
		rec := f.do(t, "POST", "/api/v1/auth/keys", tc.as, tc.body)
		if rec.Code != tc.status || (tc.code != "" && errCode(rec) != tc.code) {
			t.Fatalf("%v: %d %s", tc.body, rec.Code, rec.Body.String())
		}
	}

	// List own keys; bob has none.
	rec = f.do(t, "GET", "/api/v1/auth/keys", "alice", nil)
	if keys := decode[[]domain.SigningKey](t, rec); rec.Code != http.StatusOK || len(keys) != 1 || keys[0].ID != key.ID {
		t.Fatalf("list: %d %s", rec.Code, rec.Body.String())
	}
	rec = f.do(t, "GET", "/api/v1/auth/keys", "bob", nil)
	if keys := decode[[]domain.SigningKey](t, rec); len(keys) != 0 {
		t.Fatalf("bob keys: %s", rec.Body.String())
	}

	// Public keys of an account, for any authenticated caller.
	rec = f.do(t, "GET", "/api/v1/accounts/alice/keys", "carol", nil)
	if keys := decode[[]domain.SigningKey](t, rec); rec.Code != http.StatusOK || len(keys) != 1 {
		t.Fatalf("account keys: %d %s", rec.Code, rec.Body.String())
	}
	if rec := f.do(t, "GET", "/api/v1/accounts/alice/keys", "", nil); rec.Code != http.StatusUnauthorized {
		t.Fatalf("anonymous account keys: %d", rec.Code)
	}
	if rec := f.do(t, "GET", "/api/v1/accounts/nobody/keys", "carol", nil); rec.Code != http.StatusNotFound {
		t.Fatalf("unknown account: %d", rec.Code)
	}

	// Revoke: only the owner; idempotent.
	if rec := f.do(t, "DELETE", "/api/v1/auth/keys/"+key.ID, "bob", nil); rec.Code != http.StatusNotFound {
		t.Fatalf("foreign revoke: %d %s", rec.Code, rec.Body.String())
	}
	for range 2 {
		if rec := f.do(t, "DELETE", "/api/v1/auth/keys/"+key.ID, "alice", nil); rec.Code != http.StatusNoContent {
			t.Fatalf("revoke: %d %s", rec.Code, rec.Body.String())
		}
	}
	hasAudit(t, f, "signing_key.revoke")
	rec = f.do(t, "GET", "/api/v1/auth/keys", "alice", nil)
	if keys := decode[[]domain.SigningKey](t, rec); len(keys) != 1 || keys[0].RevokedAt == nil {
		t.Fatalf("after revoke: %s", rec.Body.String())
	}
}

func TestSigningKeyEndpointsNotImplemented(t *testing.T) {
	t.Parallel()
	f := newFixture(t)
	for _, tc := range []struct{ method, path string }{
		{"GET", "/api/v1/auth/keys"},
		{"POST", "/api/v1/auth/keys"},
		{"DELETE", "/api/v1/auth/keys/k1"},
		{"GET", "/api/v1/accounts/alice/keys"},
	} {
		var body any
		if tc.method == "POST" {
			body = map[string]any{"name": "x", "public_key": testPublicKey(1)}
		}
		rec := f.do(t, tc.method, tc.path, "alice", body)
		if rec.Code != http.StatusNotImplemented || errCode(rec) != "not_implemented" {
			t.Fatalf("%s %s: %d %s", tc.method, tc.path, rec.Code, rec.Body.String())
		}
	}
}
