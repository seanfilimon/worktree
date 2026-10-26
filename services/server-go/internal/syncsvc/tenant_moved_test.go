package syncsvc

import (
	"context"
	"strings"
	"testing"
	"time"

	"google.golang.org/genproto/googleapis/rpc/errdetails"
	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// movedStore is the fake store plus a real document store.
type movedStore struct {
	*fakeStore
	docs *meta.Memory
}

func (m movedStore) GetDoc(ctx context.Context, coll, key string, v any) error {
	return m.docs.GetDoc(ctx, coll, key, v)
}

func TestResolveTreeOfRenamedTenantAnswersTenantMoved(t *testing.T) {
	st := newFakeStore()
	docs := meta.NewMemory()
	ctx := context.Background()
	acme := &domain.Tenant{ID: "t-acme", Slug: "acme-inc", Name: "Acme", Kind: domain.TenantOrg, CreatedAt: time.Now()}
	st.tenants[acme.ID] = acme
	if err := docs.PutDoc(ctx, iam.CollTenantSlugs, "acme", iam.SlugReservation{
		Slug: "acme", TenantID: acme.ID, RenamedTo: "acme-inc", ReservedAt: time.Now(), ExpiresAt: time.Now().Add(time.Hour),
	}); err != nil {
		t.Fatal(err)
	}
	svc := New(Deps{Store: movedStore{fakeStore: st, docs: docs}, Authz: &fakeAuthz{}})
	_, err := svc.ResolveTree(ctx, &syncpb.ResolveTreeRequest{TenantSlug: "acme", TreeName: "web"})
	s, ok := status.FromError(err)
	if !ok || s.Code() != codes.NotFound {
		t.Fatalf("ResolveTree(old slug) = %v, want NotFound", err)
	}
	var info *errdetails.ErrorInfo
	for _, d := range s.Details() {
		if ei, ok := d.(*errdetails.ErrorInfo); ok {
			info = ei
		}
	}
	if info == nil || info.Reason != TenantMovedReason || info.Metadata["new_slug"] != "acme-inc" || info.Metadata["old_slug"] != "acme" {
		t.Fatalf("details = %v, want ErrorInfo TENANT_MOVED acme -> acme-inc", s.Details())
	}
	if want := `renamed to "acme-inc"`; !strings.Contains(s.Message(), want) {
		t.Fatalf("message %q does not mention %s", s.Message(), want)
	}

	// An unknown slug stays a plain NotFound.
	_, err = svc.ResolveTree(ctx, &syncpb.ResolveTreeRequest{TenantSlug: "nobody", TreeName: "web"})
	if s, _ := status.FromError(err); s.Code() != codes.NotFound || len(s.Details()) != 0 {
		t.Fatalf("unknown tenant = %v", err)
	}
}
