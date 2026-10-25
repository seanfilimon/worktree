package httpapi

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"net/http"
	"path"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/proto/syncpb"
	"github.com/seanfilimon/worktree/services/server-go/internal/staged"
	"github.com/seanfilimon/worktree/services/server-go/internal/textdiff"
)

// Limits of the insight endpoints.
const (
	// maxDiffFileSize is the largest file side that gets a patch; larger
	// files are reported with too_large.
	maxDiffFileSize = 1 << 20
	// maxCompareBlobBytes bounds the blob bytes one compare request loads
	// for patches; files beyond the budget are reported with too_large.
	maxCompareBlobBytes = 32 << 20
	// maxCompareSnapshots caps CompareResult.snapshots.
	maxCompareSnapshots = 250
	// maxDiffContext caps the context= parameter.
	maxDiffContext = 1000

	defaultFileHistoryLimit = 50
	maxFileHistoryLimit     = 1000
	// maxFileHistoryScan bounds how many snapshots of a branch the file
	// history endpoint inspects.
	maxFileHistoryScan = 10000

	defaultStatsDays = 30
	maxStatsDays     = 365
	// maxStatsLanguages is the number of languages listed before the rest
	// is folded into "Other".
	maxStatsLanguages = 10

	defaultRecentEvents = 50
	maxRecentEvents     = 200

	// minRevPrefix is the shortest snapshot id prefix accepted as a REV.
	minRevPrefix = 4
)

// File change statuses (FileChangeStatus in apps/admin/src/api/types.ts).
const (
	changeAdded    = "added"
	changeModified = "modified"
	changeDeleted  = "deleted"
)

// registerInsightRoutes registers the read-only insight endpoints of
// ENGINEERING_PLAN §11: compare, file history, tree stats, staged summary
// and recent events. tree is the "/api/v1/trees/{tenant}/{tree}" prefix.
func (a *api) registerInsightRoutes(tree string) {
	a.handle("GET "+tree+"/compare", a.handleCompare)
	a.handle("GET "+tree+"/history", a.handleFileHistory)
	a.handle("GET "+tree+"/stats", a.handleTreeStats)
	a.handle("GET "+tree+"/staged/summary", a.handleStagedSummary)
	a.handle("GET /api/v1/events/recent", a.handleRecentEvents)
}

// ---- revisions ----

// snapshotForRev resolves a REV of tree to a snapshot. In order of
// precedence a REV is: a full snapshot id of the tree, a branch name, a tag
// name, or a unique prefix (at least minRevPrefix characters) of the id of
// a snapshot reachable from the tree's branches and tags. Branch and tag
// names take precedence over id prefixes so that a short ref name such as
// "beef" can never be shadowed by a snapshot id.
//
// Unknown revisions are 404 not_found, ambiguous prefixes 400 bad_request.
func (a *api) snapshotForRev(ctx context.Context, tree *domain.Tree, rev string) (*domain.Snapshot, error) {
	if rev == "" {
		return nil, badRequestf("revision must not be empty")
	}
	s, err := a.d.Store.GetSnapshot(ctx, rev)
	switch {
	case err == nil && s.TreeID == tree.ID:
		return s, nil
	case err != nil && !errors.Is(err, domain.ErrNotFound):
		return nil, err
	}

	b, err := a.d.Store.GetBranch(ctx, tree.ID, rev)
	switch {
	case err == nil:
		if b.Tip == "" {
			return nil, notFoundf("branch %q has no snapshots", rev)
		}
		return a.treeSnapshot(ctx, tree, b.Tip)
	case !errors.Is(err, domain.ErrNotFound):
		return nil, err
	}

	tags, err := a.d.Store.ListTags(ctx, tree.ID)
	if err != nil {
		return nil, err
	}
	for _, t := range tags {
		if t.Name == rev {
			return a.treeSnapshot(ctx, tree, t.Target)
		}
	}

	if len(rev) >= minRevPrefix {
		return a.snapshotByPrefix(ctx, tree, rev)
	}
	return nil, notFoundf("revision %q not found", rev)
}

// snapshotByPrefix finds the unique snapshot reachable from the tree's
// branch tips and tag targets whose id starts with prefix.
func (a *api) snapshotByPrefix(ctx context.Context, tree *domain.Tree, prefix string) (*domain.Snapshot, error) {
	branches, err := a.d.Store.ListBranches(ctx, tree.ID)
	if err != nil {
		return nil, err
	}
	tags, err := a.d.Store.ListTags(ctx, tree.ID)
	if err != nil {
		return nil, err
	}
	var tips []string
	for _, b := range branches {
		if b.Tip != "" {
			tips = append(tips, b.Tip)
		}
	}
	for _, t := range tags {
		if t.Target != "" {
			tips = append(tips, t.Target)
		}
	}
	seen := map[string]bool{}
	var match *domain.Snapshot
	for _, tip := range tips {
		if seen[tip] {
			continue // already walked as an ancestor of an earlier tip
		}
		log, err := a.d.History.Log(ctx, tip, 0)
		if err != nil {
			return nil, err
		}
		for i := range log {
			s := &log[i]
			if seen[s.ID] {
				continue
			}
			seen[s.ID] = true
			if s.TreeID != tree.ID || !strings.HasPrefix(s.ID, prefix) {
				continue
			}
			if match != nil {
				return nil, badRequestf("revision %q is ambiguous", prefix)
			}
			match = s
		}
	}
	if match == nil {
		return nil, notFoundf("revision %q not found", prefix)
	}
	return match, nil
}

