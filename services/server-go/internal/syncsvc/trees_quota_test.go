package syncsvc

import (
	"context"
	"strings"
	"testing"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// maxTrees is a TreeQuota allowing max trees per tenant (counted in the
// fake store), like limits.Limiter.CheckTreeCreate.
type maxTrees struct {
	store *fakeStore
	max   int
}

func (q maxTrees) CheckTreeCreate(_ context.Context, t *domain.Tenant) error {
	if q.store.treeCount(t.ID) >= q.max {
		return &limits.QuotaError{Limit: limits.LimitTrees, Max: int64(q.max)}
	}
	return nil
}

func (s *fakeStore) treeCount(tenantID string) int {
	s.mu.Lock()
	defer s.mu.Unlock()
	n := 0
	for _, t := range s.trees {
		if t.TenantID == tenantID {
			n++
		}
	}
	return n
}

// ResolveTree(create_if_missing) must honour the MaxTrees quota (a push of
// a new worktree must not bypass it), while resolving existing trees keeps
// working at the limit.
func TestResolveTreeHonoursTreeQuota(t *testing.T) {
	h := newHarness(t)
	h.svc.d.TreeQuota = maxTrees{store: h.store, max: 1}
	req := func(name string) *syncpb.ResolveTreeRequest {
		return &syncpb.ResolveTreeRequest{TenantSlug: "acme", TreeName: name, CreateIfMissing: true}
	}

	if _, err := h.client.ResolveTree(h.as("alice"), req("web")); err != nil {
		t.Fatalf("first tree: %v", err)
	}
	_, err := h.client.ResolveTree(h.as("alice"), req("api"))
	wantCode(t, err, codes.PermissionDenied)
	if msg := status.Convert(err).Message(); !strings.Contains(msg, "quota_exceeded") || !strings.Contains(msg, limits.LimitTrees) {
		t.Fatalf("quota error message = %q, want quota_exceeded and %s", msg, limits.LimitTrees)
	}
	if n := h.store.treeCount(tenantAcme); n != 1 {
		t.Fatalf("trees after the refused create = %d, want 1", n)
	}
	// The existing tree still resolves.
	if _, err := h.client.ResolveTree(h.as("alice"), req("web")); err != nil {
		t.Fatalf("existing tree at the limit: %v", err)
	}
}
