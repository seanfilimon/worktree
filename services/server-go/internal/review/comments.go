package review

import (
	"context"
	"errors"
	"fmt"
	"sort"
	"strconv"
	"strings"
	"unicode/utf8"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
)

// CommentCollection holds merge request comments, keyed by
// DocKey(treeID, %08d number, id).
const CommentCollection = "mr_comments"

// MaxCommentPathLen bounds the file path a comment is anchored to.
const MaxCommentPathLen = 4096

// CommentInput creates a comment. Line requires Path; SnapshotID defaults
// to the source tip.
type CommentInput struct {
	Body       string `json:"body"`
	Path       string `json:"path,omitempty"`
	Line       int    `json:"line,omitempty"`
	SnapshotID string `json:"snapshot_id,omitempty"`

	// v0.7 (§15.7): a suggested change; Path, Line and SnapshotID are then
	// taken from it and Body may be empty.
	Suggestion *SuggestionInput `json:"suggestion,omitempty"`
}

// CommentPatch changes a comment; nil fields are left alone.
type CommentPatch struct {
	Body     *string `json:"body,omitempty"`
	Resolved *bool   `json:"resolved,omitempty"`
}

func commentKey(treeID string, number int, id string) string {
	return domain.DocKey(treeID, fmt.Sprintf("%08d", number), id)
}

func commentPrefix(treeID string, number int) string {
	return meta.DocPrefix(treeID, fmt.Sprintf("%08d", number))
}

func validateBody(body string) error {
	if strings.TrimSpace(body) == "" {
		return fmt.Errorf("comment body must not be empty: %w", domain.ErrInvalid)
	}
	if len(body) > MaxBodyLen || !utf8.ValidString(body) {
		return fmt.Errorf("comment body must be at most %d bytes of UTF-8: %w", MaxBodyLen, domain.ErrInvalid)
	}
	return nil
}

// Comments lists the comments of merge request number, oldest first. It
// requires domain.ActTreeRead.
func (s *Service) Comments(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) ([]domain.ReviewComment, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if _, err := s.getMR(ctx, tree.ID, number); err != nil {
		return nil, err
	}
	out, err := meta.ListDocsT[domain.ReviewComment](ctx, s.store, CommentCollection, commentPrefix(tree.ID, number))
	if err != nil {
		return nil, fmt.Errorf("review: list comments: %w", err)
	}
	sort.SliceStable(out, func(i, j int) bool {
		a, _ := strconv.Atoi(out[i].ID)
		b, _ := strconv.Atoi(out[j].ID)
		return a < b
	})
	return out, nil
}

// AddComment comments on merge request number (any state). It requires
// domain.ActTreeRead and a signed-in caller; publishes mr.commented.
func (s *Service) AddComment(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, in CommentInput) (*domain.ReviewComment, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	mr, err := s.getMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	if in.SnapshotID == "" && (in.Suggestion == nil || in.Suggestion.SnapshotID == "") {
		in.SnapshotID = s.currentSourceTip(ctx, tree, mr) // forks (§16.2)
	}
	sg, err := s.suggestionFor(ctx, tree, &in) // v0.7 (§15.7)
	if err != nil {
		return nil, err
	}
	c, err := s.storeComment(ctx, p, tree, mr, in, sg)
	if err != nil {
		return nil, err
	}
	s.publish(domain.EventMRCommented, p, tree, *c)
	details := map[string]string{"comment_id": c.ID, "path": c.Path}
	if sg != nil {
		details["suggestion"] = suggestionLabel(sg)
	}
	s.record(ctx, "mr.comment", p, tree, mr, details)
	return c, nil
}