// ---- manifests ----

// manifestIndex maps the paths of m to their entries.
func manifestIndex(m *syncpb.ManifestRecord) map[string]*syncpb.ManifestEntry {
	idx := make(map[string]*syncpb.ManifestEntry, len(m.GetEntries()))
	for _, e := range m.GetEntries() {
		idx[e.GetPath()] = e
	}
	return idx
}

// manifestEntry returns the entry for p in m, or nil.
func manifestEntry(m *syncpb.ManifestRecord, p string) *syncpb.ManifestEntry {
	entries := m.GetEntries()
	i := sort.Search(len(entries), func(i int) bool { return entries[i].GetPath() >= p })
	if i < len(entries) && entries[i].GetPath() == p {
		return entries[i]
	}
	// Manifests are specified as sorted, but tolerate unsorted ones.
	for _, e := range entries {
		if e.GetPath() == p {
			return e
		}
	}
	return nil
}

// validTreePath reports whether p is a clean, relative, '/'-separated path.
func validTreePath(p string) bool {
	return p != "" && !strings.HasPrefix(p, "/") && path.Clean(p) == p && p != "." && !strings.HasPrefix(p, "../") && p != ".."
}

// ---- compare ----

// fileDiff is FileDiff of apps/admin/src/api/types.ts.
type fileDiff struct {
	Path      string `json:"path"`
	Status    string `json:"status"`
	OldHash   string `json:"old_hash,omitempty"`
	NewHash   string `json:"new_hash,omitempty"`
	OldSize   uint64 `json:"old_size"`
	NewSize   uint64 `json:"new_size"`
	Binary    bool   `json:"binary"`
	TooLarge  bool   `json:"too_large"`
	Additions int    `json:"additions"`
	Deletions int    `json:"deletions"`
	Patch     string `json:"patch,omitempty"`
}

type compareStats struct {
	Files     int `json:"files"`
	Additions int `json:"additions"`
	Deletions int `json:"deletions"`
}

// compareResult is CompareResult of apps/admin/src/api/types.ts.
type compareResult struct {
	From      *domain.Snapshot  `json:"from"`
	To        domain.Snapshot   `json:"to"`
	MergeBase string            `json:"merge_base,omitempty"`
	Snapshots []domain.Snapshot `json:"snapshots"`
	Files     []fileDiff        `json:"files"`
	Stats     compareStats      `json:"stats"`
}

// handleCompare answers GET {tree}/compare?to=REV&from=REV&context=3.
//
// from defaults to the first parent of to; a root snapshot is compared with
// nothing (from null, every file added). Files are sorted by path. Patches
// are produced for text files whose sides are at most 1 MiB, within a
// per-request budget of loaded blob bytes; beyond either limit a file is
// reported with too_large and no patch.
//
// Paths the caller may not read under the tree's license rules are still
// listed (path, status, hashes, sizes) but without patch or line counts, in
// the same way the blob endpoint withholds their content.
func (a *api) handleCompare(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	q := r.URL.Query()
	contextLines, err := queryNonNegInt(r, "context", textdiff.DefaultContext, maxDiffContext)
	if err != nil {
		return err
	}
	toRev := q.Get("to")
	if toRev == "" {
		return badRequestf("to is required")
	}
	to, err := a.snapshotForRev(ctx, tree, toRev)
	if err != nil {
		return err
	}
	var from *domain.Snapshot
	if fromRev := q.Get("from"); fromRev != "" {
		if from, err = a.snapshotForRev(ctx, tree, fromRev); err != nil {
			return err
		}
	} else if len(to.Parents) > 0 {
		if from, err = a.treeSnapshot(ctx, tree, to.Parents[0]); err != nil {
			return err
		}
	}

	res := compareResult{To: *to, Snapshots: []domain.Snapshot{}, Files: []fileDiff{}}
	normalizeSnapshot(&res.To)
	if from != nil {
		normalizeSnapshot(from)
		res.From = from
	}

	// Snapshots in to but not in from, and the merge base when diverged.
	var haves []string
	if from != nil {
		haves = []string{from.ID}
	}
	missing, err := a.d.History.Missing(ctx, to.ID, haves)
	if err != nil {
		return err
	}
	for i := len(missing) - 1; i >= 0 && len(res.Snapshots) < maxCompareSnapshots; i-- {
		s := missing[i]
		normalizeSnapshot(&s)
		res.Snapshots = append(res.Snapshots, s)
	}
	if from != nil && from.ID != to.ID {
		isAnc, err := a.d.History.IsAncestor(ctx, from.ID, to.ID)
		if err != nil {
			return err
		}
		if !isAnc {
			if res.MergeBase, err = a.mergeBase(ctx, to, missing); err != nil {
				return err
			}
		}
	}

	// File differences.
	toM, err := a.loadManifest(ctx, to)
	if err != nil {
		return err
	}
	newIdx := manifestIndex(toM)
	oldIdx := map[string]*syncpb.ManifestEntry{}
	if from != nil {
		fromM, err := a.loadManifest(ctx, from)
		if err != nil {
			return err
		}
		oldIdx = manifestIndex(fromM)
	}
	files, err := changedFiles(oldIdx, newIdx)
	if err != nil {
		return err
	}
	if len(files) > 0 {
		readable, err := a.readablePaths(ctx, principal(r), tree, files)
		if err != nil {
			return err
		}
		budget := int64(maxCompareBlobBytes)
		for i := range files {
			fd := &files[i]
			if !readable[fd.Path] {
				continue
			}
			if err := a.fillPatch(ctx, fd, oldIdx[fd.Path], newIdx[fd.Path], contextLines, &budget); err != nil {
				return err
			}
		}
	}
	res.Files = files
	res.Stats.Files = len(files)
	for _, f := range files {
		res.Stats.Additions += f.Additions
		res.Stats.Deletions += f.Deletions
	}
	writeJSON(w, http.StatusOK, res)
	return nil
}

