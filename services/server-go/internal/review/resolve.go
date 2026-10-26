package review

// v0.8 conflict resolution in the browser (docs/ENGINEERING_PLAN.md §16.9,
// specs/server/Merging.md §5): the conflict report of a merge request (base
// / ours / theirs text and a diff3 marker preview per conflicting path) and
// the merge-resolution snapshot written on the source branch.
//
// Both look at the merge the way `wt merge <target>` on the source branch
// would: "ours" is the source branch (where the resolution is written),
// "theirs" the target branch.

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"slices"
	"strings"
	"unicode/utf8"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/content"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/textdiff"
)

// Error codes of the v0.8 conflict resolution errors.
const (
	// CodeResolveOutdated: a branch moved since the conflicts were loaded.
	CodeResolveOutdated = "resolve_outdated"
)

// Bounds of conflict reports and resolutions.
const (
	// MaxConflictText is the largest side whose text a conflict report
	// includes; larger sides are marked too_large.
	MaxConflictText = 1 << 20
	// MaxConflictReportText bounds the text of a whole report; files past
	// it are listed without text (too_large).
	MaxConflictReportText = 8 << 20
	// MaxConflictFiles is the number of conflicting paths a report details
	// (the rest are counted in Total, Truncated is set).
	MaxConflictFiles = 200
	// MaxResolvedFile bounds the content of one resolved file.
	MaxResolvedFile = 8 << 20
	// MaxResolveFiles bounds the resolutions of one request.
	MaxResolveFiles = 1000
)

// Resolution sides (ResolveFile.Take).
const (
	TakeOurs   = "ours"
	TakeTheirs = "theirs"
	TakeBase   = "base"
)

// ConflictSide is one version (base, ours = source, theirs = target) of a
// conflicting path.
type ConflictSide struct {
	Exists     bool   `json:"exists"`
	Size       uint64 `json:"size"`
	Executable bool   `json:"executable"`
	// Hash is the hex hash of the blob (of the chunk list for chunked files).
	Hash string `json:"hash,omitempty"`
	// Text is the content; omitted when the side does not exist, is binary
	// or chunked, too large, or restricted.
	Text string `json:"text,omitempty"`
}

// ConflictFile is one conflicting path of a ConflictReport.
type ConflictFile struct {
	Path string `json:"path"`
	Kind string `json:"kind"` // merge3.Kind*
	// Binary is set when a side is binary or chunked: no text, no markers.
	Binary bool `json:"binary"`
	// TooLarge is set when a side exceeds MaxConflictText (or the report's
	// text budget is spent): no text, no markers.
	TooLarge bool `json:"too_large"`
	// Restricted is set when the caller may not read the path (path
	// policies, license rules): no text, no markers.
	Restricted bool         `json:"restricted"`
	Base       ConflictSide `json:"base"`
	Ours       ConflictSide `json:"ours"`
	Theirs     ConflictSide `json:"theirs"`
	// Markers is the diff3-style preview ("<<<<<<< <source>", "|||||||
	// base", "=======", ">>>>>>> <target>") when ours and theirs are text.
	Markers string `json:"markers,omitempty"`
	// Regions are the conflicting regions of Markers.
	Regions []merge3.Region `json:"regions"`
}

// ConflictReport is the answer of GET .../merge-requests/{n}/conflicts.
type ConflictReport struct {
	Number       int    `json:"number"`
	State        string `json:"state"`
	SourceBranch string `json:"source_branch"`
	TargetBranch string `json:"target_branch"`
	SourceTip    string `json:"source_tip"`
	TargetTip    string `json:"target_tip"`
	MergeBase    string `json:"merge_base"`
	// Resolvable reports whether POST .../resolve can resolve the conflicts
	// now (open, diverged with conflicts, source in this tree); Reason says
	// why not.
	Resolvable bool   `json:"resolvable"`
	Reason     string `json:"reason,omitempty"`
	// Total is the number of conflicting paths; Files details the first
	// MaxConflictFiles of them (Truncated).
	Total     int            `json:"total"`
	Truncated bool           `json:"truncated"`
	Files     []ConflictFile `json:"files"`
}

