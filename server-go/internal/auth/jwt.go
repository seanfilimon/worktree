package auth

import (
	"context"
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"errors"
	"io"
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

	if decrypted, err := decryptToken(tokenStr, a.secret); err == nil {
		tokenStr = decrypted
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
	signed, err := token.SignedString(a.secret)
	if err != nil {
		return "", err
	}
	return encryptToken(signed, a.secret)
}

func encryptToken(plain string, secret []byte) (string, error) {
	key := sha256.Sum256(secret)
	block, err := aes.NewCipher(key[:])
	if err != nil {
		return "", err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", err
	}
	nonce := make([]byte, gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return "", err
	}
	ciphertext := gcm.Seal(nonce, nonce, []byte(plain), nil)
	return base64.URLEncoding.EncodeToString(ciphertext), nil
}

func decryptToken(encrypted string, secret []byte) (string, error) {
	data, err := base64.URLEncoding.DecodeString(encrypted)
	if err != nil {
		return "", err
	}
	key := sha256.Sum256(secret)
	block, err := aes.NewCipher(key[:])
	if err != nil {
		return "", err
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", err
	}
	nonceSize := gcm.NonceSize()
	if len(data) < nonceSize {
		return "", errors.New("ciphertext too short")
	}
	nonce, ciphertext := data[:nonceSize], data[nonceSize:]
	plain, err := gcm.Open(nil, nonce, ciphertext, nil)
	if err != nil {
		return "", err
	}
	return string(plain), nil
}
