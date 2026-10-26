package scan

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"sync"

	"google.golang.org/protobuf/proto"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
)

// file is one blob to scan.
type file struct {
	path     string
	blob     hash.Hash
	size     int64
	snapshot string // the snapshot that introduced it
}

// collected is the scan input of a push or snapshot.
type collected struct {
	files     []file
	bytes     int64
	skipped   int  // over the size cap, chunked or excluded
	truncated bool // MaxPushScanBytes reached
}

// manifests caches decoded manifests by hash.
type manifests struct {
	s     *Service
	cache map[hash.Hash]*syncpb.ManifestRecord
}

func (m *manifests) get(ctx context.Context, h hash.Hash) (*syncpb.ManifestRecord, error) {
	if r, ok := m.cache[h]; ok {
		return r, nil
	}
	raw, err := m.s.d.Objects.Get(ctx, h)
	if err != nil {
		return nil, fmt.Errorf("scan: manifest %s: %w", h, err)
	}
	var r syncpb.ManifestRecord
	if err := proto.Unmarshal(raw, &r); err != nil {
		return nil, fmt.Errorf("scan: decode manifest %s: %w", h, err)
	}
	if m.cache == nil {
		m.cache = map[hash.Hash]*syncpb.ManifestRecord{}
	}
	m.cache[h] = &r
	return &r, nil
}

// add appends the entries of m that differ from base (nil = everything),
// deduplicated by (path, blob).
func (c *collected) add(st *Settings, m, base *syncpb.ManifestRecord, snapshot string, seen map[string]bool) {
	old := map[string]string{}
	if base != nil {
		for _, e := range base.GetEntries() {
			old[e.GetPath()] = string(e.GetBlobHash().GetValue())
		}
	}
	maxFile := st.effectiveMaxFile()
	for _, e := range m.GetEntries() {
		if c.truncated {
			return
		}
		p, blob := e.GetPath(), e.GetBlobHash().GetValue()
		if old[p] == string(blob) || len(blob) != hash.Size {
			continue
		}
		key := p + "\x00" + string(blob)
		if seen[key] {
			continue
		}
		seen[key] = true
		size := int64(e.GetSize())
		if e.GetChunked() || size > maxFile || st.excluded(p) {
			c.skipped++
			continue
		}
		if c.bytes+size > MaxPushScanBytes {
			c.truncated = true
			return
		}
		c.bytes += size
		c.files = append(c.files, file{path: p, blob: hash.Hash(blob), size: size, snapshot: snapshot})
	}
}

// collectPush gathers the files the new snapshots of plan add or change.
func (s *Service) collectPush(ctx context.Context, plan *domain.PushPlan, st *Settings) (*collected, error) {
	ms := &manifests{s: s}
	c := &collected{}
	seen := map[string]bool{}
	byID := make(map[string]*domain.Snapshot, len(plan.NewSnapshots))
	for _, snap := range plan.NewSnapshots {
		byID[snap.ID] = snap
	}
	manifestOf := func(id string) (*syncpb.ManifestRecord, error) {
		if snap, ok := byID[id]; ok {
			return ms.get(ctx, snap.ManifestHash)
		}
		snap, err := s.d.Store.GetSnapshot(ctx, id)
		if err != nil {
			return nil, err
		}
		return ms.get(ctx, snap.ManifestHash)
	}
	if len(plan.NewSnapshots) == 0 {
		// A branch created at a stored snapshot adds no content.
		return c, nil
	}
	if len(plan.NewSnapshots) > maxDiffSnapshots {
		// Large imports: only what the tip adds relative to the old tip.
		tip, err := manifestOf(plan.NewTip)
		if err != nil {
			return nil, err
		}
		var base *syncpb.ManifestRecord
		if plan.OldTip != "" {
			if base, err = manifestOf(plan.OldTip); err != nil {
				return nil, err
			}
		}
		c.add(st, tip, base, plan.NewTip, seen)
		return c, nil
	}
	// Each new snapshot against its first parent, newest first (a blob is
	// reported with the snapshot that introduced it).
	order := make([]*domain.Snapshot, 0, len(plan.NewSnapshots))
	for i := len(plan.NewSnapshots) - 1; i >= 0; i-- {
		order = append(order, plan.NewSnapshots[i])
	}
	for _, snap := range order {
		m, err := ms.get(ctx, snap.ManifestHash)
		if err != nil {
			return nil, err
		}
		var base *syncpb.ManifestRecord
		if len(snap.Parents) > 0 {
			b, err := manifestOf(snap.Parents[0])
			if err != nil && !errors.Is(err, domain.ErrNotFound) {
				return nil, err
			}
			base = b
		}
		c.add(st, m, base, snap.ID, seen)
		if c.truncated {
			break
		}
	}
	return c, nil
}

