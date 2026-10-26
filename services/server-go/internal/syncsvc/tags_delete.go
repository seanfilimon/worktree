package syncsvc

// v0.8 history-v2 (docs/ENGINEERING_PLAN.md §16.10): the DeleteTag RPC
// (`wt tag delete NAME --remote`).

import (
	"context"
	"errors"
	"strings"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/review"
)

// EventTagDeleted is published when a tag is deleted (the same type the
// REST endpoint DELETE {tree}/tags/{name} publishes).
const EventTagDeleted = "tag.deleted"

// tagDeletePolicy is implemented by review services that enforce tag
// protections (internal/review.Service.CheckTag).
type tagDeletePolicy interface {
	CheckTag(ctx context.Context, p *domain.Principal, tree *domain.Tree, name string, op review.TagOp) error
}

// maxTagNameLen bounds DeleteTagRequest.name. Names are not re-validated
// with the ref-name rules so tags pushed by older clients stay deletable.
const maxTagNameLen = 1024

// DeleteTag deletes a tag of the tree. It needs tree:write; tag
// protections apply (NoDelete, AllowedTaggers). Domain-level refusals (no
// such tag, protected tag) are reported in the response; malformed
// requests and authorization failures are status errors. Publishes
// tag.deleted and audits tag.delete (refusals: tag.delete_rejected).
func (s *Service) DeleteTag(ctx context.Context, req *syncpb.DeleteTagRequest) (*syncpb.DeleteTagResponse, error) {
	t, p, err := s.writableTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	name := req.GetName()
	if strings.TrimSpace(name) == "" || len(name) > maxTagNameLen || strings.ContainsRune(name, 0) {
		return nil, invalidf("tag name must be 1-%d bytes without NUL", maxTagNameLen)
	}
	tag, err := s.d.Store.GetTag(ctx, t.ID, name)
	switch {
	case errors.Is(err, domain.ErrNotFound):
		return &syncpb.DeleteTagResponse{Deleted: false, Error: proto.String("tag " + name + " not found")}, nil
	case err != nil:
		return nil, s.toStatus(ctx, err, "tag")
	}
	if tp, ok := s.d.Review.(tagDeletePolicy); ok {
		if err := tp.CheckTag(ctx, p, t, name, review.TagDelete); err != nil {
			var denied *domain.PushDenied
			if errors.As(err, &denied) {
				s.audit(ctx, p, domain.AuditEvent{
					TenantID: t.TenantID, Action: "tag.delete_rejected", ResourceType: "tag",
					ResourceID: t.ID + "/" + name,
					Details:    map[string]string{"tree": t.Name, "tag": name, "reason": denied.Reason},
				})
				return &syncpb.DeleteTagResponse{Deleted: false, Error: proto.String(denied.Reason)}, nil
			}
			return nil, s.toStatus(ctx, err, "tag protection")
		}
	}
	if err := s.d.Store.DeleteTag(ctx, t.ID, name); err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return &syncpb.DeleteTagResponse{Deleted: false, Error: proto.String("tag " + name + " not found")}, nil
		}
		return nil, s.toStatus(ctx, err, "delete tag")
	}
	s.publish(domain.Event{
		Type:     EventTagDeleted,
		TenantID: t.TenantID,
		TreeID:   t.ID,
		Actor:    p.Username,
		Payload:  map[string]any{"tag": name, "target": tag.Target},
	})
	s.audit(ctx, p, domain.AuditEvent{
		TenantID:     t.TenantID,
		Action:       "tag.delete",
		ResourceType: "tag",
		ResourceID:   t.ID + "/" + name,
		Details:      map[string]string{"tree": t.Name, "tag": name, "target": tag.Target, "via": "grpc"},
	})
	return &syncpb.DeleteTagResponse{Deleted: true}, nil
}
