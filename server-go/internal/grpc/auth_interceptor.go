package grpcserver

import (
	"context"
	"strings"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"

	"github.com/ramizik/worktree/server-go/internal/auth"
)

func AuthUnaryInterceptor(authenticator auth.Authenticator) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, handler grpc.UnaryHandler) (any, error) {
		principal, err := authenticateGRPC(ctx, authenticator)
		if err != nil {
			return nil, err
		}
		return handler(auth.WithPrincipal(ctx, principal), req)
	}
}

func authenticateGRPC(ctx context.Context, authenticator auth.Authenticator) (auth.Principal, error) {
	if authenticator == nil {
		return auth.Principal{}, status.Error(codes.Unauthenticated, "authentication required")
	}
	md, ok := metadata.FromIncomingContext(ctx)
	if !ok {
		return auth.Principal{}, status.Error(codes.Unauthenticated, "authorization metadata required")
	}
	values := md.Get("authorization")
	if len(values) == 0 {
		return auth.Principal{}, status.Error(codes.Unauthenticated, "authorization metadata required")
	}
	token := bearerToken(values[0])
	if token == "" {
		return auth.Principal{}, status.Error(codes.Unauthenticated, "valid bearer token required")
	}
	principal, err := authenticator.AuthenticateBearer(ctx, token)
	if err != nil {
		return auth.Principal{}, status.Error(codes.Unauthenticated, "valid bearer token required")
	}
	return principal, nil
}

func bearerToken(value string) string {
	parts := strings.SplitN(value, " ", 2)
	if len(parts) != 2 || !strings.EqualFold(parts[0], "Bearer") {
		return ""
	}
	return strings.TrimSpace(parts[1])
}
