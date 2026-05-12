package auth

import (
	"context"
	"crypto/sha256"
	"crypto/subtle"
	"errors"
	"net/http"
	"strings"
)

type Principal struct {
	Tenant        string
	Account       string
	Subject       string
	TokenID       string
	AuthMethod    string
	Scopes        []string
	Authenticated bool
}

func (p Principal) HasScope(scope string) bool {
	for _, candidate := range p.Scopes {
		if candidate == "*" || candidate == scope {
			return true
		}
	}
	return false
}

type contextKey string

const principalKey contextKey = "principal"

var ErrUnauthorized = errors.New("unauthorized")

type Authenticator interface {
	AuthenticateHTTP(r *http.Request) (Principal, error)
	AuthenticateBearer(ctx context.Context, token string) (Principal, error)
}

type TokenCredential struct {
	TokenID string   `json:"token_id"`
	Secret  string   `json:"secret"`
	Tenant  string   `json:"tenant"`
	Account string   `json:"account"`
	Scopes  []string `json:"scopes"`
}

type BearerTokenAuthenticator struct {
	credentials []storedCredential
}

type storedCredential struct {
	TokenID    string
	SecretHash [32]byte
	Principal  Principal
}

func NewBearerTokenAuthenticator(credentials []TokenCredential) BearerTokenAuthenticator {
	stored := make([]storedCredential, 0, len(credentials))
	for _, credential := range credentials {
		principal := Principal{
			Tenant:        credential.Tenant,
			Account:       credential.Account,
			Subject:       credential.Account,
			TokenID:       credential.TokenID,
			AuthMethod:    "bearer_token",
			Scopes:        append([]string(nil), credential.Scopes...),
			Authenticated: true,
		}
		stored = append(stored, storedCredential{
			TokenID:    credential.TokenID,
			SecretHash: sha256.Sum256([]byte(credential.Secret)),
			Principal:  principal,
		})
	}
	return BearerTokenAuthenticator{credentials: stored}
}

func (a BearerTokenAuthenticator) AuthenticateHTTP(r *http.Request) (Principal, error) {
	token := bearerTokenFromHeader(r.Header.Get("Authorization"))
	return a.AuthenticateBearer(r.Context(), token)
}

func (a BearerTokenAuthenticator) AuthenticateBearer(ctx context.Context, token string) (Principal, error) {
	if err := ctx.Err(); err != nil {
		return Principal{}, err
	}
	if token == "" || len(a.credentials) == 0 {
		return Principal{}, ErrUnauthorized
	}
	gotHash := sha256.Sum256([]byte(token))
	for _, credential := range a.credentials {
		if subtle.ConstantTimeCompare(gotHash[:], credential.SecretHash[:]) == 1 {
			return credential.Principal, nil
		}
	}
	return Principal{}, ErrUnauthorized
}

// StaticAuthenticator is retained for development compatibility. It must not be
// used in production because it can derive principal identity from request
// headers.
type StaticAuthenticator struct {
	token string
}

func NewStaticAuthenticator(token string) StaticAuthenticator {
	return StaticAuthenticator{token: token}
}

func (a StaticAuthenticator) AuthenticateHTTP(r *http.Request) (Principal, error) {
	if a.token != "" {
		got := bearerTokenFromHeader(r.Header.Get("Authorization"))
		if got == "" || got != a.token {
			return Principal{}, ErrUnauthorized
		}
	}
	principal := Principal{
		Tenant:        r.Header.Get("X-WT-Tenant"),
		Account:       r.Header.Get("X-WT-Account"),
		Subject:       r.Header.Get("X-WT-Account"),
		AuthMethod:    "static_dev",
		Authenticated: true,
	}
	if principal.Tenant == "" && principal.Account == "" && a.token == "" {
		principal.Authenticated = false
	}
	return principal, nil
}

func (a StaticAuthenticator) AuthenticateBearer(ctx context.Context, token string) (Principal, error) {
	if err := ctx.Err(); err != nil {
		return Principal{}, err
	}
	if a.token == "" || token != a.token {
		return Principal{}, ErrUnauthorized
	}
	return Principal{AuthMethod: "static_dev", Authenticated: true}, nil
}

func bearerTokenFromHeader(header string) string {
	if header == "" {
		return ""
	}
	parts := strings.SplitN(header, " ", 2)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return strings.TrimSpace(parts[1])
}

func WithPrincipal(ctx context.Context, principal Principal) context.Context {
	return context.WithValue(ctx, principalKey, principal)
}

func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	principal, ok := ctx.Value(principalKey).(Principal)
	return principal, ok
}
