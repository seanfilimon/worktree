package review

// v0.7 deeper review (docs/ENGINEERING_PLAN.md §15.7): suggested changes
// and tenant storage quotas for server-written snapshots.

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"unicode/utf8"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/meta"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/textdiff"
)

// Error codes of the v0.7 errors.
const (
	CodeSuggestionOutdated = "suggestion_outdated"
	CodeQuotaExceeded      = "quota_exceeded"
)

// Bounds of suggestions.
const (
	MaxSuggestionLines       = 10000
	MaxSuggestionReplacement = 256 << 10
	MaxSuggestionFileSize    = 8 << 20
)

// SuggestionInput is the suggestion of a new comment (CommentInput).
type SuggestionInput struct {
	Path        string `json:"path"`
	StartLine   int    `json:"start_line"`
	EndLine     int    `json:"end_line"`
	Replacement string `json:"replacement"`
	// SnapshotID defaults to the comment's snapshot, else the source tip.
	SnapshotID string `json:"snapshot_id,omitempty"`
}

// AppliedSuggestion is the result of ApplySuggestion.
type AppliedSuggestion struct {
	Comment  *domain.ReviewComment `json:"comment"`
	Snapshot *domain.Snapshot      `json:"snapshot"`
}

// OutdatedError refuses a suggestion whose lines changed on the source
// (code suggestion_outdated). It wraps domain.ErrConflict.
type OutdatedError struct{ Message string }

func (e *OutdatedError) Error() string { return e.Message }

// Unwrap makes errors.Is(err, domain.ErrConflict) hold.
func (e *OutdatedError) Unwrap() error { return domain.ErrConflict }

// QuotaError refuses a server-written snapshot that would exceed the
// tenant's storage quota (code quota_exceeded). It wraps domain.ErrConflict.
type QuotaError struct {
	Limit   string
	Message string
}

func (e *QuotaError) Error() string { return e.Message }

// Unwrap makes errors.Is(err, domain.ErrConflict) hold.
func (e *QuotaError) Unwrap() error { return domain.ErrConflict }

// SetQuota installs the tenant quota check (internal/limits.Limiter) run on
// the objects of server-side merge and squash snapshots. Call before
// serving requests.
func (s *Service) SetQuota(q domain.PushChecker) { s.quota = q }

// SetPushChecks installs the push checks run on suggestion snapshots
// written to a source branch (path ACLs, quotas, signatures), in order.
// Without them only the quota check runs. Call before serving requests.
func (s *Service) SetPushChecks(checks ...domain.PushChecker) { s.pushChecks = checks }

// quotaError converts a quota PushDenied into a *QuotaError.
func quotaError(err error) error {
	var denied *domain.PushDenied
	if errors.As(err, &denied) && denied.Kind == CodeQuotaExceeded {
		return &QuotaError{Limit: denied.Reason, Message: fmt.Sprintf("tenant storage quota exceeded (%s): the snapshot would add more data than the tenant may store", denied.Reason)}
	}
	return err
}

// checkMergeQuota sizes the objects a merge or squash snapshot adds (on top
// of what the target and source tips reference) against the tenant quota.
func (s *Service) checkMergeQuota(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch, oldTip string, snap *domain.Snapshot, sizingParents []string) error {
	if s.quota == nil {
		return nil
	}
	sizing := *snap
	sizing.Parents = sizingParents
	plan := &domain.PushPlan{Branch: branch, OldTip: oldTip, NewTip: snap.ID, NewSnapshots: []*domain.Snapshot{&sizing}}
	if err := s.quota.CheckPushPlan(ctx, p, tree, plan); err != nil {
		return quotaError(err)
	}
	return nil
}

