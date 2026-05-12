package auth

import (
	"net/http"
	"os"
	"path/filepath"
	"testing"
)

func TestStaticAuthenticatorRequiresConfiguredToken(t *testing.T) {
	authenticator := NewStaticAuthenticator("secret")
	req, err := http.NewRequest(http.MethodGet, "/", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := authenticator.AuthenticateHTTP(req); err == nil {
		t.Fatal("expected unauthorized error")
	}
	req.Header.Set("Authorization", "Bearer secret")
	principal, err := authenticator.AuthenticateHTTP(req)
	if err != nil {
		t.Fatalf("Authenticate() error = %v", err)
	}
	if principal.Tenant != "" {
		t.Fatalf("tenant = %q", principal.Tenant)
	}
}

func TestStaticAuthenticatorReadsPrincipalHeaders(t *testing.T) {
	authenticator := NewStaticAuthenticator("")
	req, err := http.NewRequest(http.MethodGet, "/", nil)
	if err != nil {
		t.Fatal(err)
	}
	req.Header.Set("X-WT-Tenant", "acme")
	req.Header.Set("X-WT-Account", "alice")
	principal, err := authenticator.AuthenticateHTTP(req)
	if err != nil {
		t.Fatalf("Authenticate() error = %v", err)
	}
	if principal.Tenant != "acme" || principal.Account != "alice" {
		t.Fatalf("principal = %#v", principal)
	}
}

func TestLoadTokenCredentialsFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "tokens.json")
	data := []byte(`{"tokens":[{"token_id":"demo","secret":"dev-secret","tenant":"acme","account":"alice","scopes":["staged:*"]}]}`)
	if err := os.WriteFile(path, data, 0o644); err != nil {
		t.Fatal(err)
	}
	credentials, err := LoadTokenCredentialsFile(path)
	if err != nil {
		t.Fatalf("LoadTokenCredentialsFile: %v", err)
	}
	if len(credentials) != 1 || credentials[0].TokenID != "demo" || credentials[0].Tenant != "acme" {
		t.Fatalf("credentials = %#v", credentials)
	}
}
