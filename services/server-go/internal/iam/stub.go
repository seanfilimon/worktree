// Package iam implements authentication and authorization. STUB: replaced
// by the go-iam workstream (delete this file when implementing).
package iam

import (
	"context"
	"errors"
	"net/http"
	"time"

	"google.golang.org/grpc"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

// Options configures the IAM service.
type Options struct {
	// SessionTTL is the lifetime of tokens issued by Login (default 30 days).
	SessionTTL time.Duration
	// AllowRegistration enables self-service Register.
	AllowRegistration bool
}

// Service implements domain.Authenticator and domain.Authorizer.
type Service struct {
	domain.Authenticator
	domain.Authorizer
}

// New returns an IAM service backed by store.
func New(store domain.MetaStore, opts Options) *Service { return &Service{} }

// UnaryInterceptor attaches the bearer-token principal (if any) to the context.
func UnaryInterceptor(auth domain.Authenticator) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		return h(ctx, req)
	}
}

// StreamInterceptor is the streaming counterpart of UnaryInterceptor.
func StreamInterceptor(auth domain.Authenticator) grpc.StreamServerInterceptor {
	return func(srv any, ss grpc.ServerStream, info *grpc.StreamServerInfo, h grpc.StreamHandler) error {
		return h(srv, ss)
	}
}

// Middleware attaches the bearer-token principal (if any) to the request context.
func Middleware(auth domain.Authenticator) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler { return next }
}

type ctxKey struct{}

// PrincipalFrom returns the principal attached to ctx, or nil.
func PrincipalFrom(ctx context.Context) *domain.Principal {
	p, _ := ctx.Value(ctxKey{}).(*domain.Principal)
	return p
}

// WithPrincipal attaches p to ctx.
func WithPrincipal(ctx context.Context, p *domain.Principal) context.Context {
	return context.WithValue(ctx, ctxKey{}, p)
}

// EnsureAdmin creates (or promotes) the server admin account.
func EnsureAdmin(ctx context.Context, store domain.MetaStore, auth domain.Authenticator, username, password string) error {
	return errors.New("iam: not implemented")
}