// resolveSuggestion validates a suggestion against the file it targets.
func (s *Service) resolveSuggestion(ctx context.Context, tree *domain.Tree, in *SuggestionInput, snapshotID string) (*domain.Suggestion, error) {
	if s.objects == nil {
		return nil, fmt.Errorf("suggested changes are not available on this server: %w", domain.ErrInvalid)
	}
	in.Path = strings.TrimSpace(in.Path)
	switch {
	case in.Path == "" || len(in.Path) > MaxCommentPathLen || !utf8.ValidString(in.Path) || strings.ContainsRune(in.Path, 0):
		return nil, fmt.Errorf("suggestion path is required (at most %d bytes): %w", MaxCommentPathLen, domain.ErrInvalid)
	case in.StartLine < 1 || in.EndLine < in.StartLine:
		return nil, fmt.Errorf("suggestion lines must satisfy 1 <= start_line <= end_line: %w", domain.ErrInvalid)
	case in.EndLine-in.StartLine >= MaxSuggestionLines:
		return nil, fmt.Errorf("a suggestion replaces at most %d lines: %w", MaxSuggestionLines, domain.ErrInvalid)
	case len(in.Replacement) > MaxSuggestionReplacement || !utf8.ValidString(in.Replacement):
		return nil, fmt.Errorf("suggestion replacement must be at most %d bytes of UTF-8: %w", MaxSuggestionReplacement, domain.ErrInvalid)
	}
	if in.SnapshotID != "" {
		snapshotID = in.SnapshotID
	}
	if snapshotID == "" {
		return nil, fmt.Errorf("suggestion needs a snapshot (the source branch is empty): %w", domain.ErrInvalid)
	}
	sn, err := s.store.GetSnapshot(ctx, snapshotID)
	if err != nil || sn.TreeID != tree.ID {
		return nil, fmt.Errorf("suggestion snapshot %s not found in this tree: %w", shortID(snapshotID), domain.ErrInvalid)
	}
	data, _, err := s.fileAt(ctx, sn, in.Path)
	if err != nil {
		return nil, fmt.Errorf("%v: %w", err, domain.ErrInvalid)
	}
	if n := len(splitLines(string(data))); in.EndLine > n {
		return nil, fmt.Errorf("%s has %d lines; lines %d-%d are out of range: %w", in.Path, n, in.StartLine, in.EndLine, domain.ErrInvalid)
	}
	return &domain.Suggestion{
		Path: in.Path, StartLine: in.StartLine, EndLine: in.EndLine,
		Replacement: in.Replacement, SnapshotID: snapshotID,
	}, nil
}

// errNoFile reports a path missing from a snapshot.
var errNoFile = errors.New("file not found")

// fileAt returns the text content and manifest entry of path in snapshot
// sn. Binary, chunked and oversized files are refused.
func (s *Service) fileAt(ctx context.Context, sn *domain.Snapshot, path string) ([]byte, *syncpb.ManifestEntry, error) {
	m, err := s.loadManifest(ctx, sn.ManifestHash)
	if err != nil {
		return nil, nil, err
	}
	for _, e := range m.GetEntries() {
		if e.GetPath() != path {
			continue
		}
		switch {
		case e.GetChunked():
			return nil, e, fmt.Errorf("%s is a chunked (large) file; suggestions need a text file", path)
		case e.GetSize() > MaxSuggestionFileSize:
			return nil, e, fmt.Errorf("%s is too large for suggestions", path)
		}
		data, err := content.ReadEntry(ctx, s.objects, e)
		if err != nil {
			return nil, e, fmt.Errorf("review: read %s: %w", path, err)
		}
		if textdiff.IsBinary(data) {
			return nil, e, fmt.Errorf("%s is a binary file; suggestions need a text file", path)
		}
		return data, e, nil
	}
	return nil, nil, fmt.Errorf("%s does not exist in snapshot %s: %w", path, shortID(sn.ID), errNoFile)
}

// splitLines splits text into lines keeping their terminators.
func splitLines(text string) []string {
	var out []string
	for text != "" {
		i := strings.IndexByte(text, '\n')
		if i < 0 {
			out = append(out, text)
			break
		}
		out = append(out, text[:i+1])
		text = text[i+1:]
	}
	return out
}

// ApplyLines replaces lines start..end (1-based, inclusive) of text with
// replacement. A missing final newline is added to the replacement unless
// it replaces the last line of a text without one; "\n" becomes "\r\n" when
// the replaced lines use CRLF.
func ApplyLines(text string, start, end int, replacement string) string {
	lines := splitLines(text)
	if start < 1 || end < start || end > len(lines) {
		return text
	}
	if replacement != "" {
		if strings.HasSuffix(lines[start-1], "\r\n") && !strings.Contains(replacement, "\r\n") {
			replacement = strings.ReplaceAll(replacement, "\n", "\r\n")
		}
		eol := "\n"
		if strings.HasSuffix(lines[start-1], "\r\n") {
			eol = "\r\n"
		}
		lastWithoutEOL := end == len(lines) && !strings.HasSuffix(text, "\n")
		switch {
		case lastWithoutEOL:
			replacement = strings.TrimSuffix(strings.TrimSuffix(replacement, "\n"), "\r")
		case !strings.HasSuffix(replacement, "\n"):
			replacement += eol
		}
	}
	var b strings.Builder
	b.Grow(len(text) + len(replacement))
	for _, l := range lines[:start-1] {
		b.WriteString(l)
	}
	b.WriteString(replacement)
	for _, l := range lines[end:] {
		b.WriteString(l)
	}
	return b.String()
}

