package iam

import (
	"context"
	"errors"
	"net"
	"net/http"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
)

type (
	ctxKey          struct{}
	remoteAddrKey   struct{}
	tokenHashCtxKey struct{}
)

// PrincipalFrom returns the principal attached to ctx, or nil.
func PrincipalFrom(ctx context.Context) *domain.Principal {
	p, _ := ctx.Value(ctxKey{}).(*domain.Principal)
	return p
}

// WithPrincipal attaches p to ctx.
func WithPrincipal(ctx context.Context, p *domain.Principal) context.Context {
	return context.WithValue(ctx, ctxKey{}, p)
}

// WithRemoteAddr records the client's network address (host or host:port)
// in ctx. Login uses it to key its rate limiter. Middleware sets it from
// http.Request.RemoteAddr; for gRPC the peer address is used directly.
func WithRemoteAddr(ctx context.Context, addr string) context.Context {
	return context.WithValue(ctx, remoteAddrKey{}, addr)
}

// RemoteAddrFrom returns the address recorded by WithRemoteAddr, or "".
func RemoteAddrFrom(ctx context.Context) string {
	a, _ := ctx.Value(remoteAddrKey{}).(string)
	return a
}

// TokenHashFrom returns the storage hash (see HashToken) of the bearer token
// that authenticated the request, or "" for anonymous requests. Handlers use
// it to find the current token, e.g. to revoke it on logout.
func TokenHashFrom(ctx context.Context) string {
	h, _ := ctx.Value(tokenHashCtxKey{}).(string)
	return h
}

// clientIP returns the caller's IP (without port) from ctx, or "".
func clientIP(ctx context.Context) string {
	addr := RemoteAddrFrom(ctx)
	if addr == "" {
		if p, ok := peer.FromContext(ctx); ok && p.Addr != nil {
			addr = p.Addr.String()
		}
	}
	if host, _, err := net.SplitHostPort(addr); err == nil {
		return host
	}
	return addr
}

var errBadAuthHeader = errors.New("malformed authorization header")

// parseBearer extracts the token from an Authorization header value.
func parseBearer(v string) (string, error) {
	scheme, tok, ok := strings.Cut(strings.TrimSpace(v), " ")
	if !ok || !strings.EqualFold(scheme, "Bearer") {
		return "", errBadAuthHeader
	}
	tok = strings.TrimSpace(tok)
	if tok == "" {
		return "", errBadAuthHeader
	}
	return tok, nil
}

// grpcAuthenticate resolves the bearer token in incoming metadata. It
// returns ctx unchanged when no credential is present.
func grpcAuthenticate(ctx context.Context, auth domain.Authenticator) (context.Context, error) {
	md, _ := metadata.FromIncomingContext(ctx)
	vals := md.Get("authorization")
	if len(vals) == 0 || (len(vals) == 1 && strings.TrimSpace(vals[0]) == "") {
		return ctx, nil
	}
	if len(vals) > 1 {
		return nil, status.Error(codes.Unauthenticated, "multiple authorization headers")
	}
	tok, err := parseBearer(vals[0])
	if err != nil {
		return nil, status.Error(codes.Unauthenticated, err.Error())
	}
	p, err := auth.Authenticate(ctx, tok)
	if err != nil {
		if errors.Is(err, domain.ErrUnauthenticated) {
			return nil, status.Error(codes.Unauthenticated, "invalid or expired token")
		}
		return nil, status.Error(codes.Internal, "authentication failed")
	}
	ctx = context.WithValue(ctx, tokenHashCtxKey{}, HashToken(tok))
	return WithPrincipal(ctx, p), nil
}

// UnaryInterceptor attaches the bearer-token principal (if any) to the
// context. Calls without credentials proceed anonymously; calls with an
// invalid credential fail with codes.Unauthenticated.
func UnaryInterceptor(auth domain.Authenticator) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		ctx, err := grpcAuthenticate(ctx, auth)
		if err != nil {
			return nil, err
		}
		return h(ctx, req)
	}
}

// StreamInterceptor is the streaming counterpart of UnaryInterceptor.
func StreamInterceptor(auth domain.Authenticator) grpc.StreamServerInterceptor {
	return func(srv any, ss grpc.ServerStream, info *grpc.StreamServerInfo, h grpc.StreamHandler) error {
		ctx, err := grpcAuthenticate(ss.Context(), auth)
		if err != nil {
			return err
		}
		return h(srv, &wrappedStream{ServerStream: ss, ctx: ctx})
	}
}

type wrappedStream struct {
	grpc.ServerStream
	ctx context.Context
}

func (w *wrappedStream) Context() context.Context { return w.ctx }

const unauthenticatedBody = `{"error":{"code":"unauthenticated","message":"invalid or expired token"}}` + "\n"
const internalBody = `{"error":{"code":"internal","message":"authentication failed"}}` + "\n"

// Middleware attaches the bearer-token principal (if any) to the request
// context, along with the client address (WithRemoteAddr). The token is
// read from the Authorization header or, for GET requests without one (SSE
// EventSource cannot set headers), from the access_token query parameter,
// which is then removed from the URL seen by downstream handlers. Requests
// without credentials proceed anonymously; requests with an invalid
// credential get 401 with a JSON error body.
func Middleware(auth domain.Authenticator) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			ctx := WithRemoteAddr(r.Context(), r.RemoteAddr)

			var raw string
			present := false
			hdrs := r.Header.Values("Authorization")
			if len(hdrs) == 1 && strings.TrimSpace(hdrs[0]) == "" {
				hdrs = nil // an empty header is treated as absent
			}
			if len(hdrs) > 0 {
				present = true
				if len(hdrs) == 1 {
					raw, _ = parseBearer(hdrs[0])
				}
			} else if r.Method == http.MethodGet && r.URL.Query().Has("access_token") {
				present = true
				q := r.URL.Query()
				raw = strings.TrimSpace(q.Get("access_token"))
				q.Del("access_token")
				r = r.Clone(ctx)
				r.URL.RawQuery = q.Encode()
				r.RequestURI = r.URL.RequestURI()
			}
			if !present {
				next.ServeHTTP(w, r.WithContext(ctx))
				return
			}
			if raw == "" {
				writeAuthError(w, http.StatusUnauthorized, unauthenticatedBody)
				return
			}
			p, err := auth.Authenticate(ctx, raw)
			if err != nil {
				if errors.Is(err, domain.ErrUnauthenticated) {
					writeAuthError(w, http.StatusUnauthorized, unauthenticatedBody)
				} else {
					writeAuthError(w, http.StatusInternalServerError, internalBody)
				}
				return
			}
			ctx = context.WithValue(ctx, tokenHashCtxKey{}, HashToken(raw))
			next.ServeHTTP(w, r.WithContext(WithPrincipal(ctx, p)))
		})
	}
}

func writeAuthError(w http.ResponseWriter, code int, body string) {
	h := w.Header()
	h.Set("Content-Type", "application/json; charset=utf-8")
	h.Set("Cache-Control", "no-store")
	if code == http.StatusUnauthorized {
		h.Set("WWW-Authenticate", `Bearer error="invalid_token"`)
	}
	w.WriteHeader(code)
	_, _ = w.Write([]byte(body))
}