// queryNonNegInt parses an optional non-negative integer query parameter,
// clamping it to max.
func queryNonNegInt(r *http.Request, name string, def, max int) (int, error) {
	s := r.URL.Query().Get(name)
	if s == "" {
		return def, nil
	}
	n, err := strconv.Atoi(s)
	if err != nil || n < 0 {
		return 0, badRequestf("%s must be a non-negative integer", name)
	}
	return min(n, max), nil
}

// changedFiles lists the paths whose entry differs between oldIdx and
// newIdx (content hash or executable bit), sorted by path, with status,
// hashes and sizes filled in.
func changedFiles(oldIdx, newIdx map[string]*syncpb.ManifestEntry) ([]fileDiff, error) {
	var out []fileDiff
	for p, ne := range newIdx {
		oe := oldIdx[p]
		nh, err := entryHash(ne)
		if err != nil {
			return nil, err
		}
		fd := fileDiff{Path: p, Status: changeAdded, NewHash: nh.Hex(), NewSize: ne.GetSize()}
		if oe != nil {
			oh, err := entryHash(oe)
			if err != nil {
				return nil, err
			}
			if oh == nh && oe.GetExecutable() == ne.GetExecutable() {
				continue
			}
			fd.Status, fd.OldHash, fd.OldSize = changeModified, oh.Hex(), oe.GetSize()
		}
		out = append(out, fd)
	}
	for p, oe := range oldIdx {
		if newIdx[p] != nil {
			continue
		}
		oh, err := entryHash(oe)
		if err != nil {
			return nil, err
		}
		out = append(out, fileDiff{Path: p, Status: changeDeleted, OldHash: oh.Hex(), OldSize: oe.GetSize()})
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Path < out[j].Path })
	return nonNil(out), nil
}

// readablePaths returns which of files' paths p may read under the tree's
// license rules. The common all-readable case costs one check.
func (a *api) readablePaths(ctx context.Context, p *domain.Principal, tree *domain.Tree, files []fileDiff) (map[string]bool, error) {
	paths := make([]string, len(files))
	for i, f := range files {
		paths[i] = f.Path
	}
	ok := make(map[string]bool, len(paths))
	err := a.d.License.CheckRead(ctx, p, tree, paths)
	if err == nil {
		for _, path := range paths {
			ok[path] = true
		}
		return ok, nil
	}
	if !isReadDenied(err) {
		return nil, err
	}
	for _, path := range paths {
		err := a.d.License.CheckRead(ctx, p, tree, []string{path})
		switch {
		case err == nil:
			ok[path] = true
		case !isReadDenied(err):
			return nil, err
		}
	}
	return ok, nil
}

// isReadDenied reports whether err is a license or authorization refusal.
func isReadDenied(err error) bool {
	var denied *domain.PushDenied
	return errors.As(err, &denied) || isDenied(err)
}

