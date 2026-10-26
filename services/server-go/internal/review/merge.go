package review

// v0.6 server-side merges (docs/ENGINEERING_PLAN.md §14.1,
// specs/server/Merging.md): merge methods, their selection and protection
// restrictions, three-way merges through internal/merge3, mergeability and
// the merge_conflict / merge_method_not_allowed errors.

import (
	"context"
	"errors"
	"fmt"
	"strconv"
	"strings"
	"sync"
	"unicode/utf8"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/merge3"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// Error codes of the v0.6 merge errors.
const (
	CodeMergeConflict         = "merge_conflict"
	CodeMergeMethodNotAllowed = "merge_method_not_allowed"
)

// Blocker kinds added in v0.6 (reported by Checks when no merge method can
// merge the request).
const (
	BlockerConflicts   = "conflicts"    // the three-way merge has conflicts
	BlockerMergeMethod = "merge_method" // no allowed method can merge
)

// MaxMergeMessage bounds the message of a merge or squash snapshot.
const MaxMergeMessage = 64 << 10

// mergeCacheSize bounds the number of cached three-way outcomes.
const mergeCacheSize = 512

// MergeOptions selects how MergeWith merges a merge request.
type MergeOptions struct {
	// Method is domain.MergeFastForward, MergeCommit or MergeSquash; ""
	// selects the default: the first of fast-forward, merge, squash that is
	// allowed and possible.
	Method string
	// Message is the snapshot message of a merge or squash ("" = default:
	// "Merge branch '<src>' into <tgt> (!N)" / the merge request title).
	// Ignored by fast-forward merges.
	Message string
	// DeleteSourceBranch deletes the source branch after the merge (v0.8
	// §16.9, rebase.go); checked before merging.
	DeleteSourceBranch bool
}

// Conflict is one path a server-side merge cannot resolve. Tree is set for
// linked-group merges ("tenant/tree").
type Conflict struct {
	Tree string `json:"tree,omitempty"`
	Path string `json:"path"`
	Kind string `json:"kind"` // merge3.Kind*
}

// ConflictError refuses a merge whose three-way merge has conflicts (code
// merge_conflict). It wraps domain.ErrConflict.
type ConflictError struct {
	Message   string
	Conflicts []Conflict
}

func (e *ConflictError) Error() string { return e.Message }

// Unwrap makes errors.Is(err, domain.ErrConflict) hold.
func (e *ConflictError) Unwrap() error { return domain.ErrConflict }

// MethodError refuses a merge method that branch protection does not allow
// (code merge_method_not_allowed). It wraps domain.ErrConflict.
type MethodError struct {
	Method  string
	Message string
}

func (e *MethodError) Error() string { return e.Message }

// Unwrap makes errors.Is(err, domain.ErrConflict) hold.
func (e *MethodError) Unwrap() error { return domain.ErrConflict }

// Mergeability reports which merge methods can merge a merge request.
type Mergeability struct {
	// FastForward is true when the target can be fast-forwarded to the
	// source (or already contains it), whatever the protection allows.
	FastForward bool `json:"fast_forward"`
	// UpToDate is true when the target already contains the source.
	UpToDate bool `json:"up_to_date"`
	// Methods lists the methods that are allowed and possible, in
	// preference order; Default is the first of them ("" = none).
	Methods []string `json:"methods"`
	Default string   `json:"default"`
	// Allowed lists the methods branch protection allows.
	Allowed []string `json:"allowed"`
	// MergeBase is the best common ancestor of target and source tips.
	MergeBase string `json:"merge_base,omitempty"`
	// Conflicts of the three-way merge (empty when it is clean or when the
	// target can be fast-forwarded).
	Conflicts []Conflict `json:"conflicts"`
	// Reasons explains, per method, why it is not offered.
	Reasons map[string]string `json:"reasons"`
}

// methodState is the availability of one merge method for an evaluation.
type methodState struct {
	allowed  bool   // branch protection admits the method
	possible bool   // allowed, and it can merge the request now
	kind     string // blocker kind when not possible
	err      error  // the error a merge with this method returns
}

// threeWay is the cached outcome of a three-way merge of (target, source).
type threeWay struct {
	base      string
	conflicts []Conflict
}

// mergeCache is a small FIFO cache of three-way outcomes keyed by (tree,
// source tip, target tip); tips are immutable, so entries never go stale.
type mergeCache struct {
	mu    sync.Mutex
	m     map[string]*threeWay
	order []string
}

func (c *mergeCache) get(key string) (*threeWay, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	o, ok := c.m[key]
	return o, ok
}

func (c *mergeCache) put(key string, o *threeWay) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.m == nil {
		c.m = map[string]*threeWay{}
	}
	if _, ok := c.m[key]; ok {
		c.m[key] = o
		return
	}
	if len(c.order) >= mergeCacheSize {
		delete(c.m, c.order[0])
		c.order = c.order[1:]
	}
	c.m[key] = o
	c.order = append(c.order, key)
}