// ResolveFile resolves one conflicting path: exactly one of Content (the
// resolved text), Take (ours / theirs / base) or Delete.
type ResolveFile struct {
	Path    string  `json:"path"`
	Content *string `json:"content,omitempty"`
	Take    string  `json:"take,omitempty"`
	Delete  bool    `json:"delete,omitempty"`
	// Executable sets the executable bit of Content (default: ours', else
	// theirs').
	Executable *bool `json:"executable,omitempty"`
}

// ResolveInput is the body of POST .../merge-requests/{n}/resolve.
type ResolveInput struct {
	Files []ResolveFile `json:"files"`
	// Message of the resolution snapshot (default "Merge branch '<target>'
	// into <source> (!N, conflicts resolved)").
	Message string `json:"message,omitempty"`
	// SourceTip / TargetTip, when set, must still be the branch tips (the
	// ones the conflicts were loaded at); otherwise *OutdatedResolveError.
	SourceTip string `json:"source_tip,omitempty"`
	TargetTip string `json:"target_tip,omitempty"`
	// AllowMarkers accepts content that still holds conflict markers.
	AllowMarkers bool `json:"allow_markers,omitempty"`
}

// Resolved is the result of ResolveConflicts.
type Resolved struct {
	MergeRequest *domain.MergeRequest `json:"merge_request"`
	Snapshot     *domain.Snapshot     `json:"snapshot"`
	// Paths are the resolved conflicting paths.
	Paths []string `json:"paths"`
}

// OutdatedResolveError refuses a resolution computed against tips that
// moved (code resolve_outdated). It wraps domain.ErrConflict.
type OutdatedResolveError struct{ Message string }

func (e *OutdatedResolveError) Error() string { return e.Message }

// Unwrap makes errors.Is(err, domain.ErrConflict) hold.
func (e *OutdatedResolveError) Unwrap() error { return domain.ErrConflict }

// resolveState is the three-way merge of a merge request seen from its
// source branch.
type resolveState struct {
	mr                   *domain.MergeRequest
	sourceTip, targetTip string
	base                 string
	manifests            [3]*syncpb.ManifestRecord // base, ours (source), theirs (target)
	res                  *merge3.Result
	reason               string // why it cannot be resolved ("" = it can)
}

// Conflicts reports the conflicts of merge request number: per conflicting
// path its base, ours (source branch) and theirs (target branch) versions
// with their text (size-capped; binary and chunked files are marked) and a
// diff3 marker preview. Requires domain.ActTreeRead. Path read restrictions
// are applied by the HTTP layer (ConflictFile.Restricted).
func (s *Service) Conflicts(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*ConflictReport, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	mr, err := s.getMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	st, err := s.resolveState(ctx, tree, mr)
	if err != nil {
		return nil, err
	}
	rep := &ConflictReport{
		Number: mr.Number, State: mr.State, SourceBranch: mr.SourceBranch, TargetBranch: mr.TargetBranch,
		SourceTip: st.sourceTip, TargetTip: st.targetTip, MergeBase: st.base,
		Files: []ConflictFile{},
	}
	if st.res != nil {
		rep.Total = len(st.res.Conflicts)
	}
	rep.Resolvable, rep.Reason = st.reason == "", st.reason
	if st.res == nil || rep.Total == 0 {
		return rep, nil
	}
	bm, om, tm := entriesByPath(st.manifests[0]), entriesByPath(st.manifests[1]), entriesByPath(st.manifests[2])
	labels := merge3.Labels{Ours: mr.SourceBranch, Base: "base", Theirs: mr.TargetBranch}
	budget := MaxConflictReportText
	for i, c := range st.res.Conflicts {
		if i == MaxConflictFiles {
			rep.Truncated = true
			break
		}
		f, err := s.conflictFile(ctx, c, bm[c.Path], om[c.Path], tm[c.Path], labels, &budget)
		if err != nil {
			return nil, err
		}
		rep.Files = append(rep.Files, f)
	}
	return rep, nil
}

