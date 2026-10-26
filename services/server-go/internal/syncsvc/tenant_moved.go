package syncsvc

// tenant-lifecycle (docs/ENGINEERING_PLAN.md §16.4): requests naming the
// old slug of a renamed tenant answer NotFound with the new slug in an
// ErrorInfo detail (reason TENANT_MOVED, metadata old_slug / new_slug) and
// in the trailers wt-tenant-moved-from / wt-tenant-moved-to, so clients
// can tell the user to update the remote.

import (
	"context"
	"errors"
	"fmt"

	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/metadata"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
)

// TenantMovedReason is the ErrorInfo reason of tenant_moved errors.
const TenantMovedReason = "TENANT_MOVED"

// Trailer keys of tenant_moved errors.
const (
	TrailerTenantMovedFrom = "wt-tenant-moved-from"
	TrailerTenantMovedTo   = "wt-tenant-moved-to"
)

// tenantBySlug resolves a tenant slug; a reserved old slug answers
// tenantMovedStatus.
func (s *Service) tenantBySlug(ctx context.Context, slug string) (*domain.Tenant, error) {
	t, err := s.d.Store.GetTenantBySlug(ctx, slug)
	if err == nil {
		return t, nil
	}
	if !errors.Is(err, domain.ErrNotFound) {
		return nil, s.toStatus(ctx, err, "tenant")
	}
	if newSlug, ok, merr := iam.MovedSlug(ctx, s.d.Store, slug); merr == nil && ok {
		return nil, tenantMovedStatus(ctx, slug, newSlug)
	}
	return nil, s.toStatus(ctx, err, "tenant")
}

// tenantMovedStatus builds the NotFound status of a renamed tenant.
func tenantMovedStatus(ctx context.Context, oldSlug, newSlug string) error {
	st := status.New(codes.NotFound, fmt.Sprintf("tenant %q was renamed to %q; update the remote (wt tenant switch %s)", oldSlug, newSlug, newSlug))
	if withInfo, err := st.WithDetails(&errdetails.ErrorInfo{
		Reason:   TenantMovedReason,
		Domain:   "worktree",
		Metadata: map[string]string{"old_slug": oldSlug, "new_slug": newSlug},
	}); err == nil {
		st = withInfo
	}
	// Outside a gRPC call (unit tests) there is no stream to set trailers on.
	_ = grpc.SetTrailer(ctx, metadata.Pairs(TrailerTenantMovedFrom, oldSlug, TrailerTenantMovedTo, newSlug))
	return st.Err()
}