// Merge merges merge request number with the default method; see MergeWith.
func (s *Service) Merge(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*domain.MergeRequest, error) {
	return s.MergeWith(ctx, p, tree, number, MergeOptions{})
}

// MergeWith merges an open merge request into its target branch.
//
// It requires domain.ActMRMerge and the number of current approvals (for the
// source tip) demanded by the target's effective protection (otherwise
// *domain.PushDenied{Kind: "review_required"}). Outstanding change requests,
// required CI checks and open blocking TODOs fail with a *BlockedError (code
// "merge_blocked"); a member of an open linked group fails with code
// "linked_group" (merge the group instead).
//
// The method (opts.Method, default: the first of fast-forward, merge,
// squash that is allowed and possible) must be allowed by
// BranchProtection.MergeMethods (RequireLinear forbids merge, RequireSigned
// forbids merge and squash); otherwise *MethodError (code
// merge_method_not_allowed). Fast-forward needs the target tip to be an
// ancestor of the source tip (otherwise an error wrapping domain.ErrConflict;
// with RequireLinear the incoming snapshots must not be merges: code
// merge_blocked). Merge and squash write a new snapshot authored and pushed
// by the caller; a conflicting three-way merge fails with *ConflictError
// (code merge_conflict). When the target already contains the source the
// merge request is marked merged without moving the branch. A target that
// moves concurrently yields domain.ErrConflict.
func (s *Service) MergeWith(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int, opts MergeOptions) (*domain.MergeRequest, error) {
	if err := checkTree(tree); err != nil {
		return nil, err
	}
	if err := validateMergeOptions(&opts); err != nil {
		return nil, err
	}
	if err := s.authz.Authorize(ctx, p, domain.ActMRMerge, tree.TenantID, tree); err != nil {
		return nil, err
	}
	if err := requirePrincipal(p); err != nil {
		return nil, err
	}

	unlock := s.lockTree(tree.ID)
	defer unlock()

	mr, err := s.openMR(ctx, tree.ID, number)
	if err != nil {
		return nil, err
	}
	ev, err := s.evaluate(ctx, tree, mr, false)
	if err != nil {
		return nil, err
	}
	if err := ev.mergeError(); err != nil {
		return nil, err
	}
	method, err := ev.selectMethod(opts.Method)
	if err != nil {
		return nil, err
	}
	// v0.8 (§16.9, rebase.go): delete_source_branch is validated up front.
	var extra map[string]string
	if opts.DeleteSourceBranch {
		if err := s.checkDeleteSource(ctx, p, tree, mr); err != nil {
			return nil, err
		}
		extra = map[string]string{"source_branch_deleted": "true"}
	}
	plan, err := s.prepareMerge(ctx, p, tree, mr, ev, method, opts.Message)
	if err != nil {
		return nil, err
	}
	moved, err := s.moveTarget(ctx, tree, mr, ev, plan)
	if err != nil {
		return nil, err
	}
	if err := s.markMerged(ctx, mr, plan); err != nil {
		return nil, err
	}
	s.announceMerge(ctx, p, tree, mr, ev, plan, moved, extra)
	if opts.DeleteSourceBranch {
		s.deleteSource(ctx, p, tree, mr)
	}
	return mr, nil
}