// conflictFile describes one conflicting path.
func (s *Service) conflictFile(ctx context.Context, c merge3.Conflict, b, o, t *syncpb.ManifestEntry, labels merge3.Labels, budget *int) (ConflictFile, error) {
	f := ConflictFile{Path: c.Path, Kind: c.Kind, Regions: []merge3.Region{}}
	var texts [3]*string
	for i, e := range []*syncpb.ManifestEntry{b, o, t} {
		side := sideOf(e)
		switch {
		case e == nil:
		case e.GetChunked():
			f.Binary = true
		case e.GetSize() > MaxConflictText || int(e.GetSize()) > *budget:
			f.TooLarge = true
		default:
			data, err := content.ReadEntry(ctx, s.objects, e)
			if err != nil {
				return f, fmt.Errorf("review: read %s: %w", c.Path, err)
			}
			if textdiff.IsBinary(data) {
				f.Binary = true
				break
			}
			*budget -= len(data)
			text := string(data)
			texts[i] = &text
		}
		switch i {
		case 0:
			f.Base = side
		case 1:
			f.Ours = side
		default:
			f.Theirs = side
		}
	}
	if f.Binary || f.TooLarge {
		return f, nil
	}
	for i, side := range []*ConflictSide{&f.Base, &f.Ours, &f.Theirs} {
		if texts[i] != nil {
			side.Text = *texts[i]
		}
	}
	if texts[1] != nil && texts[2] != nil && *texts[1] != *texts[2] {
		base := ""
		if texts[0] != nil {
			base = *texts[0]
		}
		r := merge3.Text(base, *texts[1], *texts[2], labels)
		if len(r.Merged) <= *budget {
			*budget -= len(r.Merged)
			f.Markers = r.Merged
			if r.Conflicts != nil {
				f.Regions = r.Conflicts
			}
		}
	}
	return f, nil
}

func sideOf(e *syncpb.ManifestEntry) ConflictSide {
	if e == nil {
		return ConflictSide{}
	}
	return ConflictSide{
		Exists: true, Size: e.GetSize(), Executable: e.GetExecutable(),
		Hash: fmt.Sprintf("%x", e.GetBlobHash().GetValue()),
	}
}

func entriesByPath(m *syncpb.ManifestRecord) map[string]*syncpb.ManifestEntry {
	out := make(map[string]*syncpb.ManifestEntry, len(m.GetEntries()))
	for _, e := range m.GetEntries() {
		out[e.GetPath()] = e
	}
	return out
}

// resolveState computes the merge of the target into the source branch of
// mr. res is nil when there is nothing to merge (reason says why).
func (s *Service) resolveState(ctx context.Context, tree *domain.Tree, mr *domain.MergeRequest) (*resolveState, error) {
	st := &resolveState{mr: mr}
	if mr.SourceTreeID != "" && mr.SourceTreeID != tree.ID {
		st.reason = "the source branch lives in a fork: resolve the conflicts in the fork (merge the target into the source there)"
		return st, nil
	}
	var err error
	if st.sourceTip, err = s.branchTip(ctx, tree.ID, mr.SourceBranch); err != nil {
		if !errors.Is(err, domain.ErrNotFound) && !errors.Is(err, domain.ErrInvalid) {
			return nil, err
		}
		st.reason = err.Error()
		return st, nil
	}
	if st.targetTip, err = s.branchTip(ctx, tree.ID, mr.TargetBranch); err != nil {
		if !errors.Is(err, domain.ErrNotFound) && !errors.Is(err, domain.ErrInvalid) {
			return nil, err
		}
		st.reason = err.Error()
		return st, nil
	}
	if s.objects == nil {
		st.reason = "conflict resolution is not available on this server"
		return st, nil
	}
	up, err := s.classifyMerge(ctx, mr, st.sourceTip, st.targetTip)
	switch {
	case errors.Is(err, domain.ErrConflict):
	case err != nil:
		return nil, err
	default:
		st.base = st.targetTip
		if up {
			st.base = st.sourceTip
		}
		st.reason = fmt.Sprintf("merge request !%d has no conflicts: the target can be fast-forwarded or already contains the source", mr.Number)
		return st, nil
	}
	base, res, ms, err := s.mergeInto(ctx, st.targetTip, st.sourceTip)
	if err != nil {
		return nil, err
	}
	st.base, st.res, st.manifests = base, res, ms
	switch {
	case len(res.Conflicts) == 0:
		st.reason = fmt.Sprintf("merge request !%d has no conflicts: it can be merged on the server", mr.Number)
	case mr.State != domain.MRStateOpen:
		st.reason = fmt.Sprintf("merge request !%d is %s", mr.Number, mr.State)
	}
	return st, nil
}

