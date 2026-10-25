package syncsvc

import (
	"context"
	"errors"
	"sort"
	"strings"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/status"
	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// DefaultBranch is the default branch of trees created by ResolveTree.
const DefaultBranch = "main"

// ResolveTree looks a tree up by tenant slug and name. With
// create_if_missing an absent tree is created (requires tree:create on the
// tenant) as a private tree with default branch "main".
func (s *Service) ResolveTree(ctx context.Context, req *syncpb.ResolveTreeRequest) (*syncpb.TreeInfo, error) {
	slug := strings.TrimSpace(req.GetTenantSlug())
	name := strings.TrimSpace(req.GetTreeName())
	if slug == "" {
		return nil, invalidf("tenant_slug is required")
	}
	if name == "" {
		return nil, invalidf("tree_name is required")
	}
	p := iam.PrincipalFrom(ctx)

	tenant, err := s.d.Store.GetTenantBySlug(ctx, slug)
	if err != nil {
		return nil, s.toStatus(ctx, err, "tenant")
	}

	if t, err := s.d.Store.GetTreeByName(ctx, tenant.ID, name); err == nil {
		if err := s.authorize(ctx, p, domain.ActTreeRead, tenant.ID, t); err != nil {
			return nil, err
		}
		return treeToPB(t, tenant.Slug, false), nil
	} else if !errors.Is(err, domain.ErrNotFound) {
		return nil, s.toStatus(ctx, err, "tree")
	}

	if !req.GetCreateIfMissing() {
		return nil, status.Error(codes.NotFound, "tree: not found")
	}
	if p == nil {
		return nil, status.Error(codes.Unauthenticated, "authentication required")
	}
	if err := validateTreeName(name); err != nil {
		return nil, err
	}
	if err := s.authorize(ctx, p, domain.ActTreeCreate, tenant.ID, nil); err != nil {
		return nil, err
	}

	now := s.now()
	t := &domain.Tree{
		ID:            newID(),
		TenantID:      tenant.ID,
		Name:          name,
		DefaultBranch: DefaultBranch,
		Visibility:    domain.VisibilityPrivate,
		CreatedAt:     now,
		UpdatedAt:     now,
	}
	if err := s.d.Store.CreateTree(ctx, t); err != nil {
		if !errors.Is(err, domain.ErrAlreadyExists) {
			return nil, s.toStatus(ctx, err, "create tree")
		}
		// Lost a creation race: resolve the winner's tree instead.
		existing, gerr := s.d.Store.GetTreeByName(ctx, tenant.ID, name)
		if gerr != nil {
			return nil, s.toStatus(ctx, gerr, "tree")
		}
		if err := s.authorize(ctx, p, domain.ActTreeRead, tenant.ID, existing); err != nil {
			return nil, err
		}
		return treeToPB(existing, tenant.Slug, false), nil
	}

	s.publish(domain.Event{
		Type:     domain.EventTreeCreated,
		TenantID: tenant.ID,
		TreeID:   t.ID,
		Actor:    p.Username,
		Payload:  map[string]any{"name": t.Name, "tenant": tenant.Slug},
	})
	s.audit(ctx, p, domain.AuditEvent{
		TenantID:     tenant.ID,
		Action:       "tree.create",
		ResourceType: "tree",
		ResourceID:   t.ID,
		Details:      map[string]string{"name": t.Name, "tenant": tenant.Slug, "via": "grpc"},
	})
	return treeToPB(t, tenant.Slug, true), nil
}

// ListBranches lists the branches of a tree, sorted by name.
func (s *Service) ListBranches(ctx context.Context, req *syncpb.ListBranchesRequest) (*syncpb.ListBranchesResponse, error) {
	t, _, err := s.readableTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	branches, err := s.d.Store.ListBranches(ctx, t.ID)
	if err != nil {
		return nil, s.toStatus(ctx, err, "list branches")
	}
	sort.Slice(branches, func(i, j int) bool { return branches[i].Name < branches[j].Name })
	resp := &syncpb.ListBranchesResponse{Branches: make([]*syncpb.BranchInfo, 0, len(branches))}
	for i := range branches {
		resp.Branches = append(resp.Branches, branchToPB(&branches[i]))
	}
	return resp, nil
}

// ListTags lists the tags of a tree, sorted by name.
func (s *Service) ListTags(ctx context.Context, req *syncpb.ListTagsRequest) (*syncpb.ListTagsResponse, error) {
	t, _, err := s.readableTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	tags, err := s.d.Store.ListTags(ctx, t.ID)
	if err != nil {
		return nil, s.toStatus(ctx, err, "list tags")
	}
	sort.Slice(tags, func(i, j int) bool { return tags[i].Name < tags[j].Name })
	resp := &syncpb.ListTagsResponse{Tags: make([]*syncpb.TagRecord, 0, len(tags))}
	for i := range tags {
		resp.Tags = append(resp.Tags, tagToPB(&tags[i]))
	}
	return resp, nil
}

// tagPolicy is implemented by review services that enforce tag protections
// (internal/review.Service, §13.4). replace is true when the tag exists.
type tagPolicy interface {
	CheckTagPush(ctx context.Context, p *domain.Principal, tree *domain.Tree, name string, replace bool) error
}

// maxTagMessageLen bounds tag annotations.
const maxTagMessageLen = 64 << 10

// PushTag creates a tag pointing at an existing snapshot of the tree. Tags
// are immutable: pushing an existing name is refused with accepted=false.
// Domain-level refusals (unknown target, name taken) are reported in the
// response; malformed requests and authorization failures are status errors.
func (s *Service) PushTag(ctx context.Context, req *syncpb.PushTagRequest) (*syncpb.PushTagResponse, error) {
	t, p, err := s.writableTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	rec := req.GetTag()
	if rec == nil {
		return nil, invalidf("tag is required")
	}
	if err := validateRefName("tag", rec.GetName()); err != nil {
		return nil, err
	}
	target := rec.GetTarget().GetValue()
	if !validID(target) {
		return nil, invalidf("tag target is missing or malformed")
	}
	if len(rec.GetMessage()) > maxTagMessageLen {
		return nil, invalidf("tag message is longer than %d bytes", maxTagMessageLen)
	}

	snap, err := s.d.Store.GetSnapshot(ctx, target)
	switch {
	case errors.Is(err, domain.ErrNotFound), err == nil && snap.TreeID != t.ID:
		return &syncpb.PushTagResponse{Accepted: false, Error: proto.String("target snapshot " + target + " not found in this tree")}, nil
	case err != nil:
		return nil, s.toStatus(ctx, err, "tag target")
	}

	created := tsFromPB(rec.GetCreatedAt())
	if created.IsZero() {
		created = s.now()
	}
	tag := &domain.Tag{
		TreeID:  t.ID,
		Name:    rec.GetName(),
		Target:  target,
		Message: rec.GetMessage(),
		// The tagger is the authenticated pusher; client-supplied names
		// are not trusted.
		Tagger:    p.Username,
		CreatedAt: created,
	}
	// Tag protections (§13.4): AllowedTaggers for new tags, NoMove for
	// existing ones (tags stay immutable either way).
	if tp, ok := s.d.Review.(tagPolicy); ok {
		_, gerr := s.d.Store.GetTag(ctx, t.ID, tag.Name)
		if gerr != nil && !errors.Is(gerr, domain.ErrNotFound) {
			return nil, s.toStatus(ctx, gerr, "tag")
		}
		if err := tp.CheckTagPush(ctx, p, t, tag.Name, gerr == nil); err != nil {
			var denied *domain.PushDenied
			if errors.As(err, &denied) {
				s.audit(ctx, p, domain.AuditEvent{
					TenantID: t.TenantID, Action: "tag.push_rejected", ResourceType: "tag",
					ResourceID: t.ID + "/" + tag.Name,
					Details:    map[string]string{"tree": t.Name, "tag": tag.Name, "reason": denied.Reason},
				})
				return &syncpb.PushTagResponse{Accepted: false, Error: proto.String(denied.Reason)}, nil
			}
			return nil, s.toStatus(ctx, err, "tag protection")
		}
	}
	if err := s.d.Store.PutTag(ctx, tag); err != nil {
		if errors.Is(err, domain.ErrAlreadyExists) {
			return &syncpb.PushTagResponse{Accepted: false, Error: proto.String("tag " + tag.Name + " already exists")}, nil
		}
		return nil, s.toStatus(ctx, err, "tag")
	}

	s.publish(domain.Event{
		Type:     domain.EventTagPushed,
		TenantID: t.TenantID,
		TreeID:   t.ID,
		Actor:    p.Username,
		Payload:  map[string]any{"tag": tag.Name, "target": tag.Target},
	})
	s.audit(ctx, p, domain.AuditEvent{
		TenantID:     t.TenantID,
		Action:       "tag.push",
		ResourceType: "tag",
		ResourceID:   t.ID + "/" + tag.Name,
		Details:      map[string]string{"tree": t.Name, "tag": tag.Name, "target": tag.Target},
	})
	return &syncpb.PushTagResponse{Accepted: true}, nil
}

// ListStaged lists the staged (WIP) snapshots of a tree visible to the
// caller, optionally restricted to one branch.
func (s *Service) ListStaged(ctx context.Context, req *syncpb.ListStagedRequest) (*syncpb.ListStagedResponse, error) {
	if s.d.Staged == nil {
		return nil, status.Error(codes.Unimplemented, "staged snapshots are not enabled on this server")
	}
	t, p, err := s.readableTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	branch := req.GetBranchName()
	if branch != "" {
		if err := validateRefName("branch", branch); err != nil {
			return nil, err
		}
	}
	staged, err := s.d.Staged.List(ctx, p, t, branch)
	if err != nil {
		return nil, s.toStatus(ctx, err, "list staged")
	}
	resp := &syncpb.ListStagedResponse{Staged: make([]*syncpb.StagedInfo, 0, len(staged))}
	for i := range staged {
		resp.Staged = append(resp.Staged, stagedToPB(&staged[i]))
	}
	return resp, nil
}

// Limits on staged uploads.
const (
	maxStagedFiles      = 100_000
	maxStagedMessageLen = 64 << 10
)

// Stage records a team-visible WIP snapshot for the caller. The tenant,
// account and author are derived from the tree and the authenticated
// principal; the request's tenant_id / account_id fields are ignored.
func (s *Service) Stage(ctx context.Context, req *syncpb.StageUploadRequest) (*syncpb.StageUploadResponse, error) {
	if s.d.Staged == nil {
		return nil, status.Error(codes.Unimplemented, "staged snapshots are not enabled on this server")
	}
	t, p, err := s.writableTree(ctx, req.GetTreeId())
	if err != nil {
		return nil, err
	}
	snapID := req.GetSnapshotId().GetValue()
	if !validID(snapID) {
		return nil, invalidf("snapshot_id is missing or malformed")
	}
	branch := req.GetBranchName()
	if branch == "" {
		branch = req.GetBranchId().GetValue()
	}
	if branch == "" {
		branch = t.DefaultBranch
	}
	if err := validateRefName("branch", branch); err != nil {
		return nil, err
	}
	if len(req.GetFilesChanged()) > maxStagedFiles {
		return nil, invalidf("files_changed lists more than %d paths", maxStagedFiles)
	}
	if len(req.GetMessage()) > maxStagedMessageLen {
		return nil, invalidf("message is longer than %d bytes", maxStagedMessageLen)
	}

	st := &domain.StagedSnapshot{
		SnapshotID:    snapID,
		TreeID:        t.ID,
		TenantID:      t.TenantID,
		AccountID:     p.AccountID,
		Author:        p.Username,
		BranchName:    branch,
		FilesChanged:  append([]string(nil), req.GetFilesChanged()...),
		FilesAdded:    req.GetFilesAdded(),
		FilesModified: req.GetFilesModified(),
		FilesDeleted:  req.GetFilesDeleted(),
		Message:       req.GetMessage(),
		StagedAt:      s.now(),
	}
	if err := s.d.Staged.Stage(ctx, p, t, st); err != nil {
		if errors.Is(err, domain.ErrInvalid) {
			return &syncpb.StageUploadResponse{Accepted: false, Error: proto.String(err.Error())}, nil
		}
		return nil, s.toStatus(ctx, err, "stage")
	}
	return &syncpb.StageUploadResponse{Accepted: true}, nil
}