// Mergeability reports which merge methods can merge request number (it
// does not consider approvals, CI or TODOs: see Checks). Requires
// domain.ActTreeRead. The three-way outcome is cached per (source tip,
// target tip).
func (s *Service) Mergeability(ctx context.Context, p *domain.Principal, tree *domain.Tree, number int) (*Mergeability, error) {
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
	ev, err := s.evaluate(ctx, tree, mr, true)
	if err != nil {
		return nil, err
	}
	out := &Mergeability{
		FastForward: ev.sourceTip != "" && !ev.diverged,
		UpToDate:    ev.upToDate,
		Methods:     []string{},
		Allowed:     []string{},
		MergeBase:   ev.base,
		Conflicts:   nonNilConflicts(ev.conflicts),
		Reasons:     map[string]string{},
	}
	for _, m := range AllMergeMethods {
		st := ev.methods[m]
		switch {
		case st == nil:
			out.Reasons[m] = ev.sourceErr.Error()
			continue
		case st.allowed:
			out.Allowed = append(out.Allowed, m)
		}
		switch {
		case mr.State != domain.MRStateOpen:
			out.Reasons[m] = fmt.Sprintf("merge request !%d is %s", mr.Number, mr.State)
		case st.possible:
			out.Methods = append(out.Methods, m)
		case st.err != nil:
			out.Reasons[m] = st.err.Error()
		}
	}
	if len(out.Methods) > 0 {
		out.Default = out.Methods[0]
	}
	return out, nil
}

func validateMergeOptions(opts *MergeOptions) error {
	opts.Method = strings.TrimSpace(opts.Method)
	if opts.Method != "" && !ValidMergeMethod(opts.Method) {
		return fmt.Errorf("merge method %q is not one of %s: %w", opts.Method, strings.Join(AllMergeMethods, ", "), domain.ErrInvalid)
	}
	opts.Message = strings.TrimSpace(opts.Message)
	if len(opts.Message) > MaxMergeMessage || !utf8.ValidString(opts.Message) || strings.ContainsRune(opts.Message, 0) {
		return fmt.Errorf("merge message must be at most %d bytes of UTF-8 text: %w", MaxMergeMessage, domain.ErrInvalid)
	}
	return nil
}

// analyzeMethods classifies the merge (up to date / fast-forward /
// diverged), checks RequireLinear, computes the three-way outcome when the
// branches diverged, and fills ev.methods. When no method can merge it adds
// one blocker explaining why (and remembers the matching error).
func (s *Service) analyzeMethods(ctx context.Context, tree *domain.Tree, ev *evaluation) error {
	mr, eff := ev.mr, ev.eff
	up, err := s.classifyMerge(ctx, mr, ev.sourceTip, ev.targetTip)
	switch {
	case errors.Is(err, domain.ErrConflict):
		ev.diverged, ev.divergedErr = true, err
	case err != nil:
		return err
	default:
		ev.upToDate = up
		if ev.targetTip != "" {
			ev.base = ev.targetTip
			if up {
				ev.base = ev.sourceTip
			}
		}
	}

	// Linear history: the snapshots a fast-forward brings must not be merges.
	if eff.RequireLinear && !ev.upToDate {
		var haves []string
		if ev.targetTip != "" {
			haves = []string{ev.targetTip}
		}
		incoming, err := s.hist.Missing(ctx, ev.sourceTip, haves)
		if err != nil {
			return fmt.Errorf("review: list incoming snapshots: %w", err)
		}
		for _, snap := range incoming {
			if len(snap.Parents) > 1 {
				b := Blocker{Kind: BlockerLinear, Message: fmt.Sprintf(
					"branch '%s' requires linear history but snapshot %s is a merge snapshot", mr.TargetBranch, shortID(snap.ID))}
				ev.linearErr = &BlockedError{
					Code:     CodeMergeBlocked,
					Message:  fmt.Sprintf("merge request !%d cannot be fast-forwarded: %s", mr.Number, b.Message),
					Blockers: []Blocker{b},
				}
				ev.linearMsg = b.Message
				break
			}
		}
	}

	// The three-way outcome when the branches diverged.
	if ev.diverged && s.objects != nil && ev.targetTip != "" {
		tw, err := s.threeWay(ctx, tree, ev.sourceTip, ev.targetTip)
		if err != nil {
			return err
		}
		ev.base, ev.conflicts = tw.base, tw.conflicts
	}

	ev.methods = make(map[string]*methodState, len(AllMergeMethods))
	for _, m := range AllMergeMethods {
		st := &methodState{}
		ev.methods[m] = st
		if msg := methodRefusal(eff, mr, m); msg != "" {
			st.kind, st.err = BlockerMergeMethod, &MethodError{Method: m, Message: msg}
			continue
		}
		st.allowed = true
		switch {
		case ev.upToDate:
			st.possible = true
		case m == MergeRebase: // v0.8 (§16.9, rebase.go)
			if err := s.analyzeRebase(ctx, tree, ev, st); err != nil {
				return err
			}
		case m == domain.MergeFastForward && ev.diverged:
			st.kind, st.err = BlockerDiverged, ev.divergedErr
		case m == domain.MergeFastForward && ev.linearErr != nil:
			st.kind, st.err = BlockerLinear, ev.linearErr
		case m == domain.MergeFastForward:
			st.possible = true
		case s.objects == nil:
			st.kind, st.err = BlockerMergeMethod, fmt.Errorf(
				"merge request !%d: server-side %s merges are not available on this server: %w", mr.Number, m, domain.ErrConflict)
		case ev.targetTip == "":
			st.kind, st.err = BlockerMergeMethod, fmt.Errorf(
				"merge request !%d: target branch '%s' is empty; use fast-forward: %w", mr.Number, mr.TargetBranch, domain.ErrConflict)
		case len(ev.conflicts) > 0:
			st.kind, st.err = BlockerConflicts, s.conflictError(mr, ev.conflicts)
		default:
			st.possible = true
		}
	}

	for _, m := range AllMergeMethods {
		if ev.methods[m].possible {
			return nil
		}
	}
	// No method can merge: report the most useful reason.
	var b Blocker
	switch {
	case len(ev.conflicts) > 0 && (ev.methods[domain.MergeCommit].allowed || ev.methods[domain.MergeSquash].allowed || ev.methods[MergeRebase].allowed):
		b = Blocker{Kind: BlockerConflicts, Message: conflictSummary(ev.conflicts)}
		ev.summaryErr = s.conflictError(mr, ev.conflicts)
	case ev.diverged:
		b = Blocker{Kind: BlockerDiverged, Message: fmt.Sprintf(
			"target '%s' has diverged from '%s'; merge the target into the source and push", mr.TargetBranch, mr.SourceBranch)}
		ev.summaryErr = ev.divergedErr
	case ev.linearErr != nil:
		b = Blocker{Kind: BlockerLinear, Message: ev.linearMsg}
		ev.summaryErr = ev.linearErr
	default:
		reasons := make([]string, 0, len(AllMergeMethods))
		for _, m := range AllMergeMethods {
			if st := ev.methods[m]; st.err != nil {
				reasons = append(reasons, m+": "+st.err.Error())
			}
		}
		msg := fmt.Sprintf("no merge method can merge !%d into '%s' (%s)", mr.Number, mr.TargetBranch, strings.Join(reasons, "; "))
		b = Blocker{Kind: BlockerMergeMethod, Message: msg}
		ev.summaryErr = &MethodError{Message: msg}
	}
	ev.block(b.Kind, "%s", b.Message)
	return nil
}