// ApplySuggestion applies the suggestion of comment id on merge request
// number: it writes a snapshot on the source branch (parent = source tip,
// authored and pushed by the caller) whose file is the suggested version,
// marks the suggestion applied and the comment resolved, and publishes
// branch.pushed and mr.suggestion_applied.
//
// The caller needs domain.ActTreeWrite and the source branch's protection
// must allow a direct push by them (*domain.PushDenied otherwise); the push
// checks installed with SetPushChecks run on the new snapshot (quota
// failures: *QuotaError). When the source tip changed the suggested lines
// since the suggestion was made the result is *OutdatedError; edits
// elsewhere in the file are merged line-wise.
func (s *Service) ApplySuggestion(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, id, message string) (*AppliedSuggestion, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	message = strings.TrimSpace(message)
	if len(message) > MaxMergeMessage || !utf8.ValidString(message) || strings.ContainsRune(message, 0) {
		return nil, fmt.Errorf("message must be at most %d bytes of UTF-8 text: %w", MaxMergeMessage, domain.ErrInvalid)
	}
	if id == "" || strings.ContainsRune(id, 0) {
		return nil, fmt.Errorf("comment id is required: %w", domain.ErrInvalid)
	}

	unlock := s.lockTree(tree.ID)
	defer unlock()

	mr, err := s.getMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	if err := s.CheckPush(ctx, p, tree, mr.SourceBranch); err != nil {
		return nil, err
	}
	if err := requireOpen(mr); err != nil {
		return nil, err
	}
	key := commentKey(tree.ID, number, id)
	c, err := meta.GetDocT[domain.ReviewComment](ctx, s.store, CommentCollection, key)
	if err != nil {
		if errors.Is(err, domain.ErrNotFound) {
			return nil, fmt.Errorf("comment %s on merge request !%d not found: %w", id, number, domain.ErrNotFound)
		}
		return nil, fmt.Errorf("review: get comment: %w", err)
	}
	sg := c.Suggestion
	switch {
	case sg == nil:
		return nil, fmt.Errorf("comment %s has no suggested change: %w", id, domain.ErrInvalid)
	case sg.AppliedSnapshot != "":
		return nil, fmt.Errorf("the suggestion of comment %s was already applied as %s: %w", id, shortID(sg.AppliedSnapshot), domain.ErrConflict)
	case s.objects == nil:
		return nil, fmt.Errorf("suggested changes are not available on this server: %w", domain.ErrConflict)
	}
	tip, err := s.branchTip(ctx, tree.ID, mr.SourceBranch)
	if err != nil {
		return nil, fmt.Errorf("merge request !%d: %w", number, err)
	}

	newData, err := s.suggestedContent(ctx, sg, tip)
	if err != nil {
		return nil, err
	}
	tipSnap, err := s.store.GetSnapshot(ctx, tip)
	if err != nil {
		return nil, fmt.Errorf("review: snapshot %s: %w", shortID(tip), err)
	}
	m, err := s.loadManifest(ctx, tipSnap.ManifestHash)
	if err != nil {
		return nil, err
	}
	blob := hash.Sum(newData)
	nm := &syncpb.ManifestRecord{Entries: make([]*syncpb.ManifestEntry, 0, len(m.GetEntries()))}
	for _, e := range m.GetEntries() {
		if e.GetPath() == sg.Path {
			e = &syncpb.ManifestEntry{Path: e.GetPath(), BlobHash: &syncpb.ContentHash{Value: blob[:]},
				Size: uint64(len(newData)), Executable: e.GetExecutable()}
		}
		nm.Entries = append(nm.Entries, e)
	}
	raw, err := proto.MarshalOptions{Deterministic: true}.Marshal(nm)
	if err != nil {
		return nil, fmt.Errorf("review: encode manifest: %w", err)
	}
	mh := hash.Sum(raw)
	if err := s.objects.Put(ctx, blob, newData); err != nil {
		return nil, fmt.Errorf("review: store suggested blob: %w", err)
	}
	if err := s.objects.Put(ctx, mh, raw); err != nil {
		return nil, fmt.Errorf("review: store manifest: %w", err)
	}
	if message == "" {
		message = fmt.Sprintf("Apply suggestion to %s (!%d, comment #%s)", sg.Path, number, id)
	}
	now := s.now()
	snap := &domain.Snapshot{
		ID: s.newID(), TreeID: tree.ID, BranchName: mr.SourceBranch, Parents: []string{tip},
		ManifestHash: mh, Author: p.Username, Message: message, CreatedAt: now,
		PushedBy: p.AccountID, PushedAt: now,
	}
	plan := &domain.PushPlan{
		Branch: mr.SourceBranch, OldTip: tip, NewTip: snap.ID, NewSnapshots: []*domain.Snapshot{snap},
		ChangedPaths: []string{sg.Path}, NewObjects: []hash.Hash{mh, blob},
	}
	checks := s.pushChecks
	if len(checks) == 0 && s.quota != nil {
		checks = []domain.PushChecker{s.quota}
	}
	for _, chk := range checks {
		if err := chk.CheckPushPlan(ctx, p, tree, plan); err != nil {
			return nil, quotaError(err)
		}
	}
	if err := s.store.PutSnapshot(ctx, snap); err != nil {
		return nil, fmt.Errorf("review: store snapshot: %w", err)
	}
	if err := s.store.CompareAndSwapBranchTip(ctx, tree.ID, mr.SourceBranch, tip, snap.ID); err != nil {
		if errors.Is(err, domain.ErrConflict) {
			return nil, fmt.Errorf("merge request !%d: source branch '%s' moved while applying the suggestion; retry: %w",
				number, mr.SourceBranch, err)
		}
		return nil, fmt.Errorf("review: update branch '%s': %w", mr.SourceBranch, err)
	}

	var out *domain.ReviewComment
	err = meta.UpdateDocT(ctx, s.store, CommentCollection, key, func(cur *domain.ReviewComment) (*domain.ReviewComment, error) {
		if cur == nil {
			return nil, fmt.Errorf("comment %s not found: %w", id, domain.ErrNotFound)
		}
		if cur.Suggestion != nil {
			at := now
			cur.Suggestion.AppliedSnapshot, cur.Suggestion.AppliedBy, cur.Suggestion.AppliedAt = snap.ID, p.Username, &at
		}
		cur.Resolved = true
		cur.UpdatedAt = now
		out = cur
		return cur, nil
	})
	if err != nil {
		return nil, fmt.Errorf("review: branch '%s' updated but comment %s could not be saved: %w", mr.SourceBranch, id, err)
	}

	s.publish(domain.EventBranchPushed, p, tree, map[string]any{
		"branch":        mr.SourceBranch,
		"old_tip":       tip,
		"new_tip":       snap.ID,
		"merge_request": number,
		"suggestion":    id,
	})
	s.publish(domain.EventSuggestionApplied, p, tree, map[string]any{
		"number":      number,
		"comment_id":  id,
		"path":        sg.Path,
		"snapshot_id": snap.ID,
		"applied_by":  p.Username,
	})
	s.record(ctx, "mr.suggestion_apply", p, tree, mr, map[string]string{
		"comment_id": id, "path": sg.Path, "snapshot": snap.ID, "old_tip": tip,
	})
	return &AppliedSuggestion{Comment: out, Snapshot: snap}, nil
}

