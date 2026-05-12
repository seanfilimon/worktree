package auth

import (
	"context"
	"errors"
	"net/http"
	"time"

	"github.com/golang-jwt/jwt/v5"
)

type JWTAuthenticator struct {
	secret []byte
}

func NewJWTAuthenticator(secret string) JWTAuthenticator {
	return JWTAuthenticator{secret: []byte(secret)}
}

type claims struct {
	Tenant  string   `json:"tenant,omitempty"`
	Account string   `json:"account,omitempty"`
	TokenID string   `json:"token_id,omitempty"`
	Scopes  []string `json:"scopes,omitempty"`
	jwt.RegisteredClaims
}

func (a JWTAuthenticator) AuthenticateHTTP(r *http.Request) (Principal, error) {
	token := bearerTokenFromHeader(r.Header.Get("Authorization"))
	return a.AuthenticateBearer(r.Context(), token)
}

func (a JWTAuthenticator) AuthenticateBearer(ctx context.Context, tokenStr string) (Principal, error) {
	if err := ctx.Err(); err != nil {
		return Principal{}, err
	}
	if tokenStr == "" {
		return Principal{}, ErrUnauthorized
	}

	token, err := jwt.ParseWithClaims(tokenStr, &claims{}, func(token *jwt.Token) (interface{}, error) {
		if _, ok := token.Method.(*jwt.SigningMethodHMAC); !ok {
			return nil, errors.New("unexpected signing method")
		}
		return a.secret, nil
	})

	if err != nil || !token.Valid {
		return Principal{}, ErrUnauthorized
	}

	c, ok := token.Claims.(*claims)
	if !ok {
		return Principal{}, ErrUnauthorized
	}

	return Principal{
		Tenant:        c.Tenant,
		Account:       c.Account,
		Subject:       c.Subject,
		TokenID:       c.TokenID,
		AuthMethod:    "jwt",
		Scopes:        c.Scopes,
		Authenticated: true,
	}, nil
}

func (a JWTAuthenticator) GenerateToken(principal Principal) (string, error) {
	subject := principal.Subject
	if subject == "" {
		subject = principal.Account
	}
	c := claims{
		Tenant:  principal.Tenant,
		Account: principal.Account,
		TokenID: principal.TokenID,
		Scopes:  principal.Scopes,
		RegisteredClaims: jwt.RegisteredClaims{
			Subject:   subject,
			ExpiresAt: jwt.NewNumericDate(time.Now().Add(7 * 24 * time.Hour)),
			IssuedAt:  jwt.NewNumericDate(time.Now()),
		},
	}
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, c)
	return token.SignedString(a.secret)
}