// methodRefusal returns why branch protection refuses method ("" = allowed).
func methodRefusal(eff Effective, mr *domain.MergeRequest, method string) string {
	switch {
	case !eff.MethodAllowed(method):
		allowed := "none"
		if len(eff.MergeMethods) > 0 {
			allowed = strings.Join(eff.MergeMethods, ", ")
		}
		return fmt.Sprintf("merge method %q is not allowed into '%s' (allowed: %s)", method, mr.TargetBranch, allowed)
	case method == domain.MergeCommit && eff.RequireLinear:
		return fmt.Sprintf("branch '%s' requires linear history: merge snapshots are not allowed (use fast-forward or squash)", mr.TargetBranch)
	case method != domain.MergeFastForward && eff.RequireSigned:
		return fmt.Sprintf("branch '%s' requires signed snapshots, which server-side %s snapshots cannot be", mr.TargetBranch, method)
	}
	return ""
}

// selectMethod returns the method a merge uses: requested when it can merge,
// the default when requested is "", or the matching error.
func (ev *evaluation) selectMethod(requested string) (string, error) {
	if ev.sourceErr != nil {
		return "", ev.sourceErr
	}
	if requested == "" {
		for _, m := range AllMergeMethods {
			if ev.methods[m].possible {
				return m, nil
			}
		}
		return "", ev.summaryErr
	}
	st := ev.methods[requested]
	if st == nil {
		return "", fmt.Errorf("merge method %q is unknown: %w", requested, domain.ErrInvalid)
	}
	if st.possible {
		return requested, nil
	}
	return "", st.err
}

func (s *Service) conflictError(mr *domain.MergeRequest, cs []Conflict) *ConflictError {
	return &ConflictError{
		Message: fmt.Sprintf("merge request !%d cannot be merged automatically: %s; merge '%s' into '%s' locally, resolve and push",
			mr.Number, conflictSummary(cs), mr.TargetBranch, mr.SourceBranch),
		Conflicts: append([]Conflict(nil), cs...),
	}
}

