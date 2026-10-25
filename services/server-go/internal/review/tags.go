package review

import (
	"context"
	"fmt"
	"slices"
	"strings"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// TagProtectionCollection holds tag protections, keyed by
// DocKey(treeID, pattern).
const TagProtectionCollection = "tag_protections"

// TagOp is the tag operation checked by CheckTag.
type TagOp int

const (
	TagCreate TagOp = iota // create a new tag
	TagMove                // replace an existing tag (point it elsewhere)
	TagDelete              // delete a tag
)

func (op TagOp) String() string {
	switch op {
	case TagMove:
		return "move"
	case TagDelete:
		return "delete"
	}
	return "create"
}

// TagProtections lists the tag protections of a tree (pattern order). No
// authorization: callers check tree:read.
func (s *Service) TagProtections(ctx context.Context, treeID string) ([]domain.TagProtection, error) {
	out, err := meta.ListDocsT[domain.TagProtection](ctx, s.store, TagProtectionCollection, meta.DocPrefix(treeID))
	if err != nil {
		return nil, fmt.Errorf("review: list tag protections: %w", err)
	}
	for i := range out {
		if out[i].AllowedTaggers == nil {
			out[i].AllowedTaggers = []string{}
		}
	}
	return out, nil
}

// PutTagProtection creates or replaces the protection of a tag pattern.
// It requires domain.ActTreeAdmin.
func (s *Service) PutTagProtection(ctx context.Context, p *domain.Principal, tree *domain.Tree, tp domain.TagProtection) (*domain.TagProtection, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeAdmin, tree.TenantID, tree); err != nil {
		return nil, err
	}
	tp.Pattern = strings.TrimSpace(tp.Pattern)
	if err := ValidatePattern(tp.Pattern); err != nil {
		return nil, err
	}
	if len(tp.Pattern) > 255 || strings.ContainsRune(tp.Pattern, 0) {
		return nil, fmt.Errorf("tag pattern must be at most 255 bytes: %w", domain.ErrInvalid)
	}
	taggers := []string{}
	for _, u := range tp.AllowedTaggers {
		if u = strings.TrimSpace(u); u != "" && !slices.Contains(taggers, u) {
			taggers = append(taggers, u)
		}
	}
	slices.Sort(taggers)
	tp.AllowedTaggers = taggers
	tp.TreeID = tree.ID
	if err := s.store.PutDoc(ctx, TagProtectionCollection, domain.DocKey(tree.ID, tp.Pattern), &tp); err != nil {
		return nil, fmt.Errorf("review: store tag protection: %w", err)
	}
	return &tp, nil
}

// DeleteTagProtection removes the protection of a tag pattern
// (domain.ErrNotFound when missing). It requires domain.ActTreeAdmin.
func (s *Service) DeleteTagProtection(ctx context.Context, p *domain.Principal, tree *domain.Tree, pattern string) error {
	if err := checkTree(tree); err != nil {
		return err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeAdmin, tree.TenantID, tree); err != nil {
		return err
	}
	if pattern == "" {
		return fmt.Errorf("pattern is required: %w", domain.ErrInvalid)
	}
	if err := s.store.DeleteDoc(ctx, TagProtectionCollection, domain.DocKey(tree.ID, pattern)); err != nil {
		if isNotFound(err) {
			return fmt.Errorf("no tag protection for pattern %q: %w", pattern, domain.ErrNotFound)
		}
		return fmt.Errorf("review: delete tag protection: %w", err)
	}
	return nil
}

// CheckTagPush is CheckTag for a tag push: TagMove when the tag already
// exists (replace), TagCreate otherwise. It lets other packages enforce tag
// protections without importing TagOp.
func (s *Service) CheckTagPush(ctx context.Context, p *domain.Principal, tree *domain.Tree, name string, replace bool) error {
	op := TagCreate
	if replace {
		op = TagMove
	}
	return s.CheckTag(ctx, p, tree, name, op)
}

// CheckTag enforces the tag protections of tree for op on tag name. It does
// not authorize the operation itself (callers require tree:write or
// tree:admin); it returns *domain.PushDenied{Kind: "tag_protection"} when a
// matching rule forbids it:
//   - AllowedTaggers: only the listed users may create, move or delete
//     matching tags (every matching non-empty list must contain the caller);
//   - NoMove: an existing matching tag cannot be replaced;
//   - NoDelete: a matching tag cannot be deleted.
func (s *Service) CheckTag(ctx context.Context, p *domain.Principal, tree *domain.Tree, name string, op TagOp) error {
	if err := checkTree(tree); err != nil {
		return err
	}
	rules, err := s.TagProtections(ctx, tree.ID)
	if err != nil {
		return err
	}
	user := username(p)
	for _, r := range rules {
		if !MatchPattern(r.Pattern, name) {
			continue
		}
		if len(r.AllowedTaggers) > 0 && !slices.Contains(r.AllowedTaggers, user) {
			return &domain.PushDenied{Kind: "tag_protection", Reason: fmt.Sprintf(
				"tag '%s' is protected by '%s': %s may not %s it", name, r.Pattern, displayName(p), op)}
		}
		if op == TagMove && r.NoMove {
			return &domain.PushDenied{Kind: "tag_protection", Reason: fmt.Sprintf(
				"tag '%s' is protected by '%s' and cannot be moved", name, r.Pattern)}
		}
		if op == TagDelete && r.NoDelete {
			return &domain.PushDenied{Kind: "tag_protection", Reason: fmt.Sprintf(
				"tag '%s' is protected by '%s' and cannot be deleted", name, r.Pattern)}
		}
	}
	return nil
}