// fillPatch computes the patch and line counts of fd, or marks it binary or
// too large. budget is the remaining blob byte budget of the request.
func (a *api) fillPatch(ctx context.Context, fd *fileDiff, oe, ne *syncpb.ManifestEntry, contextLines int, budget *int64) error {
	if fd.OldSize > maxDiffFileSize || fd.NewSize > maxDiffFileSize {
		fd.TooLarge = true
		return nil
	}
	need := int64(fd.OldSize + fd.NewSize)
	if need > *budget {
		fd.TooLarge = true
		return nil
	}
	*budget -= need
	load := func(e *syncpb.ManifestEntry) ([]byte, error) {
		if e == nil {
			return nil, nil
		}
		h, err := entryHash(e)
		if err != nil {
			return nil, err
		}
		data, err := a.d.Objects.Get(ctx, h)
		if err != nil {
			// A referenced blob that is missing is corruption (500).
			return nil, fmt.Errorf("blob %s of %q: %v", h, e.GetPath(), err)
		}
		return data, nil
	}
	oldData, err := load(oe)
	if err != nil {
		return err
	}
	newData, err := load(ne)
	if err != nil {
		return err
	}
	if len(oldData) > maxDiffFileSize || len(newData) > maxDiffFileSize {
		fd.TooLarge = true // manifest sizes understated the blobs
		return nil
	}
	if textdiff.IsBinary(oldData) || textdiff.IsBinary(newData) {
		fd.Binary = true
		return nil
	}
	oldName, newName := "a/"+fd.Path, "b/"+fd.Path
	switch fd.Status {
	case changeAdded:
		oldName = "/dev/null"
	case changeDeleted:
		newName = "/dev/null"
	}
	d := textdiff.Unified(oldData, newData, textdiff.Options{Context: contextLines, OldName: oldName, NewName: newName})
	fd.Patch, fd.Additions, fd.Deletions = d.Patch, d.Additions, d.Deletions
	return nil
}

// mergeBase picks the merge base of from and to given missing =
// History.Missing(to, [from]) for a from that is not an ancestor of to.
// The candidates are the parents of missing snapshots that are not
// themselves missing: exactly the common ancestors at the boundary. The
// newest candidate (by created_at, then id) is returned; "" when the
// histories are unrelated. An empty missing set means to is an ancestor of
// from, so to itself is the merge base.
func (a *api) mergeBase(ctx context.Context, to *domain.Snapshot, missing []domain.Snapshot) (string, error) {
	if len(missing) == 0 {
		return to.ID, nil
	}
	inMissing := make(map[string]bool, len(missing))
	for _, s := range missing {
		inMissing[s.ID] = true
	}
	var best *domain.Snapshot
	seen := map[string]bool{}
	for _, s := range missing {
		for _, pid := range s.Parents {
			if inMissing[pid] || seen[pid] {
				continue
			}
			seen[pid] = true
			c, err := a.d.Store.GetSnapshot(ctx, pid)
			if err != nil {
				return "", fmt.Errorf("merge base candidate %s: %w", pid, err)
			}
			if best == nil || c.CreatedAt.After(best.CreatedAt) || (c.CreatedAt.Equal(best.CreatedAt) && c.ID > best.ID) {
				best = c
			}
		}
	}
	if best == nil {
		return "", nil
	}
	return best.ID, nil
}

// ---- file history ----

// fileHistoryEntry is FileHistoryEntry of apps/admin/src/api/types.ts.
type fileHistoryEntry struct {
	Snapshot domain.Snapshot `json:"snapshot"`
	Change   string          `json:"change"`
}

// pathState is the state of one path in one snapshot.
type pathState struct {
	present bool
	hash    hash.Hash
	exec    bool
}

// handleFileHistory answers GET {tree}/history?path=P&branch=B&limit=50
// with the snapshots on the branch's history (default: the default branch)
// that changed P, newest first. Each snapshot is compared with its first
// parent. At most maxFileHistoryScan snapshots are inspected. Reading the
// history of a path requires the same license read permission as its
// content.
func (a *api) handleFileHistory(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	q := r.URL.Query()
	filePath := q.Get("path")
	if !validTreePath(filePath) {
		return badRequestf("path must be a clean relative file path")
	}
	limit, err := queryInt(r, "limit", defaultFileHistoryLimit, maxFileHistoryLimit)
	if err != nil {
		return err
	}
	if err := a.d.License.CheckRead(ctx, principal(r), tree, []string{filePath}); err != nil {
		return err
	}
	tip, _, err := a.branchTipForInsights(ctx, tree, q.Get("branch"))
	if err != nil {
		return err
	}
	out := []fileHistoryEntry{}
	if tip == "" {
		writeJSON(w, http.StatusOK, out)
		return nil
	}
	log, err := a.d.History.Log(ctx, tip, maxFileHistoryScan)
	if err != nil {
		return err
	}
	byID := make(map[string]*domain.Snapshot, len(log))
	for i := range log {
		byID[log[i].ID] = &log[i]
	}
	cache := map[string]pathState{}
	state := func(s *domain.Snapshot) (pathState, error) {
		if st, ok := cache[s.ID]; ok {
			return st, nil
		}
		m, err := a.loadManifest(ctx, s)
		if err != nil {
			return pathState{}, err
		}
		var st pathState
		if e := manifestEntry(m, filePath); e != nil {
			h, err := entryHash(e)
			if err != nil {
				return pathState{}, err
			}
			st = pathState{present: true, hash: h, exec: e.GetExecutable()}
		}
		cache[s.ID] = st
		return st, nil
	}
	for i := range log {
		if len(out) >= limit {
			break
		}
		s := &log[i]
		cur, err := state(s)
		if err != nil {
			return err
		}
		var prev pathState
		if len(s.Parents) > 0 {
			parent := byID[s.Parents[0]]
			if parent == nil { // beyond the scan window
				if parent, err = a.treeSnapshot(ctx, tree, s.Parents[0]); err != nil {
					return err
				}
			}
			if prev, err = state(parent); err != nil {
				return err
			}
		}
		var change string
		switch {
		case cur.present && !prev.present:
			change = changeAdded
		case !cur.present && prev.present:
			change = changeDeleted
		case cur.present && (cur.hash != prev.hash || cur.exec != prev.exec):
			change = changeModified
		default:
			continue
		}
		snap := *s
		normalizeSnapshot(&snap)
		out = append(out, fileHistoryEntry{Snapshot: snap, Change: change})
	}
	writeJSON(w, http.StatusOK, out)
	return nil
}