// conflictSummary lists up to ten conflicting paths.
func conflictSummary(cs []Conflict) string {
	const show = 10
	parts := make([]string, 0, min(len(cs), show)+1)
	for i, c := range cs {
		if i == show {
			parts = append(parts, fmt.Sprintf("and %d more", len(cs)-show))
			break
		}
		p := c.Path
		if c.Tree != "" {
			p = c.Tree + ":" + p
		}
		parts = append(parts, fmt.Sprintf("%s (%s)", p, c.Kind))
	}
	noun := "conflicting paths"
	if len(cs) == 1 {
		noun = "conflicting path"
	}
	return fmt.Sprintf("%d %s: %s", len(cs), noun, strings.Join(parts, ", "))
}

func nonNilConflicts(cs []Conflict) []Conflict {
	if cs == nil {
		return []Conflict{}
	}
	return cs
}

// threeWay returns the (cached) merge base and conflicts of merging
// sourceTip into targetTip.
func (s *Service) threeWay(ctx context.Context, tree *domain.Tree, sourceTip, targetTip string) (*threeWay, error) {
	key := tree.ID + "\x00" + sourceTip + "\x00" + targetTip
	if tw, ok := s.merges.get(key); ok {
		return tw, nil
	}
	base, res, err := s.mergeTips(ctx, sourceTip, targetTip)
	if err != nil {
		return nil, err
	}
	tw := &threeWay{base: base, conflicts: toConflicts(res.Conflicts)}
	s.merges.put(key, tw)
	return tw, nil
}

func toConflicts(cs []merge3.Conflict) []Conflict {
	out := make([]Conflict, len(cs))
	for i, c := range cs {
		out[i] = Conflict{Path: c.Path, Kind: c.Kind}
	}
	return out
}

// mergeTips three-way merges the manifest of sourceTip ("theirs") into the
// one of targetTip ("ours") against their merge base (an empty manifest for
// unrelated histories).
func (s *Service) mergeTips(ctx context.Context, sourceTip, targetTip string) (string, *merge3.Result, error) {
	snaps := map[string]*domain.Snapshot{}
	lookups := 0
	getSnap := func(id string) (*domain.Snapshot, error) {
		if sn, ok := snaps[id]; ok {
			return sn, nil
		}
		lookups++
		if lookups%256 == 0 {
			if err := ctx.Err(); err != nil {
				return nil, err
			}
		}
		sn, err := s.store.GetSnapshot(ctx, id)
		if err != nil {
			return nil, fmt.Errorf("review: snapshot %s: %w", shortID(id), err)
		}
		snaps[id] = sn
		return sn, nil
	}
	base, ok, err := merge3.MergeBase(targetTip, sourceTip, func(id string) ([]string, error) {
		sn, err := getSnap(id)
		if err != nil {
			return nil, err
		}
		return sn.Parents, nil
	})
	if err != nil {
		return "", nil, fmt.Errorf("review: merge base: %w", err)
	}
	manifest := func(id string) (*syncpb.ManifestRecord, error) {
		if id == "" {
			return &syncpb.ManifestRecord{}, nil
		}
		sn, err := getSnap(id)
		if err != nil {
			return nil, err
		}
		return s.loadManifest(ctx, sn.ManifestHash)
	}
	if !ok {
		base = ""
	}
	var ms [3]*syncpb.ManifestRecord
	for i, id := range []string{base, targetTip, sourceTip} {
		if ms[i], err = manifest(id); err != nil {
			return "", nil, err
		}
	}
	res, err := merge3.Merge(ctx, s.objects, ms[0], ms[1], ms[2])
	if err != nil {
		return "", nil, fmt.Errorf("review: three-way merge: %w", err)
	}
	return base, res, nil
}

func (s *Service) loadManifest(ctx context.Context, h hash.Hash) (*syncpb.ManifestRecord, error) {
	data, err := s.objects.Get(ctx, h)
	if err != nil {
		return nil, fmt.Errorf("review: read manifest %s: %w", h.Hex(), err)
	}
	m := &syncpb.ManifestRecord{}
	if err := proto.Unmarshal(data, m); err != nil {
		return nil, fmt.Errorf("review: decode manifest %s: %w", h.Hex(), err)
	}
	return m, nil
}