// mergeInto three-way merges the manifest of theirsTip into the one of
// oursTip against their merge base and returns the base, the result and the
// three manifests (base, ours, theirs).
func (s *Service) mergeInto(ctx context.Context, theirsTip, oursTip string) (string, *merge3.Result, [3]*syncpb.ManifestRecord, error) {
	var ms [3]*syncpb.ManifestRecord
	base, ok, err := merge3.MergeBase(oursTip, theirsTip, func(id string) ([]string, error) {
		sn, err := s.store.GetSnapshot(ctx, id)
		if err != nil {
			return nil, fmt.Errorf("review: snapshot %s: %w", shortID(id), err)
		}
		return sn.Parents, nil
	})
	if err != nil {
		return "", nil, ms, fmt.Errorf("review: merge base: %w", err)
	}
	if !ok {
		base = ""
	}
	for i, id := range []string{base, oursTip, theirsTip} {
		if ms[i], err = s.manifestOf(ctx, id); err != nil {
			return "", nil, ms, err
		}
	}
	res, err := merge3.Merge(ctx, s.objects, ms[0], ms[1], ms[2])
	if err != nil {
		return "", nil, ms, fmt.Errorf("review: three-way merge: %w", err)
	}
	return base, res, ms, nil
}

// manifestOf loads the manifest of snapshot id ("" = empty).
func (s *Service) manifestOf(ctx context.Context, id string) (*syncpb.ManifestRecord, error) {
	if id == "" {
		return &syncpb.ManifestRecord{}, nil
	}
	sn, err := s.store.GetSnapshot(ctx, id)
	if err != nil {
		return nil, fmt.Errorf("review: snapshot %s: %w", shortID(id), err)
	}
	return s.loadManifest(ctx, sn.ManifestHash)
}