// branchTipForInsights resolves the branch= parameter (default: the tree's
// default branch) to its tip and name. An explicit unknown branch is 404;
// a missing default branch (nothing pushed yet) yields tip "".
func (a *api) branchTipForInsights(ctx context.Context, tree *domain.Tree, name string) (tip, branch string, err error) {
	explicit := name != ""
	if !explicit {
		name = tree.DefaultBranch
	}
	b, err := a.d.Store.GetBranch(ctx, tree.ID, name)
	switch {
	case errors.Is(err, domain.ErrNotFound) && !explicit:
		return "", name, nil
	case errors.Is(err, domain.ErrNotFound):
		return "", name, notFoundf("branch %q not found", name)
	case err != nil:
		return "", name, err
	}
	return b.Tip, name, nil
}

// ---- stats ----

type contributorStat struct {
	Author    string    `json:"author"`
	Snapshots int       `json:"snapshots"`
	LastAt    time.Time `json:"last_at"`
}

type activityDay struct {
	Date      string `json:"date"`
	Snapshots int    `json:"snapshots"`
}

type languageShare struct {
	Language string `json:"language"`
	Files    int    `json:"files"`
	Bytes    uint64 `json:"bytes"`
}

// treeStats is TreeStats of apps/admin/src/api/types.ts.
type treeStats struct {
	Branch        string            `json:"branch"`
	SnapshotCount int               `json:"snapshot_count"`
	Contributors  []contributorStat `json:"contributors"`
	Activity      []activityDay     `json:"activity"`
	FileCount     int               `json:"file_count"`
	TotalBytes    uint64            `json:"total_bytes"`
	Languages     []languageShare   `json:"languages"`
}

// statsNow is the clock of the stats endpoint (a variable for tests).
var statsNow = time.Now

// handleTreeStats answers GET {tree}/stats?branch=B&days=30.
//
// snapshot_count, contributors and activity cover the snapshots of the
// branch's history created within the last `days` UTC calendar days
// (today included); activity has one entry per day, oldest first.
// file_count, total_bytes and languages describe the branch tip.
func (a *api) handleTreeStats(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	days, err := queryInt(r, "days", defaultStatsDays, maxStatsDays)
	if err != nil {
		return err
	}
	tip, branch, err := a.branchTipForInsights(ctx, tree, r.URL.Query().Get("branch"))
	if err != nil {
		return err
	}

	now := statsNow().UTC()
	today := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
	start := today.AddDate(0, 0, -(days - 1))
	st := treeStats{
		Branch:       branch,
		Contributors: []contributorStat{},
		Activity:     make([]activityDay, days),
		Languages:    []languageShare{},
	}
	for i := range st.Activity {
		st.Activity[i].Date = start.AddDate(0, 0, i).Format(time.DateOnly)
	}
	if tip == "" {
		writeJSON(w, http.StatusOK, st)
		return nil
	}

	window, err := a.logSince(ctx, tip, start)
	if err != nil {
		return err
	}
	byAuthor := map[string]*contributorStat{}
	for _, s := range window {
		st.SnapshotCount++
		created := s.CreatedAt.UTC()
		day := int(time.Date(created.Year(), created.Month(), created.Day(), 0, 0, 0, 0, time.UTC).Sub(start) / (24 * time.Hour))
		st.Activity[min(max(day, 0), days-1)].Snapshots++ // future-dated snapshots count as today
		c := byAuthor[s.Author]
		if c == nil {
			c = &contributorStat{Author: s.Author}
			byAuthor[s.Author] = c
		}
		c.Snapshots++
		if created.After(c.LastAt) {
			c.LastAt = created
		}
	}
	for _, c := range byAuthor {
		st.Contributors = append(st.Contributors, *c)
	}
	sort.Slice(st.Contributors, func(i, j int) bool {
		a, b := st.Contributors[i], st.Contributors[j]
		if a.Snapshots != b.Snapshots {
			return a.Snapshots > b.Snapshots
		}
		if !a.LastAt.Equal(b.LastAt) {
			return a.LastAt.After(b.LastAt)
		}
		return a.Author < b.Author
	})

	tipSnap, err := a.treeSnapshot(ctx, tree, tip)
	if err != nil {
		return err
	}
	m, err := a.loadManifest(ctx, tipSnap)
	if err != nil {
		return err
	}
	st.FileCount = len(m.GetEntries())
	for _, e := range m.GetEntries() {
		st.TotalBytes += e.GetSize()
	}
	st.Languages = languageShares(m.GetEntries())
	writeJSON(w, http.StatusOK, st)
	return nil
}