// mergePlan is a prepared merge: the method and the target tip after it.
type mergePlan struct {
	method string
	// newTip is the target tip after the merge (the source tip for
	// fast-forwards, the new snapshot for merge/squash, unchanged when the
	// target is up to date).
	newTip string
	// merged is recorded as MergeRequest.MergedSnapshot.
	merged string
}

// prepareMerge writes the objects and snapshot a merge or squash needs
// (nothing for fast-forwards) and returns the plan. The target branch is
// not moved.
func (s *Service) prepareMerge(ctx context.Context, p *domain.Principal, tree *domain.Tree, mr *domain.MergeRequest, ev *evaluation, method, message string) (*mergePlan, error) {
	switch {
	case ev.upToDate:
		return &mergePlan{method: method, newTip: ev.targetTip, merged: ev.sourceTip}, nil
	case method == domain.MergeFastForward:
		return &mergePlan{method: method, newTip: ev.sourceTip, merged: ev.sourceTip}, nil
	case method == MergeRebase: // v0.8 (§16.9, rebase.go)
		return s.prepareRebase(ctx, p, tree, mr, ev)
	}

	var manifestHash hash.Hash
	if !ev.diverged {
		// The target is an ancestor of the source: the merge result is the
		// source tree.
		src, err := s.store.GetSnapshot(ctx, ev.sourceTip)
		if err != nil {
			return nil, fmt.Errorf("review: snapshot %s: %w", shortID(ev.sourceTip), err)
		}
		manifestHash = src.ManifestHash
	} else {
		_, res, err := s.mergeTips(ctx, ev.sourceTip, ev.targetTip)
		if err != nil {
			return nil, err
		}
		if !res.Clean() {
			return nil, s.conflictError(mr, toConflicts(res.Conflicts))
		}
		for h, data := range res.Blobs {
			if err := s.objects.Put(ctx, h, data); err != nil {
				return nil, fmt.Errorf("review: store merged blob: %w", err)
			}
		}
		data, err := proto.MarshalOptions{Deterministic: true}.Marshal(res.Manifest)
		if err != nil {
			return nil, fmt.Errorf("review: encode merged manifest: %w", err)
		}
		manifestHash = hash.Sum(data)
		if err := s.objects.Put(ctx, manifestHash, data); err != nil {
			return nil, fmt.Errorf("review: store merged manifest: %w", err)
		}
	}

	parents := []string{ev.targetTip, ev.sourceTip}
	if method == domain.MergeSquash {
		parents = []string{ev.targetTip}
	}
	if message == "" {
		message = defaultMergeMessage(mr, method)
	}
	now := s.now()
	snap := &domain.Snapshot{
		ID:           s.newID(),
		TreeID:       tree.ID,
		BranchName:   mr.TargetBranch,
		Parents:      parents,
		ManifestHash: manifestHash,
		Author:       p.Username,
		Message:      message,
		CreatedAt:    now,
		PushedBy:     p.AccountID,
		PushedAt:     now,
	}
	// v0.7 (§15.7): the new objects must fit the tenant's storage quota
	// (sized against both tips, which already reference the rest).
	if err := s.checkMergeQuota(ctx, p, tree, mr.TargetBranch, ev.targetTip, snap, []string{ev.targetTip, ev.sourceTip}); err != nil {
		return nil, err
	}
	if err := s.store.PutSnapshot(ctx, snap); err != nil {
		return nil, fmt.Errorf("review: store %s snapshot: %w", method, err)
	}
	return &mergePlan{method: method, newTip: snap.ID, merged: snap.ID}, nil
}

// defaultMergeMessage is the snapshot message of a merge or squash.
func defaultMergeMessage(mr *domain.MergeRequest, method string) string {
	if method == domain.MergeSquash {
		return mr.Title
	}
	return fmt.Sprintf("Merge branch '%s' into %s (!%d)", mr.SourceBranch, mr.TargetBranch, mr.Number)
}

// mergeAuditDetails returns the audit details of a merge.
func mergeAuditDetails(mr *domain.MergeRequest, ev *evaluation, plan *mergePlan, moved bool) map[string]string {
	return map[string]string{
		"old_tip":    ev.targetTip,
		"new_tip":    plan.newTip,
		"method":     plan.method,
		"approvals":  strconv.Itoa(countApprovals(mr, ev.sourceTip)),
		"up_to_date": strconv.FormatBool(!moved),
	}
}