// suggestedContent returns the content of the suggestion's file after
// applying it on top of the source tip.
func (s *Service) suggestedContent(ctx context.Context, sg *domain.Suggestion, tip string) ([]byte, error) {
	outdated := func(format string, args ...any) error {
		return &OutdatedError{Message: fmt.Sprintf("the suggestion is outdated: "+format, args...)}
	}
	orig, err := s.store.GetSnapshot(ctx, sg.SnapshotID)
	if err != nil {
		return nil, outdated("its snapshot %s is gone", shortID(sg.SnapshotID))
	}
	base, _, err := s.fileAt(ctx, orig, sg.Path)
	if err != nil {
		return nil, outdated("%v", err)
	}
	proposed := ApplyLines(string(base), sg.StartLine, sg.EndLine, sg.Replacement)
	if tip == sg.SnapshotID {
		return []byte(proposed), nil
	}
	tipSnap, err := s.store.GetSnapshot(ctx, tip)
	if err != nil {
		return nil, fmt.Errorf("review: snapshot %s: %w", shortID(tip), err)
	}
	cur, _, err := s.fileAt(ctx, tipSnap, sg.Path)
	if err != nil {
		return nil, outdated("%v", err)
	}
	if string(cur) == string(base) {
		return []byte(proposed), nil
	}
	r := merge3.Text(string(base), string(cur), proposed, merge3.DefaultLabels())
	if !r.Clean() {
		return nil, outdated("the source changed lines %d-%d of %s since the suggestion was made", sg.StartLine, sg.EndLine, sg.Path)
	}
	if r.Merged == string(cur) {
		return nil, fmt.Errorf("the source already contains the suggested change: %w", domain.ErrConflict)
	}
	return []byte(r.Merged), nil
}

// suggestionFor resolves the suggestion of a new comment (nil when none).
func (s *Service) suggestionFor(ctx context.Context, tree *domain.Tree, in *CommentInput) (*domain.Suggestion, error) {
	if in.Suggestion == nil {
		return nil, nil
	}
	sg, err := s.resolveSuggestion(ctx, tree, in.Suggestion, in.SnapshotID)
	if err != nil {
		return nil, err
	}
	in.Path, in.Line, in.SnapshotID = sg.Path, sg.StartLine, sg.SnapshotID
	return sg, nil
}

// suggestionLabel describes a suggestion for audit records.
func suggestionLabel(sg *domain.Suggestion) string {
	if sg == nil {
		return ""
	}
	return sg.Path + ":" + strconv.Itoa(sg.StartLine) + "-" + strconv.Itoa(sg.EndLine)
}