// hit is a match located in a snapshot.
type hit struct {
	Match
	snapshot string
}

// scanFiles reads and scans the files concurrently. Unreadable blobs are
// skipped (the push pipeline has verified their presence).
func (s *Service) scanFiles(ctx context.Context, files []file) []hit {
	results := make([][]hit, len(files))
	workers := min(max(runtime.GOMAXPROCS(0), 1), 8, max(len(files), 1))
	var wg sync.WaitGroup
	next := make(chan int)
	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := range next {
				f := files[i]
				data, err := s.d.Objects.Get(ctx, f.blob)
				if err != nil {
					s.d.Logger.WarnContext(ctx, "scan: reading a blob failed", "path", f.path, "err", err)
					continue
				}
				for _, m := range ScanBytes(f.path, data) {
					results[i] = append(results[i], hit{Match: m, snapshot: f.snapshot})
				}
			}
		}()
	}
	for i := range files {
		if ctx.Err() != nil {
			break
		}
		next <- i
	}
	close(next)
	wg.Wait()
	var out []hit
	for _, r := range results {
		out = append(out, r...)
	}
	sort.SliceStable(out, func(i, j int) bool {
		if out[i].Path != out[j].Path {
			return out[i].Path < out[j].Path
		}
		return out[i].Line < out[j].Line
	})
	return out
}

// dedupe keeps the first hit per (fingerprint, path) and drops allowlisted
// fingerprints; it returns the kept hits and how many were allowed.
func dedupe(hits []hit, allow map[string]bool) ([]hit, int) {
	seen := map[string]bool{}
	var out []hit
	allowed := 0
	for _, h := range hits {
		if allow[h.Fingerprint] {
			allowed++
			continue
		}
		k := h.Fingerprint + "\x00" + h.Path
		if seen[k] {
			continue
		}
		seen[k] = true
		out = append(out, h)
	}
	return out, allowed
}

// CheckPushPlan implements domain.PushChecker (see the package
// documentation). In warn mode a scanner failure never fails the push; in
// block mode it does (fail closed).
func (s *Service) CheckPushPlan(ctx context.Context, p *domain.Principal, tree *domain.Tree, plan *domain.PushPlan) error {
	if s.d.Objects == nil || s.d.Store == nil || tree == nil || plan == nil {
		return nil
	}
	st, err := s.GetSettings(ctx, tree.ID)
	if err != nil {
		s.d.Logger.WarnContext(ctx, "scan: reading settings failed; using the defaults", "tree", tree.ID, "err", err)
		st = &Settings{TreeID: tree.ID, Mode: ModeWarn}
	}
	if st.Mode == ModeOff {
		return nil
	}
	c, err := s.collectPush(ctx, plan, st)
	if err != nil {
		if st.Mode == ModeBlock {
			return fmt.Errorf("secret scanning: %w", err)
		}
		s.d.Logger.WarnContext(ctx, "scan: collecting pushed files failed; push not scanned", "tree", tree.ID, "err", err)
		return nil
	}
	if c.truncated {
		s.d.Logger.WarnContext(ctx, "scan: push larger than the scan budget; the rest is not scanned",
			"tree", tree.ID, "branch", plan.Branch, "budget_bytes", MaxPushScanBytes)
	}
	hits, allowed := dedupe(s.scanFiles(ctx, c.files), s.allowSet(ctx, tree.ID))
	if len(hits) == 0 {
		return nil
	}
	action := ActionWarned
	if st.Mode == ModeBlock {
		action = ActionBlocked
		if s.canBypass(ctx, p, tree) {
			action = ActionBypassed
		}
	}
	s.record(ctx, p, tree, plan.Branch, plan.NewTip, st.Mode, action, hits, allowed, c.truncated)
	if action == ActionBlocked {
		return denial(hits)
	}
	return nil
}

// canBypass reports whether p holds branch:bypass_protection on tree.
func (s *Service) canBypass(ctx context.Context, p *domain.Principal, tree *domain.Tree) bool {
	if s.d.Authz == nil || p == nil {
		return false
	}
	return s.d.Authz.Authorize(ctx, p, domain.ActProtectionBypass, tree.TenantID, tree) == nil
}

