package syncsvc

import (
	"context"
	"crypto/rand"
	"errors"
	"fmt"
	"strings"
	"unicode"
	"unicode/utf8"

	"google.golang.org/grpc/codes"
	"google.golang.org/grpc/peer"
	"google.golang.org/grpc/status"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/iam"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// toStatus converts err into a gRPC status error. what names the operation
// or resource for the client-facing message ("load tree", "tag"...).
//
// Domain sentinel errors map to canonical codes. Only the sentinel text is
// exposed for most kinds so that wrapped internals (storage paths, ids of
// unrelated resources) never reach the client; ErrInvalid and *PushDenied
// carry user-facing explanations and are passed through verbatim. Anything
// unrecognised is logged and reported as a bare Internal error.
func (s *Service) toStatus(ctx context.Context, err error, what string) error {
	if err == nil {
		return nil
	}
	if _, ok := status.FromError(err); ok {
		return err
	}
	var denied *domain.PushDenied
	switch {
	case errors.Is(err, context.Canceled):
		return status.Error(codes.Canceled, "request canceled")
	case errors.Is(err, context.DeadlineExceeded):
		return status.Error(codes.DeadlineExceeded, "deadline exceeded")
	case errors.As(err, &denied):
		return status.Error(codes.PermissionDenied, denied.Error())
	case errors.Is(err, domain.ErrNotFound):
		return status.Errorf(codes.NotFound, "%s: not found", what)
	case errors.Is(err, domain.ErrAlreadyExists):
		return status.Errorf(codes.AlreadyExists, "%s: already exists", what)
	case errors.Is(err, domain.ErrConflict):
		return status.Errorf(codes.FailedPrecondition, "%s: conflicting concurrent update", what)
	case errors.Is(err, domain.ErrUnauthenticated):
		return status.Error(codes.Unauthenticated, "authentication required")
	case errors.Is(err, domain.ErrForbidden):
		return status.Errorf(codes.PermissionDenied, "%s: permission denied", what)
	case errors.Is(err, domain.ErrInvalid):
		return status.Error(codes.InvalidArgument, err.Error())
	}
	s.log.ErrorContext(ctx, "internal error", "op", what, "err", err)
	return status.Errorf(codes.Internal, "%s: internal error", what)
}

// invalidf returns an InvalidArgument status error.
func invalidf(format string, args ...any) error {
	return status.Errorf(codes.InvalidArgument, format, args...)
}

// requireAuth returns the authenticated principal or an Unauthenticated
// status error.
func requireAuth(ctx context.Context) (*domain.Principal, error) {
	p := iam.PrincipalFrom(ctx)
	if p == nil {
		return nil, status.Error(codes.Unauthenticated, "authentication required")
	}
	return p, nil
}

// authorize checks act for p and maps a refusal to a status error.
// Anonymous callers that are refused get Unauthenticated (they may succeed
// after logging in); authenticated ones get PermissionDenied.
func (s *Service) authorize(ctx context.Context, p *domain.Principal, act domain.Action, tenantID string, tree *domain.Tree) error {
	err := s.d.Authz.Authorize(ctx, p, act, tenantID, tree)
	if err == nil {
		return nil
	}
	if p == nil && (errors.Is(err, domain.ErrForbidden) || errors.Is(err, domain.ErrUnauthenticated)) {
		return status.Error(codes.Unauthenticated, "authentication required")
	}
	if errors.Is(err, domain.ErrForbidden) {
		return status.Errorf(codes.PermissionDenied, "permission denied: %s", act)
	}
	return s.toStatus(ctx, err, "authorize")
}

// loadTree fetches the tree referenced by id.
func (s *Service) loadTree(ctx context.Context, id *syncpb.TreeId) (*domain.Tree, error) {
	tid := id.GetValue()
	if tid == "" {
		return nil, invalidf("tree_id is required")
	}
	if !validID(tid) {
		return nil, invalidf("tree_id is malformed")
	}
	t, err := s.d.Store.GetTree(ctx, tid)
	if err != nil {
		return nil, s.toStatus(ctx, err, "tree")
	}
	return t, nil
}

// readableTree loads the tree and checks that the caller may read it.
func (s *Service) readableTree(ctx context.Context, id *syncpb.TreeId) (*domain.Tree, *domain.Principal, error) {
	t, err := s.loadTree(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	p := iam.PrincipalFrom(ctx)
	if err := s.authorize(ctx, p, domain.ActTreeRead, t.TenantID, t); err != nil {
		return nil, nil, err
	}
	return t, p, nil
}

// writableTree loads the tree and checks that the authenticated caller may
// write to it.
func (s *Service) writableTree(ctx context.Context, id *syncpb.TreeId) (*domain.Tree, *domain.Principal, error) {
	p, err := requireAuth(ctx)
	if err != nil {
		return nil, nil, err
	}
	t, err := s.loadTree(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	if err := s.authorize(ctx, p, domain.ActTreeWrite, t.TenantID, t); err != nil {
		return nil, nil, err
	}
	return t, p, nil
}

// remoteAddr returns the peer address of the call, or "".
func remoteAddr(ctx context.Context) string {
	if pr, ok := peer.FromContext(ctx); ok && pr.Addr != nil {
		return pr.Addr.String()
	}
	return ""
}

// newID returns a random RFC 4122 version 4 UUID string.
func newID() string {
	var b [16]byte
	// crypto/rand.Read never returns an error on supported platforms
	// (Go 1.24+ panics internally instead), so the result is ignored.
	_, _ = rand.Read(b[:])
	b[6] = (b[6] & 0x0f) | 0x40
	b[8] = (b[8] & 0x3f) | 0x80
	return fmt.Sprintf("%x-%x-%x-%x-%x", b[0:4], b[4:6], b[6:8], b[8:10], b[10:16])
}

// maxIDLen bounds opaque identifiers (tree, snapshot ids) accepted from
// clients.
const maxIDLen = 128

// validID reports whether s is acceptable as an opaque identifier: non-empty,
// bounded, printable ASCII without whitespace or path separators.
func validID(s string) bool {
	if s == "" || len(s) > maxIDLen {
		return false
	}
	for i := 0; i < len(s); i++ {
		c := s[i]
		if c <= ' ' || c >= 0x7f || c == '/' || c == '\\' {
			return false
		}
	}
	return true
}

// maxRefLen bounds branch and tag names.
const maxRefLen = 255

// validateRefName checks a branch or tag name. Names are '/'-separated
// components of printable characters; they may not contain whitespace,
// control characters, empty / "." / ".." components, or a few characters
// that are reserved by revision syntax (~ ^ : ? * [ \).
func validateRefName(kind, name string) error {
	if name == "" {
		return invalidf("%s name is required", kind)
	}
	if len(name) > maxRefLen {
		return invalidf("%s name is longer than %d bytes", kind, maxRefLen)
	}
	if !utf8.ValidString(name) {
		return invalidf("%s name is not valid UTF-8", kind)
	}
	for _, r := range name {
		if unicode.IsSpace(r) || unicode.IsControl(r) || strings.ContainsRune(`~^:?*[\`, r) {
			return invalidf("%s name %q contains an invalid character %q", kind, name, r)
		}
	}
	for _, comp := range strings.Split(name, "/") {
		if comp == "" || comp == "." || comp == ".." {
			return invalidf("%s name %q has an empty or relative path component", kind, name)
		}
	}
	return nil
}

// maxTreeNameLen bounds tree names.
const maxTreeNameLen = 100

// validateTreeName checks a tree name for creation: 1-100 characters from
// [A-Za-z0-9._-], not starting with '.' or '-'.
func validateTreeName(name string) error {
	if name == "" {
		return invalidf("tree_name is required")
	}
	if len(name) > maxTreeNameLen {
		return invalidf("tree_name is longer than %d characters", maxTreeNameLen)
	}
	if name[0] == '.' || name[0] == '-' {
		return invalidf("tree_name may not start with %q", name[0])
	}
	for i := 0; i < len(name); i++ {
		c := name[i]
		ok := (c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || c == '.' || c == '_' || c == '-'
		if !ok {
			return invalidf("tree_name %q contains an invalid character %q", name, c)
		}
	}
	return nil
}
