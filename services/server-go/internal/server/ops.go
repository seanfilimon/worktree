package server

// Operations wiring (docs/ENGINEERING_PLAN.md §13.3): TLS, gRPC rate
// limiting, sync metrics, access.denied auditing and authorization
// decision counting.

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"math"
	"net"
	"os"
	"strconv"

	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
	"github.com/seanfilimon/worktree/services/server-go/internal/authz"
	"github.com/seanfilimon/worktree/services/server-go/internal/config"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// tlsConfig builds the server TLS configuration from cfg (nil when TLS is
// off): the certificate pair, ALPN h2 + http/1.1 (gRPC needs h2) and, with
// a client CA bundle, required and verified client certificates (mTLS).
func tlsConfig(cfg config.Config) (*tls.Config, error) {
	if !cfg.TLSEnabled() {
		return nil, nil
	}
	cert, err := tls.LoadX509KeyPair(cfg.TLSCert, cfg.TLSKey)
	if err != nil {
		return nil, fmt.Errorf("load TLS certificate: %w", err)
	}
	tc := &tls.Config{
		Certificates: []tls.Certificate{cert},
		MinVersion:   tls.VersionTLS12,
		NextProtos:   []string{"h2", "http/1.1"},
	}
	if cfg.TLSClientCA != "" {
		pem, err := os.ReadFile(cfg.TLSClientCA)
		if err != nil {
			return nil, fmt.Errorf("read TLS client CA: %w", err)
		}
		pool := x509.NewCertPool()
		if !pool.AppendCertsFromPEM(pem) {
			return nil, fmt.Errorf("TLS client CA %s: no PEM certificates found", cfg.TLSClientCA)
		}
		tc.ClientCAs = pool
		tc.ClientAuth = tls.RequireAndVerifyClientCert
	}
	return tc, nil
}

// grpcOps are the §13.3 collaborators of the gRPC interceptors (each may
// be nil).
type grpcOps struct {
	limiter *limits.Limiter
	audit   domain.AuditLog
}

// countingAuthz counts allow/deny decisions (wt_authz_decisions_total) of
// the policy engine. Embedding keeps every other method (Evaluate's
// AccessEvaluator interface, EffectivePermissions, ...) available.
type countingAuthz struct {
	*authz.Authorizer
}

func (c countingAuthz) Authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	err := c.Authorizer.Authorize(ctx, p, act, tenantID, tree)
	countDecision(err)
	return err
}

func (c countingAuthz) Evaluate(ctx context.Context, p *domain.Principal, act domain.Action, res domain.Resource, env domain.RequestEnv) (domain.Decision, error) {
	d, err := c.Authorizer.Evaluate(ctx, p, act, res, env)
	if err == nil {
		if d.Allowed {
			opsmetrics.AuthzDecisions.Inc("allow")
		} else {
			opsmetrics.AuthzDecisions.Inc("deny")
		}
	}
	return d, err
}

func countDecision(err error) {
	switch {
	case err == nil:
		opsmetrics.AuthzDecisions.Inc("allow")
	case errors.Is(err, domain.ErrForbidden), errors.Is(err, domain.ErrUnauthenticated):
		opsmetrics.AuthzDecisions.Inc("deny")
	}
}

// peerIP returns the client IP of a gRPC call.
func peerIP(ctx context.Context) string {
	if p, ok := peer.FromContext(ctx); ok && p.Addr != nil {
		if host, _, err := net.SplitHostPort(p.Addr.String()); err == nil {
			return host
		}
		return p.Addr.String()
	}
	return ""
}

// rateLimited charges one call to the caller's bucket; it returns a
// ResourceExhausted status (with retry-after and x-ratelimit-* trailers'
// equivalents as headers) when the bucket is empty.
func rateLimited(ctx context.Context, l *limits.Limiter) error {
	if l == nil {
		return nil
	}
	d := l.Allow(ctx, iam.PrincipalFrom(ctx), peerIP(ctx))
	if d.Allowed || d.Limit == 0 {
		return nil
	}
	opsmetrics.RateLimited.Inc("")
	secs := int(math.Ceil(d.RetryAfter.Seconds()))
	if secs < 1 {
		secs = 1
	}
	_ = grpc.SetHeader(ctx, metadata.Pairs("retry-after", strconv.Itoa(secs),
		"x-ratelimit-limit", strconv.Itoa(d.Limit), "x-ratelimit-remaining", "0"))
	return status.Errorf(codes.ResourceExhausted, "rate limit of %d requests per minute exceeded; retry in %ds", d.Limit, secs)
}

func rateLimitUnary(l *limits.Limiter) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, _ *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		if err := rateLimited(ctx, l); err != nil {
			return nil, err
		}
		return h(ctx, req)
	}
}

func rateLimitStream(l *limits.Limiter) grpc.StreamServerInterceptor {
	return func(srv any, ss grpc.ServerStream, _ *grpc.StreamServerInfo, h grpc.StreamHandler) error {
		if err := rateLimited(ss.Context(), l); err != nil {
			return err
		}
		return h(srv, ss)
	}
}

// observeUnary counts pushes/pulls and audits PermissionDenied answers.
func observeUnary(log domain.AuditLog) grpc.UnaryServerInterceptor {
	return func(ctx context.Context, req any, info *grpc.UnaryServerInfo, h grpc.UnaryHandler) (any, error) {
		resp, err := h(ctx, req)
		switch info.FullMethod {
		case syncpb.SyncService_Push_FullMethodName:
			switch r, _ := resp.(*syncpb.PushResponse); {
			case err != nil:
				opsmetrics.SyncPushes.Inc("error")
			case r.GetAccepted():
				opsmetrics.SyncPushes.Inc("accepted")
			default:
				opsmetrics.SyncPushes.Inc("rejected")
			}
		case syncpb.SyncService_Pull_FullMethodName:
			if err == nil {
				opsmetrics.SyncPulls.Inc("")
			}
		}
		auditGRPCDenied(ctx, log, info.FullMethod, err)
		return resp, err
	}
}

func observeStream(log domain.AuditLog) grpc.StreamServerInterceptor {
	return func(srv any, ss grpc.ServerStream, info *grpc.StreamServerInfo, h grpc.StreamHandler) error {
		err := h(srv, ss)
		auditGRPCDenied(ss.Context(), log, info.FullMethod, err)
		return err
	}
}

// auditGRPCDenied records access.denied for a PermissionDenied answer.
func auditGRPCDenied(ctx context.Context, log domain.AuditLog, method string, err error) {
	if log == nil || status.Code(err) != codes.PermissionDenied {
		return
	}
	details := map[string]string{"method": method, "message": status.Convert(err).Message()}
	e := audit.FromPrincipal(iam.PrincipalFrom(ctx), audit.ActionAccessDenied, "rpc", method, details)
	if md, ok := metadata.FromIncomingContext(ctx); ok {
		if v := md.Get("x-wt-tree-id"); len(v) > 0 {
			e.Details["tree_id"] = v[0]
		}
	}
	log.Record(ctx, e)
}
