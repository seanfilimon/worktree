package auth

import (
	"context"
	"errors"
	"net/http"
	"strings"
)

type Principal struct {
	Tenant  string
	Account string
}

type contextKey string

const principalKey contextKey = "principal"

var ErrUnauthorized = errors.New("unauthorized")

type StaticAuthenticator struct {
	token string
}

func NewStaticAuthenticator(token string) StaticAuthenticator {
	return StaticAuthenticator{token: token}
}

func (a StaticAuthenticator) Authenticate(r *http.Request) (Principal, error) {
	if a.token != "" {
		got := strings.TrimPrefix(r.Header.Get("Authorization"), "Bearer ")
		if got == "" || got != a.token {
			return Principal{}, ErrUnauthorized
		}
	}
	return Principal{
		Tenant:  r.Header.Get("X-WT-Tenant"),
		Account: r.Header.Get("X-WT-Account"),
	}, nil
}

func WithPrincipal(ctx context.Context, principal Principal) context.Context {
	return context.WithValue(ctx, principalKey, principal)
}

func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	principal, ok := ctx.Value(principalKey).(Principal)
	return principal, ok
}