// ResolveConflicts writes a merge-resolution snapshot on the source branch
// of merge request number: the merge of the target into the source with
// every conflicting path resolved by in.Files (parents [source tip, target
// tip], authored and pushed by the caller, no signature). Afterwards the
// merge request can be fast-forwarded (or merged) without conflicts.
//
// The caller needs domain.ActTreeWrite and the source branch's protection
// must allow a direct push by them (*domain.PushDenied otherwise); the push
// checks installed with SetPushChecks (path ACLs, quotas, signing rules)
// run on the new snapshot (quota failures: *QuotaError). Every conflicting
// path must be resolved exactly once and only conflicting paths may be
// (domain.ErrInvalid); content still holding conflict markers is refused
// unless in.AllowMarkers. Tips that moved since in.SourceTip / in.TargetTip
// fail with *OutdatedResolveError. Publishes branch.pushed and
// mr.conflicts_resolved and records audit mr.resolve_conflicts.
func (s *Service) ResolveConflicts(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, in ResolveInput) (*Resolved, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActTreeRead, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}
	if err := validateResolveInput(&in); err != nil {
		return nil, err
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
	st, err := s.resolveState(ctx, tree, mr)
	if err != nil {
		return nil, err
	}
	if (in.SourceTip != "" && in.SourceTip != st.sourceTip) || (in.TargetTip != "" && in.TargetTip != st.targetTip) {
		return nil, &OutdatedResolveError{Message: fmt.Sprintf(
			"merge request !%d: '%s' or '%s' moved since the conflicts were loaded (now %s / %s); reload the conflicts and resolve again",
			number, mr.SourceBranch, mr.TargetBranch, shortID(st.sourceTip), shortID(st.targetTip))}
	}
	if st.reason != "" {
		return nil, fmt.Errorf("%s: %w", st.reason, domain.ErrConflict)
	}

	manifest, blobs, err := applyResolutions(st, in)
	if err != nil {
		return nil, err
	}
	raw, err := proto.MarshalOptions{Deterministic: true}.Marshal(manifest)
	if err != nil {
		return nil, fmt.Errorf("review: encode manifest: %w", err)
	}
	mh := hash.Sum(raw)
	newObjects := []hash.Hash{mh}
	for h := range blobs {
		newObjects = append(newObjects, h)
	}
	slices.SortFunc(newObjects, func(a, b hash.Hash) int { return bytes.Compare(a[:], b[:]) })

	message := in.Message
	if message == "" {
		message = fmt.Sprintf("Merge branch '%s' into %s (!%d, conflicts resolved)", mr.TargetBranch, mr.SourceBranch, number)
	}
	now := s.now()
	snap := &domain.Snapshot{
		ID: s.newID(), TreeID: tree.ID, BranchName: mr.SourceBranch, Parents: []string{st.sourceTip, st.targetTip},
		ManifestHash: mh, Author: p.Username, Message: message, CreatedAt: now,
		PushedBy: p.AccountID, PushedAt: now,
	}
	plan := &domain.PushPlan{
		Branch: mr.SourceBranch, OldTip: st.sourceTip, NewTip: snap.ID, NewSnapshots: []*domain.Snapshot{snap},
		ChangedPaths: changedPaths(st.manifests[1], manifest), NewObjects: newObjects,
	}
	// The objects are stored first (the quota check sizes them through the
	// object store); refused resolutions leave them to the GC.
	for h, data := range blobs {
		if err := s.objects.Put(ctx, h, data); err != nil {
			return nil, fmt.Errorf("review: store resolved blob: %w", err)
		}
	}
	if err := s.objects.Put(ctx, mh, raw); err != nil {
		return nil, fmt.Errorf("review: store manifest: %w", err)
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
	if err := s.store.CompareAndSwapBranchTip(ctx, tree.ID, mr.SourceBranch, st.sourceTip, snap.ID); err != nil {
		if errors.Is(err, domain.ErrConflict) {
			return nil, &OutdatedResolveError{Message: fmt.Sprintf(
				"merge request !%d: source branch '%s' moved while resolving the conflicts; reload and retry", number, mr.SourceBranch)}
		}
		return nil, fmt.Errorf("review: update branch '%s': %w", mr.SourceBranch, err)
	}

	paths := make([]string, len(st.res.Conflicts))
	for i, c := range st.res.Conflicts {
		paths[i] = c.Path
	}
	s.publish(domain.EventBranchPushed, p, tree, map[string]any{
		"branch":        mr.SourceBranch,
		"old_tip":       st.sourceTip,
		"new_tip":       snap.ID,
		"merge_request": number,
		"resolution":    true,
	})
	s.publish(domain.EventConflictsSolved, p, tree, map[string]any{
		"number":        number,
		"source_branch": mr.SourceBranch,
		"target_branch": mr.TargetBranch,
		"old_tip":       st.sourceTip,
		"target_tip":    st.targetTip,
		"snapshot_id":   snap.ID,
		"paths":         paths,
		"resolved_by":   p.Username,
	})
	s.record(ctx, "mr.resolve_conflicts", p, tree, mr, map[string]string{
		"snapshot": snap.ID, "old_tip": st.sourceTip, "target_tip": st.targetTip,
		"paths": strings.Join(paths, ","),
	})
	return &Resolved{MergeRequest: mr, Snapshot: snap, Paths: paths}, nil
}

func validateResolveInput(in *ResolveInput) error {
	in.Message = strings.TrimSpace(in.Message)
	if len(in.Message) > MaxMergeMessage || !utf8.ValidString(in.Message) || strings.ContainsRune(in.Message, 0) {
		return fmt.Errorf("message must be at most %d bytes of UTF-8 text: %w", MaxMergeMessage, domain.ErrInvalid)
	}
	in.SourceTip, in.TargetTip = strings.TrimSpace(in.SourceTip), strings.TrimSpace(in.TargetTip)
	switch {
	case len(in.Files) == 0:
		return fmt.Errorf("files: resolve at least one conflicting path: %w", domain.ErrInvalid)
	case len(in.Files) > MaxResolveFiles:
		return fmt.Errorf("files: at most %d resolutions per request: %w", MaxResolveFiles, domain.ErrInvalid)
	}
	seen := make(map[string]bool, len(in.Files))
	for i := range in.Files {
		f := &in.Files[i]
		if f.Path == "" || len(f.Path) > MaxCommentPathLen || !utf8.ValidString(f.Path) || strings.ContainsRune(f.Path, 0) {
			return fmt.Errorf("files[%d].path is required (at most %d bytes): %w", i, MaxCommentPathLen, domain.ErrInvalid)
		}
		if seen[f.Path] {
			return fmt.Errorf("files: %s is resolved twice: %w", f.Path, domain.ErrInvalid)
		}
		seen[f.Path] = true
		n := 0
		if f.Content != nil {
			n++
		}
		if f.Take != "" {
			n++
		}
		if f.Delete {
			n++
		}
		if n != 1 {
			return fmt.Errorf("files[%d] (%s): set exactly one of content, take or delete: %w", i, f.Path, domain.ErrInvalid)
		}
		switch f.Take {
		case "", TakeOurs, TakeTheirs, TakeBase:
		default:
			return fmt.Errorf("files[%d] (%s): take must be ours, theirs or base: %w", i, f.Path, domain.ErrInvalid)
		}
		if f.Executable != nil && f.Content == nil {
			return fmt.Errorf("files[%d] (%s): executable applies to content only: %w", i, f.Path, domain.ErrInvalid)
		}
		if f.Content != nil {
			switch {
			case len(*f.Content) > MaxResolvedFile:
				return fmt.Errorf("files[%d] (%s): content must be at most %d bytes: %w", i, f.Path, MaxResolvedFile, domain.ErrInvalid)
			case !utf8.ValidString(*f.Content):
				return fmt.Errorf("files[%d] (%s): content must be UTF-8 text: %w", i, f.Path, domain.ErrInvalid)
			case !in.AllowMarkers && HasConflictMarkers(*f.Content):
				return fmt.Errorf("files[%d] (%s): the content still contains conflict markers (pass allow_markers to keep them): %w", i, f.Path, domain.ErrInvalid)
			}
		}
	}
	return nil
}

// HasConflictMarkers reports whether text holds a diff3 conflict block: a
// line starting with "<<<<<<<" followed by a "=======" line and a line
// starting with ">>>>>>>".
func HasConflictMarkers(text string) bool {
	state := 0
	for _, line := range strings.Split(text, "\n") {
		line = strings.TrimSuffix(line, "\r")
		switch {
		case state == 0 && strings.HasPrefix(line, "<<<<<<<"):
			state = 1
		case state == 1 && line == "=======":
			state = 2
		case state == 2 && strings.HasPrefix(line, ">>>>>>>"):
			return true
		}
	}
	return false
}

// applyResolutions builds the manifest of the resolution snapshot: the
// clean merge of the target into the source with every conflicting path
// resolved. It returns the manifest and the new blobs to store.
func applyResolutions(st *resolveState, in ResolveInput) (*syncpb.ManifestRecord, map[hash.Hash][]byte, error) {
	conflicting := make(map[string]bool, len(st.res.Conflicts))
	for _, c := range st.res.Conflicts {
		conflicting[c.Path] = true
	}
	var unknown []string
	given := make(map[string]*ResolveFile, len(in.Files))
	for i := range in.Files {
		f := &in.Files[i]
		if !conflicting[f.Path] {
			unknown = append(unknown, f.Path)
		}
		given[f.Path] = f
	}
	if len(unknown) > 0 {
		return nil, nil, fmt.Errorf("not conflicting in merge request !%d: %s: %w", st.mr.Number, strings.Join(unknown, ", "), domain.ErrInvalid)
	}
	var missing []string
	for _, c := range st.res.Conflicts {
		if given[c.Path] == nil {
			missing = append(missing, c.Path)
		}
	}
	if len(missing) > 0 {
		return nil, nil, fmt.Errorf("unresolved conflicts: %s (every conflicting path must be resolved): %w", strings.Join(missing, ", "), domain.ErrInvalid)
	}

	bm, om, tm := entriesByPath(st.manifests[0]), entriesByPath(st.manifests[1]), entriesByPath(st.manifests[2])
	merged := make(map[string]*syncpb.ManifestEntry, len(st.res.Manifest.GetEntries()))
	for _, e := range st.res.Manifest.GetEntries() {
		merged[e.GetPath()] = e
	}
	blobs := map[hash.Hash][]byte{}
	for h, data := range st.res.Blobs {
		blobs[h] = data
	}
	for path, f := range given {
		delete(merged, path)
		switch {
		case f.Delete:
		case f.Take != "":
			var e *syncpb.ManifestEntry
			switch f.Take {
			case TakeOurs:
				e = om[path]
			case TakeTheirs:
				e = tm[path]
			default:
				e = bm[path]
			}
			if e == nil && f.Take == TakeBase {
				return nil, nil, fmt.Errorf("%s does not exist in the merge base; resolve it with content, ours, theirs or delete: %w", path, domain.ErrInvalid)
			}
			if e != nil { // taking a side that deleted the path deletes it
				merged[path] = proto.Clone(e).(*syncpb.ManifestEntry)
			}
		default:
			data := []byte(*f.Content)
			h := hash.Sum(data)
			exec := false
			switch {
			case f.Executable != nil:
				exec = *f.Executable
			case om[path] != nil:
				exec = om[path].GetExecutable()
			case tm[path] != nil:
				exec = tm[path].GetExecutable()
			}
			blobs[h] = data
			merged[path] = &syncpb.ManifestEntry{Path: path, BlobHash: &syncpb.ContentHash{Value: h[:]}, Size: uint64(len(data)), Executable: exec}
		}
	}
	out := &syncpb.ManifestRecord{Entries: make([]*syncpb.ManifestEntry, 0, len(merged))}
	for _, e := range merged {
		out.Entries = append(out.Entries, e)
	}
	slices.SortFunc(out.Entries, func(a, b *syncpb.ManifestEntry) int { return strings.Compare(a.GetPath(), b.GetPath()) })
	if clash := fileDirectoryClash(out.Entries); clash != "" {
		return nil, nil, fmt.Errorf("the resolution leaves %s both a file and a directory; delete one of them: %w", clash, domain.ErrInvalid)
	}
	// Keep only blobs the manifest references.
	used := make(map[hash.Hash]bool, len(out.Entries))
	for _, e := range out.Entries {
		if h, err := content.EntryHash(e); err == nil {
			used[h] = true
		}
	}
	for h := range blobs {
		if !used[h] {
			delete(blobs, h)
		}
	}
	return out, blobs, nil
}

// fileDirectoryClash returns a file path of sorted entries that is also a
// directory prefix of another path ("" when none).
func fileDirectoryClash(entries []*syncpb.ManifestEntry) string {
	files := make(map[string]bool, len(entries))
	for _, e := range entries {
		files[e.GetPath()] = true
	}
	for _, e := range entries {
		p := e.GetPath()
		for i := strings.IndexByte(p, '/'); i >= 0; {
			if files[p[:i]] {
				return p[:i]
			}
			j := strings.IndexByte(p[i+1:], '/')
			if j < 0 {
				break
			}
			i += 1 + j
		}
	}
	return ""
}

// changedPaths lists the paths whose entries differ between two manifests.
func changedPaths(old, cur *syncpb.ManifestRecord) []string {
	om, cm := entriesByPath(old), entriesByPath(cur)
	var out []string
	for path, e := range cm {
		if o := om[path]; o == nil || !proto.Equal(o, e) {
			out = append(out, path)
		}
	}
	for path := range om {
		if cm[path] == nil {
			out = append(out, path)
		}
	}
	slices.Sort(out)
	return out
}
