package auth

import (
	"net/http"
	"testing"
)

func TestStaticAuthenticatorRequiresConfiguredToken(t *testing.T) {
	authenticator := NewStaticAuthenticator("secret")
	req, err := http.NewRequest(http.MethodGet, "/", nil)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := authenticator.Authenticate(req); err == nil {
		t.Fatal("expected unauthorized error")
	}
	req.Header.Set("Authorization", "Bearer secret")
	principal, err := authenticator.Authenticate(req)
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
	principal, err := authenticator.Authenticate(req)
	if err != nil {
		t.Fatalf("Authenticate() error = %v", err)
	}
	if principal.Tenant != "acme" || principal.Account != "alice" {
		t.Fatalf("principal = %#v", principal)
	}
}