// logSince returns the snapshots of tip's history created at or after
// since, newest first. History.Log is ordered by created_at, so the log is
// fetched in growing pages until it reaches past since.
func (a *api) logSince(ctx context.Context, tip string, since time.Time) ([]domain.Snapshot, error) {
	for limit := 256; ; limit *= 4 {
		log, err := a.d.History.Log(ctx, tip, limit)
		if err != nil {
			return nil, err
		}
		complete := len(log) < limit
		if n := len(log); n > 0 && log[n-1].CreatedAt.Before(since) {
			complete = true
		}
		if !complete {
			continue
		}
		i := sort.Search(len(log), func(i int) bool { return log[i].CreatedAt.Before(since) })
		return log[:i], nil
	}
}

// languageShares aggregates manifest entries by language (detected from
// the file name), returning the top maxStatsLanguages by bytes followed by
// "Other" for everything else, including unrecognised files.
func languageShares(entries []*syncpb.ManifestEntry) []languageShare {
	const other = "Other"
	agg := map[string]*languageShare{}
	for _, e := range entries {
		lang := languageOf(e.GetPath())
		if lang == "" {
			lang = other
		}
		ls := agg[lang]
		if ls == nil {
			ls = &languageShare{Language: lang}
			agg[lang] = ls
		}
		ls.Files++
		ls.Bytes += e.GetSize()
	}
	known := make([]languageShare, 0, len(agg))
	for name, ls := range agg {
		if name != other {
			known = append(known, *ls)
		}
	}
	sort.Slice(known, func(i, j int) bool {
		if known[i].Bytes != known[j].Bytes {
			return known[i].Bytes > known[j].Bytes
		}
		if known[i].Files != known[j].Files {
			return known[i].Files > known[j].Files
		}
		return known[i].Language < known[j].Language
	})
	rest := languageShare{Language: other}
	if ls := agg[other]; ls != nil {
		rest = *ls
	}
	if len(known) > maxStatsLanguages {
		for _, ls := range known[maxStatsLanguages:] {
			rest.Files += ls.Files
			rest.Bytes += ls.Bytes
		}
		known = known[:maxStatsLanguages]
	}
	if rest.Files > 0 {
		known = append(known, rest)
	}
	return known
}

// languageByName recognises well-known extension-less file names.
var languageByName = map[string]string{
	"makefile":       "Makefile",
	"gnumakefile":    "Makefile",
	"dockerfile":     "Dockerfile",
	"cmakelists.txt": "CMake",
	"rakefile":       "Ruby",
	"gemfile":        "Ruby",
	"build.bazel":    "Starlark",
	"workspace":      "Starlark",
}

// languageByExt maps lower-case file extensions to languages.
var languageByExt = map[string]string{
	".go": "Go", ".rs": "Rust", ".py": "Python", ".pyi": "Python",
	".js": "JavaScript", ".mjs": "JavaScript", ".cjs": "JavaScript", ".jsx": "JavaScript",
	".ts": "TypeScript", ".mts": "TypeScript", ".cts": "TypeScript", ".tsx": "TypeScript",
	".java": "Java", ".kt": "Kotlin", ".kts": "Kotlin", ".scala": "Scala", ".groovy": "Groovy", ".gradle": "Groovy",
	".swift": "Swift", ".m": "Objective-C", ".mm": "Objective-C++",
	".c": "C", ".h": "C", ".cc": "C++", ".cpp": "C++", ".cxx": "C++", ".hpp": "C++", ".hh": "C++", ".hxx": "C++",
	".cs": "C#", ".fs": "F#", ".fsx": "F#", ".vb": "Visual Basic",
	".rb": "Ruby", ".php": "PHP", ".pl": "Perl", ".pm": "Perl", ".lua": "Lua", ".r": "R", ".jl": "Julia",
	".dart": "Dart", ".ex": "Elixir", ".exs": "Elixir", ".erl": "Erlang", ".hrl": "Erlang",
	".hs": "Haskell", ".ml": "OCaml", ".mli": "OCaml", ".clj": "Clojure", ".cljs": "Clojure", ".elm": "Elm",
	".zig": "Zig", ".nim": "Nim", ".sol": "Solidity", ".asm": "Assembly", ".s": "Assembly",
	".sh": "Shell", ".bash": "Shell", ".zsh": "Shell", ".fish": "Shell", ".ps1": "PowerShell", ".psm1": "PowerShell",
	".bat": "Batchfile", ".cmd": "Batchfile",
	".sql": "SQL", ".html": "HTML", ".htm": "HTML", ".css": "CSS", ".scss": "SCSS", ".sass": "SCSS", ".less": "Less",
	".vue": "Vue", ".svelte": "Svelte",
	".md": "Markdown", ".markdown": "Markdown", ".mdx": "MDX", ".rst": "reStructuredText", ".tex": "TeX",
	".json": "JSON", ".yaml": "YAML", ".yml": "YAML", ".toml": "TOML", ".xml": "XML",
	".proto": "Protocol Buffers", ".graphql": "GraphQL", ".gql": "GraphQL",
	".tf": "HCL", ".hcl": "HCL", ".cmake": "CMake", ".bzl": "Starlark", ".nix": "Nix", ".ipynb": "Jupyter Notebook",
}