// storeComment validates and stores a new comment on mr.
func (s *Service) storeComment(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, in CommentInput, sg *domain.Suggestion) (*domain.ReviewComment, error) {
	if sg == nil || in.Body != "" {
		if err := validateBody(in.Body); err != nil {
			return nil, err
		}
	}
	switch {
	case len(in.Path) > MaxCommentPathLen || !utf8.ValidString(in.Path) || strings.ContainsRune(in.Path, 0):
		return nil, fmt.Errorf("path must be at most %d bytes of UTF-8: %w", MaxCommentPathLen, domain.ErrInvalid)
	case in.Line < 0:
		return nil, fmt.Errorf("line must not be negative: %w", domain.ErrInvalid)
	case in.Line > 0 && in.Path == "":
		return nil, fmt.Errorf("line requires path: %w", domain.ErrInvalid)
	case len(in.SnapshotID) > 128:
		return nil, fmt.Errorf("snapshot_id is malformed: %w", domain.ErrInvalid)
	}
	seq, err := s.store.NextSeq(ctx, CommentCollection+"/"+tree.ID+"/"+strconv.Itoa(mr.Number))
	if err != nil {
		return nil, fmt.Errorf("review: comment id: %w", err)
	}
	now := s.now()
	c := &domain.ReviewComment{
		ID:         strconv.FormatInt(seq, 10),
		TreeID:     tree.ID,
		MRNumber:   mr.Number,
		AuthorID:   p.AccountID,
		Author:     p.Username,
		Body:       in.Body,
		Path:       in.Path,
		Line:       in.Line,
		SnapshotID: in.SnapshotID,
		CreatedAt:  now,
		UpdatedAt:  now,
		Suggestion: sg,
	}
	if err := s.store.CreateDoc(ctx, CommentCollection, commentKey(tree.ID, mr.Number, c.ID), c); err != nil {
		return nil, fmt.Errorf("review: store comment: %w", err)
	}
	return c, nil
}

// UpdateComment edits a comment (body, resolved). Only its author or a
// caller with domain.ActTreeAdmin may change it.
func (s *Service) UpdateComment(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, id string, patch CommentPatch) (*domain.ReviewComment, error) {
	if patch.Body == nil && patch.Resolved == nil {
		return nil, fmt.Errorf("at least one of body, resolved is required: %w", domain.ErrInvalid)
	}
	if patch.Body != nil {
		if err := validateBody(*patch.Body); err != nil {
			return nil, err
		}
	}
	c, err := s.commentForWrite(ctx, p, tree, number, id)
	if err != nil {
		return nil, err
	}
	var out *domain.ReviewComment
	err = meta.UpdateDocT(ctx, s.store, CommentCollection, commentKey(tree.ID, number, c.ID),
		func(cur *domain.ReviewComment) (*domain.ReviewComment, error) {
			if cur == nil {
				return nil, fmt.Errorf("comment %s not found: %w", id, domain.ErrNotFound)
			}
			if patch.Body != nil {
				cur.Body = *patch.Body
			}
			if patch.Resolved != nil {
				cur.Resolved = *patch.Resolved
			}
			cur.UpdatedAt = s.now()
			out = cur
			return cur, nil
		})
	if err != nil {
		return nil, err
	}
	return out, nil
}

// DeleteComment removes a comment. Only its author or a caller with
// domain.ActTreeAdmin may delete it.
func (s *Service) DeleteComment(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, id string) error {
	c, err := s.commentForWrite(ctx, p, tree, number, id)
	if err != nil {
		return err
	}
	if err := s.store.DeleteDoc(ctx, CommentCollection, commentKey(tree.ID, number, c.ID)); err != nil {
		if isNotFound(err) {
			return fmt.Errorf("comment %s not found: %w", id, domain.ErrNotFound)
		}
		return fmt.Errorf("review: delete comment: %w", err)
	}
	return nil
}

// commentForWrite loads a comment and checks that p may change it.
func (s *Service) commentForWrite(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, id string) (*domain.ReviewComment, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	if id == "" || strings.ContainsRune(id, 0) {
		return nil, fmt.Errorf("comment id is required: %w", domain.ErrInvalid)
	}
	c, err := meta.GetDocT[domain.ReviewComment](ctx, s.store, CommentCollection, commentKey(tree.ID, number, id))
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("comment %s on merge request !%d not found: %w", id, number, domain.ErrNotFound)
		}
		return nil, fmt.Errorf("review: get comment: %w", err)
	}
	if c.AuthorID != p.AccountID {
		if err := s.authz.Authorize(ctx, p, domain.ActTreeAdmin, tree.TenantID, tree); err != nil {
			return nil, err
		}
	}
	return c, nil
}