// denial is the rejection of a blocked push.
func denial(hits []hit) *domain.PushDenied {
	var b strings.Builder
	fmt.Fprintf(&b, "secret scanning blocked the push: %d possible secret(s)", len(hits))
	for i, h := range hits {
		if i == maxListedInDenial {
			fmt.Fprintf(&b, "\n  ... and %d more (wt compliance findings)", len(hits)-i)
			break
		}
		fmt.Fprintf(&b, "\n  %s:%d [%s] %s (fingerprint %s)", h.Path, h.Line, h.Rule, h.Excerpt, h.Fingerprint)
	}
	b.WriteString("\nremove the secret from every new snapshot and push again; a false positive can be allowed by a tree admin with `wt compliance allow FINGERPRINT --reason ...`")
	return &domain.PushDenied{Kind: KindSecretDetected, Reason: b.String()}
}

// record stores the findings of a push, publishes secret.detected and
// audits it.
func (s *Service) record(ctx context.Context, p *domain.Principal, tree *domain.Tree, branch, tip, mode, action string, hits []hit, allowed int, truncated bool) {
	ctx = context.WithoutCancel(ctx)
	now := s.now()
	actor := ""
	if p != nil {
		actor = p.Username
	}
	stored := make([]Finding, 0, min(len(hits), maxRecorded))
	for i, h := range hits {
		if i == maxRecorded {
			break
		}
		id := findingID(h.Fingerprint, h.Path)
		var out Finding
		err := s.d.Store.UpdateDoc(ctx, CollFindings, domain.DocKey(tree.ID, id), func(raw []byte) ([]byte, error) {
			out = Finding{}
			if raw != nil {
				if err := json.Unmarshal(raw, &out); err != nil {
					return nil, err
				}
			}
			if out.ID == "" {
				out = Finding{ID: id, TreeID: tree.ID, Fingerprint: h.Fingerprint, Path: h.Path, FirstSeenAt: now, Status: StatusOpen}
			}
			if out.Status == StatusResolved && action != ActionBlocked {
				// The secret landed again: reopen.
				out.Status, out.Resolution, out.ResolvedBy, out.ResolvedAt = StatusOpen, "", "", nil
			}
			out.Line, out.Rule, out.Excerpt = h.Line, h.Rule, h.Excerpt
			out.Branch, out.SnapshotID, out.Action, out.PushedBy = branch, h.snapshot, action, actor
			out.Count++
			out.LastSeenAt = now
			return json.Marshal(out)
		})
		if err != nil {
			s.d.Logger.WarnContext(ctx, "scan: storing a finding failed", "tree", tree.ID, "err", err)
			continue
		}
		stored = append(stored, out)
	}

	type eventFinding struct {
		ID          string `json:"id"`
		Path        string `json:"path"`
		Line        int    `json:"line"`
		Rule        string `json:"rule"`
		Excerpt     string `json:"excerpt"`
		Fingerprint string `json:"fingerprint"`
	}
	evs := make([]eventFinding, 0, min(len(stored), maxEventFindings))
	paths := make([]string, 0, len(stored))
	ruleSet := map[string]bool{}
	for i, f := range stored {
		if i < maxEventFindings {
			evs = append(evs, eventFinding{ID: f.ID, Path: f.Path, Line: f.Line, Rule: f.Rule, Excerpt: f.Excerpt, Fingerprint: f.Fingerprint})
		}
		if i < 10 {
			paths = append(paths, fmt.Sprintf("%s:%d", f.Path, f.Line))
		}
		ruleSet[f.Rule] = true
	}
	ruleIDs := make([]string, 0, len(ruleSet))
	for r := range ruleSet {
		ruleIDs = append(ruleIDs, r)
	}
	sort.Strings(ruleIDs)
	if s.d.Bus != nil {
		s.d.Bus.Publish(domain.Event{
			Type:     domain.EventSecretDetected,
			TenantID: tree.TenantID,
			TreeID:   tree.ID,
			Actor:    actor,
			At:       now,
			Payload: map[string]any{
				"tree":      tree.Name,
				"branch":    branch,
				"new_tip":   tip,
				"mode":      mode,
				"action":    action,
				"count":     len(hits),
				"allowed":   allowed,
				"truncated": truncated,
				"findings":  evs,
			},
		})
	}
	if s.d.Audit != nil {
		e := domain.AuditEvent{
			TenantID:     tree.TenantID,
			Actor:        "system",
			Action:       "secret.detected",
			ResourceType: "branch",
			ResourceID:   tree.ID + "/" + branch,
			At:           now,
			Details: map[string]string{
				"tree":    tree.Name,
				"branch":  branch,
				"new_tip": tip,
				"mode":    mode,
				"action":  action,
				"count":   strconv.Itoa(len(hits)),
				"rules":   strings.Join(ruleIDs, ","),
				"paths":   strings.Join(paths, ","),
			},
		}
		if p != nil {
			e.ActorID, e.Actor = p.AccountID, p.Username
		}
		s.d.Audit.Record(ctx, e)
	}
}