// languageOf returns the language of a file path, or "" if unknown.
func languageOf(p string) string {
	base := strings.ToLower(path.Base(p))
	if lang, ok := languageByName[base]; ok {
		return lang
	}
	if strings.HasPrefix(base, "dockerfile.") {
		return "Dockerfile"
	}
	return languageByExt[path.Ext(base)]
}

// ---- staged summary ----

// stagedSummarizer is implemented by staged.Service. Deps.Staged is
// type-asserted to it; other implementations get a summary computed from
// StagedService.List.
type stagedSummarizer interface {
	Summary(ctx context.Context, p *domain.Principal, tree *domain.Tree) ([]staged.BranchActivity, error)
}

type stagedAuthorActivity struct {
	Author    string    `json:"author"`
	AccountID string    `json:"account_id"`
	Files     int       `json:"files"`
	StagedAt  time.Time `json:"staged_at"`
}

type stagedFileOverlap struct {
	Path     string   `json:"path"`
	Authors  []string `json:"authors"`
	Branches []string `json:"branches"`
}

// stagedBranchActivity is StagedBranchActivity of apps/admin/src/api/types.ts.
type stagedBranchActivity struct {
	Branch       string                 `json:"branch"`
	Authors      []stagedAuthorActivity `json:"authors"`
	FilesChanged int                    `json:"files_changed"`
	LatestAt     time.Time              `json:"latest_at"`
	Overlaps     []stagedFileOverlap    `json:"overlaps"`
}

// handleStagedSummary answers GET {tree}/staged/summary with the tree's
// staged (WIP) activity per branch, most recently active first.
func (a *api) handleStagedSummary(w http.ResponseWriter, r *http.Request) error {
	_, tree, err := a.treeFor(r, domain.ActTreeRead)
	if err != nil {
		return err
	}
	ctx := r.Context()
	p := principal(r)
	var acts []staged.BranchActivity
	if s, ok := a.d.Staged.(stagedSummarizer); ok {
		if acts, err = s.Summary(ctx, p, tree); err != nil {
			return err
		}
	} else {
		entries, err := a.d.Staged.List(ctx, p, tree, "")
		if err != nil {
			return err
		}
		acts = summarizeStaged(entries)
	}
	out := make([]stagedBranchActivity, 0, len(acts))
	for _, ba := range acts {
		out = append(out, toStagedBranchActivity(ba))
	}
	writeJSON(w, http.StatusOK, out)
	return nil
}

// toStagedBranchActivity maps the staged package's aggregate onto the API
// shape. An author's file count is the larger of the reported counters and
// the stored path list; files_changed is the distinct listed paths, but at
// least the largest single author's count.
func toStagedBranchActivity(ba staged.BranchActivity) stagedBranchActivity {
	out := stagedBranchActivity{
		Branch:       ba.Branch,
		Authors:      make([]stagedAuthorActivity, 0, len(ba.Authors)),
		FilesChanged: ba.DistinctFiles,
		LatestAt:     ba.LatestAt,
		Overlaps:     make([]stagedFileOverlap, 0, len(ba.Overlaps)),
	}
	for _, au := range ba.Authors {
		files := max(int(au.FilesAdded)+int(au.FilesModified)+int(au.FilesDeleted), au.FilesListed)
		out.FilesChanged = max(out.FilesChanged, files)
		out.Authors = append(out.Authors, stagedAuthorActivity{
			Author: au.Author, AccountID: au.AccountID, Files: files, StagedAt: au.StagedAt,
		})
	}
	for _, ov := range ba.Overlaps {
		authors, branches := map[string]bool{}, map[string]bool{}
		for _, c := range ov.Contributors {
			name := c.Author
			if name == "" {
				name = c.AccountID
			}
			authors[name] = true
			branches[c.Branch] = true
		}
		out.Overlaps = append(out.Overlaps, stagedFileOverlap{Path: ov.Path, Authors: sortedKeys(authors), Branches: sortedKeys(branches)})
	}
	return out
}

func sortedKeys(m map[string]bool) []string {
	out := make([]string, 0, len(m))
	for k := range m {
		out = append(out, k)
	}
	sort.Strings(out)
	return out
}

// summarizeStaged aggregates staged entries per branch with the semantics
// of staged.Service.Summary: authors newest first, overlaps are listed
// paths also staged by a different account on any branch of the tree,
// branches ordered by most recent activity.
func summarizeStaged(entries []domain.StagedSnapshot) []staged.BranchActivity {
	byBranch := map[string]*staged.BranchActivity{}
	byPath := map[string][]staged.Contributor{}
	for _, e := range entries {
		ba := byBranch[e.BranchName]
		if ba == nil {
			ba = &staged.BranchActivity{Branch: e.BranchName}
			byBranch[e.BranchName] = ba
		}
		ba.Authors = append(ba.Authors, staged.AuthorActivity{
			AccountID: e.AccountID, Author: e.Author, SnapshotID: e.SnapshotID, Message: e.Message,
			FilesAdded: e.FilesAdded, FilesModified: e.FilesModified, FilesDeleted: e.FilesDeleted,
			FilesListed: len(e.FilesChanged), StagedAt: e.StagedAt,
		})
		ba.FilesAdded += int(e.FilesAdded)
		ba.FilesModified += int(e.FilesModified)
		ba.FilesDeleted += int(e.FilesDeleted)
		if e.StagedAt.After(ba.LatestAt) {
			ba.LatestAt = e.StagedAt
		}
		seen := map[string]bool{}
		for _, f := range e.FilesChanged {
			if seen[f] {
				continue
			}
			seen[f] = true
			byPath[f] = append(byPath[f], staged.Contributor{AccountID: e.AccountID, Author: e.Author, Branch: e.BranchName})
		}
	}
	paths := make([]string, 0, len(byPath))
	for f := range byPath {
		paths = append(paths, f)
	}
	sort.Strings(paths)
	for _, f := range paths {
		cs := byPath[f]
		branches, accounts := map[string]bool{}, map[string]bool{}
		for _, c := range cs {
			branches[c.Branch] = true
			accounts[c.AccountID] = true
		}
		for b := range branches {
			byBranch[b].DistinctFiles++
			if len(accounts) >= 2 {
				byBranch[b].Overlaps = append(byBranch[b].Overlaps, staged.FileOverlap{Path: f, Contributors: cs})
			}
		}
	}
	out := make([]staged.BranchActivity, 0, len(byBranch))
	for _, ba := range byBranch {
		sort.SliceStable(ba.Authors, func(i, j int) bool {
			x, y := ba.Authors[i], ba.Authors[j]
			if !x.StagedAt.Equal(y.StagedAt) {
				return x.StagedAt.After(y.StagedAt)
			}
			return x.Author < y.Author
		})
		out = append(out, *ba)
	}
	sort.Slice(out, func(i, j int) bool {
		if !out[i].LatestAt.Equal(out[j].LatestAt) {
			return out[i].LatestAt.After(out[j].LatestAt)
		}
		return out[i].Branch < out[j].Branch
	})
	return out
}

// ---- recent events ----

// recentEvents is implemented by events.Bus. Deps.Bus is type-asserted to
// it; buses without a history yield an empty list.
type recentEvents interface {
	Recent(f domain.EventFilter, n int) []domain.Event
}

// handleRecentEvents answers GET /api/v1/events/recent?tenant=&tree=&types=&limit=50
// with the most recent retained events the caller may see, oldest first.
// Authorization and scoping are those of the GET /events stream.
func (a *api) handleRecentEvents(w http.ResponseWriter, r *http.Request) error {
	f, allow, err := a.eventScope(r)
	if err != nil {
		return err
	}
	limit, err := queryInt(r, "limit", defaultRecentEvents, maxRecentEvents)
	if err != nil {
		return err
	}
	out := []json.RawMessage{}
	src, ok := a.d.Bus.(recentEvents)
	if !ok || src == nil {
		writeJSON(w, http.StatusOK, out)
		return nil
	}
	var evs []domain.Event
	if allow == nil {
		evs = src.Recent(f, limit)
	} else {
		for _, e := range src.Recent(f, 0) {
			if allow(e) {
				evs = append(evs, e)
			}
		}
		if len(evs) > limit {
			evs = evs[len(evs)-limit:]
		}
	}
	for _, e := range evs {
		raw, err := json.Marshal(e)
		if err != nil {
			// Unencodable payload: keep the envelope, as the SSE stream does.
			e.Payload = nil
			if raw, err = json.Marshal(e); err != nil {
				continue
			}
		}
		out = append(out, raw)
	}
	writeJSON(w, http.StatusOK, out)
	return nil
}
