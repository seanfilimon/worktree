// Package gc garbage-collects unreachable objects and expires staged
// snapshots and audit events (docs/ENGINEERING_PLAN.md §13.3).
//
// A run (Collector.Run) proceeds in four steps:
//
//  1. Retention: staged snapshots older than StagedRetention are deleted
//     (they then stop being GC roots) and audit events older than
//     AuditRetention are pruned (MetaStore.PruneAudit).
//  2. Mark: every object reachable from a root is collected — roots are
//     every branch tip, tag, staged snapshot, merge request source/target
//     tip (and merged snapshot) and release snapshot and asset. From each
//     root snapshot the ancestry is walked; each snapshot contributes its
//     manifest and every object its entries need (blobs, chunk lists and
//     chunks). Any storage error aborts the run before anything is
//     deleted.
//  3. Sweep: every stored object is classified. Unreachable objects get a
//     mark in doc collection gc_marks (key hex hash, value {hash,
//     marked_at}); objects whose mark is older than Grace and that are
//     still unreachable are deleted. Reachable objects lose their mark.
//  4. Report: the GCReport is stored in doc collection gc_runs (key = the
//     run's start, fixed-width RFC 3339) of which the last 20 are kept,
//     the exact per-tenant storage usage is recorded (limits.SetStorageBytes)
//     and a "gc.run" audit event is written.
//
// Dry runs perform the classification and report what a live run would do
// without changing anything. One run executes at a time.
package gc

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"sort"
	"strconv"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/hash"
	"github.com/seanfilimon/worktree/services/server-go/internal/limits"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
	"github.com/seanfilimon/worktree/services/server-go/internal/opsmetrics"
)

// Doc collections (§13.14).
const (
	CollMarks = "gc_marks"
	CollRuns  = "gc_runs"
	// collReleases is the release workstream's collection (§13.14).
	collReleases = "releases"
)

// KeepRuns is the number of GCReports kept in gc_runs.
const KeepRuns = 20

// runKeyLayout is RFC 3339 with fixed-width nanoseconds, so keys sort
// chronologically.
const runKeyLayout = "2006-01-02T15:04:05.000000000Z07:00"

// ErrRunning is returned by Run while another run is in progress.
var ErrRunning = errors.New("gc: a garbage-collection run is already in progress")

// Mark is a gc_marks document.
type Mark struct {
	Hash     string    `json:"hash"`
	MarkedAt time.Time `json:"marked_at"`
}

// Config is the effective collector configuration.
type Config struct {
	Interval        time.Duration
	Grace           time.Duration
	StagedRetention time.Duration
	AuditRetention  time.Duration
}

// Deps are the collaborators of the gc package. Store and Objects are
// required for object collection (without Objects only retention runs);
// the others may be nil.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger

	// Interval between automatic runs (0 disables the ticker).
	Interval time.Duration
	// Grace is how long an object stays marked before deletion.
	Grace time.Duration
	// StagedRetention expires staged snapshots older than this (0 keeps).
	StagedRetention time.Duration
	// AuditRetention prunes audit events older than this (0 keeps).
	AuditRetention time.Duration
	// Now is the clock (nil = time.Now).
	Now func() time.Time
}

// Collector is the gc component.
type Collector struct {
	d Deps

	run sync.Mutex // held for the duration of a run

	mu      sync.Mutex
	running bool
	nextRun time.Time
}

// New returns a Collector.
func New(d Deps) *Collector {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	if d.Now == nil {
		d.Now = time.Now
	}
	return &Collector{d: d}
}

// Config returns the collector configuration.
func (c *Collector) Config() Config {
	return Config{Interval: c.d.Interval, Grace: c.d.Grace, StagedRetention: c.d.StagedRetention, AuditRetention: c.d.AuditRetention}
}

// Running reports whether a run is in progress.
func (c *Collector) Running() bool {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.running
}

// NextRun returns the time of the next scheduled run (zero when the ticker
// is not running).
func (c *Collector) NextRun() time.Time {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.nextRun
}

// Runs returns the stored reports, newest first.
func (c *Collector) Runs(ctx context.Context) ([]domain.GCReport, error) {
	raws, err := c.d.Store.ListDocs(ctx, CollRuns, "")
	if err != nil {
		return nil, err
	}
	out := make([]domain.GCReport, 0, len(raws))
	for i := len(raws) - 1; i >= 0; i-- {
		var r domain.GCReport
		if err := json.Unmarshal(raws[i], &r); err != nil {
			return nil, fmt.Errorf("gc: decode run: %w", err)
		}
		out = append(out, r)
	}
	return out, nil
}

// Start runs the collector every Interval until ctx is cancelled or the
// returned stop function is called (which waits for a run in progress).
// With Interval 0 it does nothing.
func (c *Collector) Start(ctx context.Context) (stop func()) {
	if c.d.Interval <= 0 {
		return func() {}
	}
	ctx, cancel := context.WithCancel(ctx)
	done := make(chan struct{})
	go func() {
		defer close(done)
		t := time.NewTicker(c.d.Interval)
		defer t.Stop()
		for {
			c.mu.Lock()
			c.nextRun = c.d.Now().Add(c.d.Interval)
			c.mu.Unlock()
			select {
			case <-ctx.Done():
				return
			case <-t.C:
			}
			if _, err := c.Run(ctx, false); err != nil && !errors.Is(err, ErrRunning) && ctx.Err() == nil {
				c.d.Logger.Error("gc: scheduled run failed", slog.Any("error", err))
			}
		}
	}()
	return func() {
		cancel()
		<-done
		c.mu.Lock()
		c.nextRun = time.Time{}
		c.mu.Unlock()
	}
}

// Run performs one collection (see the package documentation). The report
// is returned (and stored) even when the run fails part-way; its Error
// field then describes the failure.
func (c *Collector) Run(ctx context.Context, dryRun bool) (*domain.GCReport, error) {
	if !c.run.TryLock() {
		return nil, ErrRunning
	}
	defer c.run.Unlock()
	c.mu.Lock()
	c.running = true
	c.mu.Unlock()
	defer func() {
		c.mu.Lock()
		c.running = false
		c.mu.Unlock()
	}()

	start := c.d.Now().UTC()
	rep := &domain.GCReport{DryRun: dryRun, StartedAt: start}
	err := c.collect(ctx, rep, start)
	rep.FinishedAt = c.d.Now().UTC()
	if err != nil {
		rep.Error = err.Error()
	}
	opsmetrics.GCRuns.Inc("")
	if !dryRun {
		opsmetrics.GCDeletedObjects.Add("", uint64(rep.Deleted))
	}
	// Record the run even after a failure (with a fresh context when the
	// run was cancelled), so operators can see what happened.
	rctx := context.WithoutCancel(ctx)
	if serr := c.storeRun(rctx, rep); serr != nil {
		c.d.Logger.Warn("gc: storing run report failed", slog.Any("error", serr))
	}
	c.auditRun(rctx, rep)
	level := slog.LevelInfo
	if err != nil {
		level = slog.LevelError
	}
	c.d.Logger.Log(rctx, level, "gc: run finished",
		slog.Bool("dry_run", dryRun), slog.Int("scanned", rep.Scanned), slog.Int("unreachable", rep.Unreachable),
		slog.Int("marked", rep.Marked), slog.Int("deleted", rep.Deleted), slog.Int64("freed_bytes", rep.FreedBytes),
		slog.Int("staged_deleted", rep.StagedDeleted), slog.Int("audit_pruned", rep.AuditPruned),
		slog.Duration("duration", rep.FinishedAt.Sub(rep.StartedAt)), slog.Any("error", err))
	if err != nil {
		return rep, err
	}
	return rep, nil
}

func (c *Collector) collect(ctx context.Context, rep *domain.GCReport, now time.Time) error {
	trees, err := c.d.Store.ListTrees(ctx, "")
	if err != nil {
		return fmt.Errorf("list trees: %w", err)
	}
	expired, err := c.expireStaged(ctx, rep, trees, now)
	if err != nil {
		return err
	}
	if err := c.pruneAudit(ctx, rep, now); err != nil {
		return err
	}
	if c.d.Objects == nil {
		return nil
	}
	m, err := c.mark(ctx, trees, expired)
	if err != nil {
		return err
	}
	if err := c.sweep(ctx, rep, m.reachable, now); err != nil {
		return err
	}
	if !rep.DryRun {
		c.recordUsage(ctx, m, now)
	}
	return nil
}

// stagedKey identifies a staged entry.
func stagedKey(s domain.StagedSnapshot) string {
	return domain.DocKey(s.TreeID, s.AccountID, s.BranchName, s.Label) // staged-v2 (§16.8): + label
}

// expireStaged deletes (or, in a dry run, counts) staged snapshots older
// than the retention and returns the keys of the expired entries.
func (c *Collector) expireStaged(ctx context.Context, rep *domain.GCReport, trees []domain.Tree, now time.Time) (map[string]bool, error) {
	expired := map[string]bool{}
	if c.d.StagedRetention <= 0 {
		return expired, nil
	}
	cutoff := now.Add(-c.d.StagedRetention)
	for _, t := range trees {
		staged, err := c.d.Store.ListStaged(ctx, t.ID, "")
		if err != nil {
			return nil, fmt.Errorf("list staged of %s: %w", t.ID, err)
		}
		for _, s := range staged {
			if !s.StagedAt.Before(cutoff) {
				continue
			}
			expired[stagedKey(s)] = true
			rep.StagedDeleted++
			if rep.DryRun {
				continue
			}
			if err := c.d.Store.DeleteStagedLabel(ctx, s.TreeID, s.AccountID, s.BranchName, s.Label); err != nil && !errors.Is(err, domain.ErrNotFound) {
				return nil, fmt.Errorf("delete staged snapshot: %w", err)
			}
			if c.d.Bus != nil {
				c.d.Bus.Publish(domain.Event{Type: domain.EventStagedCleared, TenantID: s.TenantID, TreeID: s.TreeID,
					Actor: "system", At: now, Payload: map[string]any{"branch": s.BranchName, "account_id": s.AccountID, "reason": "expired"}})
			}
		}
	}
	return expired, nil
}

func (c *Collector) pruneAudit(ctx context.Context, rep *domain.GCReport, now time.Time) error {
	if c.d.AuditRetention <= 0 {
		return nil
	}
	cutoff := now.Add(-c.d.AuditRetention)
	if rep.DryRun {
		evs, err := c.d.Store.ListAudit(ctx, domain.AuditFilter{Until: cutoff, Limit: 1 << 30})
		if err != nil {
			return fmt.Errorf("count audit events: %w", err)
		}
		rep.AuditPruned = len(evs)
		return nil
	}
	n, err := c.d.Store.PruneAudit(ctx, cutoff)
	if err != nil {
		return fmt.Errorf("prune audit events: %w", err)
	}
	rep.AuditPruned = n
	return nil
}

// marking is the result of the mark phase.
type marking struct {
	reachable map[hash.Hash]struct{}
	// perTenant holds the reachable objects of each tenant's trees.
	perTenant map[string]map[hash.Hash]struct{}
}

// mark collects every object reachable from a root.
func (c *Collector) mark(ctx context.Context, trees []domain.Tree, expired map[string]bool) (*marking, error) {
	m := &marking{reachable: map[hash.Hash]struct{}{}, perTenant: map[string]map[hash.Hash]struct{}{}}
	// Per tenant: snapshots walked and manifests expanded, so each is
	// handled once per tenant (and per-tenant usage stays complete).
	type tenantState struct {
		visited   map[string]bool
		manifests map[hash.Hash]bool
	}
	states := map[string]*tenantState{}
	for _, t := range trees {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		set := m.perTenant[t.TenantID]
		st := states[t.TenantID]
		if set == nil {
			set = map[hash.Hash]struct{}{}
			m.perTenant[t.TenantID] = set
			st = &tenantState{visited: map[string]bool{}, manifests: map[hash.Hash]bool{}}
			states[t.TenantID] = st
		}
		roots, assets, err := c.roots(ctx, t, expired)
		if err != nil {
			return nil, err
		}
		for _, h := range assets {
			set[h] = struct{}{}
		}
		// Staged working manifests (not snapshot records on the server).
		staged, err := c.d.Store.ListStaged(ctx, t.ID, "")
		if err != nil {
			return nil, fmt.Errorf("list staged of %s: %w", t.ID, err)
		}
		for _, s := range staged {
			mh, err := hash.FromHex(s.ManifestHash)
			if err != nil || expired[stagedKey(s)] || st.manifests[mh] {
				continue
			}
			st.manifests[mh] = true
			if err := limits.SnapshotObjects(ctx, c.d.Objects, mh, set); err != nil {
				return nil, fmt.Errorf("staged manifest of %s: %w", t.ID, err)
			}
		}
		stack := roots
		for len(stack) > 0 {
			id := stack[len(stack)-1]
			stack = stack[:len(stack)-1]
			if id == "" || st.visited[id] {
				continue
			}
			st.visited[id] = true
			s, err := c.d.Store.GetSnapshot(ctx, id)
			if errors.Is(err, domain.ErrNotFound) {
				continue // e.g. a staged snapshot whose record is client-side only
			}
			if err != nil {
				return nil, fmt.Errorf("load snapshot %s: %w", id, err)
			}
			stack = append(stack, s.Parents...)
			if st.manifests[s.ManifestHash] {
				continue
			}
			st.manifests[s.ManifestHash] = true
			if err := limits.SnapshotObjects(ctx, c.d.Objects, s.ManifestHash, set); err != nil {
				return nil, fmt.Errorf("snapshot %s: %w", id, err)
			}
		}
	}
	for _, set := range m.perTenant {
		for h := range set {
			m.reachable[h] = struct{}{}
		}
	}
	return m, nil
}

// roots returns the snapshot ids and directly referenced objects (release
// assets) that keep tree t's content alive.
func (c *Collector) roots(ctx context.Context, t domain.Tree, expired map[string]bool) ([]string, []hash.Hash, error) {
	var ids []string
	branches, err := c.d.Store.ListBranches(ctx, t.ID)
	if err != nil {
		return nil, nil, fmt.Errorf("list branches of %s: %w", t.ID, err)
	}
	tips := map[string]string{}
	for _, b := range branches {
		ids = append(ids, b.Tip)
		tips[b.Name] = b.Tip
	}
	tags, err := c.d.Store.ListTags(ctx, t.ID)
	if err != nil {
		return nil, nil, fmt.Errorf("list tags of %s: %w", t.ID, err)
	}
	for _, tg := range tags {
		ids = append(ids, tg.Target)
	}
	staged, err := c.d.Store.ListStaged(ctx, t.ID, "")
	if err != nil {
		return nil, nil, fmt.Errorf("list staged of %s: %w", t.ID, err)
	}
	for _, s := range staged {
		if !expired[stagedKey(s)] {
			ids = append(ids, s.SnapshotID)
		}
	}
	mrs, err := c.d.Store.ListMergeRequests(ctx, t.ID, "")
	if err != nil {
		return nil, nil, fmt.Errorf("list merge requests of %s: %w", t.ID, err)
	}
	for _, mr := range mrs {
		ids = append(ids, tips[mr.SourceBranch], tips[mr.TargetBranch], mr.MergedSnapshot)
	}
	var assets []hash.Hash
	raws, err := c.d.Store.ListDocs(ctx, collReleases, domain.DocKey(t.ID)+"\x00")
	if err != nil {
		return nil, nil, fmt.Errorf("list releases of %s: %w", t.ID, err)
	}
	for _, raw := range raws {
		var r domain.Release
		if err := json.Unmarshal(raw, &r); err != nil {
			return nil, nil, fmt.Errorf("decode release of %s: %w", t.ID, err)
		}
		ids = append(ids, r.SnapshotID)
		for _, a := range r.Assets {
			if !a.Hash.IsZero() {
				assets = append(assets, a.Hash)
			}
		}
	}
	return ids, assets, nil
}

// sweep classifies every stored object against the reachable set.
func (c *Collector) sweep(ctx context.Context, rep *domain.GCReport, reachable map[hash.Hash]struct{}, now time.Time) error {
	marks, err := c.loadMarks(ctx)
	if err != nil {
		return err
	}
	lister, ok := c.d.Objects.(objstore.ListStore)
	if !ok {
		return errors.New("object store cannot enumerate objects")
	}
	var stored []hash.Hash
	if err := lister.List(ctx, func(h hash.Hash) error {
		stored = append(stored, h)
		return nil
	}); err != nil {
		return fmt.Errorf("list objects: %w", err)
	}
	present := make(map[string]bool, len(stored))
	for _, h := range stored {
		if err := ctx.Err(); err != nil {
			return err
		}
		key := h.Hex()
		present[key] = true
		rep.Scanned++
		mk, marked := marks[key]
		if _, live := reachable[h]; live {
			rep.Reachable++
			if marked && !rep.DryRun {
				if err := c.d.Store.DeleteDoc(ctx, CollMarks, key); err != nil && !errors.Is(err, domain.ErrNotFound) {
					return fmt.Errorf("unmark %s: %w", key, err)
				}
			}
			continue
		}
		rep.Unreachable++
		if !marked {
			rep.Marked++
			if !rep.DryRun {
				if err := c.d.Store.PutDoc(ctx, CollMarks, key, Mark{Hash: key, MarkedAt: now}); err != nil {
					return fmt.Errorf("mark %s: %w", key, err)
				}
			}
			continue
		}
		if now.Sub(mk.MarkedAt) < c.d.Grace {
			continue
		}
		size, err := objstore.ObjectSize(ctx, c.d.Objects, h)
		if errors.Is(err, objstore.ErrNotFound) {
			size = 0
		} else if err != nil {
			return fmt.Errorf("size %s: %w", key, err)
		}
		rep.Deleted++
		rep.FreedBytes += size
		if rep.DryRun {
			continue
		}
		if err := c.d.Objects.Delete(ctx, h); err != nil {
			return fmt.Errorf("delete %s: %w", key, err)
		}
		if err := c.d.Store.DeleteDoc(ctx, CollMarks, key); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return fmt.Errorf("unmark %s: %w", key, err)
		}
	}
	// Marks of objects that no longer exist are dropped.
	if !rep.DryRun {
		for key := range marks {
			if present[key] {
				continue
			}
			if err := c.d.Store.DeleteDoc(ctx, CollMarks, key); err != nil && !errors.Is(err, domain.ErrNotFound) {
				return fmt.Errorf("drop stale mark %s: %w", key, err)
			}
		}
	}
	return nil
}

func (c *Collector) loadMarks(ctx context.Context) (map[string]Mark, error) {
	raws, err := c.d.Store.ListDocs(ctx, CollMarks, "")
	if err != nil {
		return nil, fmt.Errorf("list marks: %w", err)
	}
	out := make(map[string]Mark, len(raws))
	for _, raw := range raws {
		var mk Mark
		if err := json.Unmarshal(raw, &mk); err != nil {
			return nil, fmt.Errorf("decode mark: %w", err)
		}
		if mk.Hash != "" {
			out[mk.Hash] = mk
		}
	}
	return out, nil
}

// recordUsage stores the exact storage usage of every tenant with trees.
// Failures are logged: usage is advisory and recomputed next run.
func (c *Collector) recordUsage(ctx context.Context, m *marking, now time.Time) {
	tenants := make([]string, 0, len(m.perTenant))
	for id := range m.perTenant {
		tenants = append(tenants, id)
	}
	sort.Strings(tenants)
	for _, id := range tenants {
		var total int64
		for h := range m.perTenant[id] {
			n, err := objstore.ObjectSize(ctx, c.d.Objects, h)
			if err == nil {
				total += n
			}
		}
		if err := limits.SetStorageBytes(ctx, c.d.Store, id, total, now); err != nil {
			c.d.Logger.Warn("gc: recording tenant usage failed", slog.String("tenant_id", id), slog.Any("error", err))
		}
	}
}

// storeRun saves rep and trims gc_runs to the newest KeepRuns reports.
func (c *Collector) storeRun(ctx context.Context, rep *domain.GCReport) error {
	// Keys must be unique: back-to-back runs can share a clock reading on
	// coarse clocks, so a taken key moves the start by a nanosecond.
	for i := 0; ; i++ {
		err := c.d.Store.CreateDoc(ctx, CollRuns, rep.StartedAt.UTC().Format(runKeyLayout), rep)
		if err == nil {
			break
		}
		if !errors.Is(err, domain.ErrAlreadyExists) || i >= 1000 {
			return err
		}
		rep.StartedAt = rep.StartedAt.Add(time.Nanosecond)
	}
	raws, err := c.d.Store.ListDocs(ctx, CollRuns, "")
	if err != nil {
		return err
	}
	for i := 0; i < len(raws)-KeepRuns; i++ {
		var old domain.GCReport
		if err := json.Unmarshal(raws[i], &old); err != nil {
			return err
		}
		if err := c.d.Store.DeleteDoc(ctx, CollRuns, old.StartedAt.UTC().Format(runKeyLayout)); err != nil && !errors.Is(err, domain.ErrNotFound) {
			return err
		}
	}
	return nil
}

func (c *Collector) auditRun(ctx context.Context, rep *domain.GCReport) {
	if c.d.Audit == nil {
		return
	}
	details := map[string]string{
		"dry_run":        strconv.FormatBool(rep.DryRun),
		"scanned":        strconv.Itoa(rep.Scanned),
		"unreachable":    strconv.Itoa(rep.Unreachable),
		"marked":         strconv.Itoa(rep.Marked),
		"deleted":        strconv.Itoa(rep.Deleted),
		"freed_bytes":    strconv.FormatInt(rep.FreedBytes, 10),
		"staged_deleted": strconv.Itoa(rep.StagedDeleted),
		"audit_pruned":   strconv.Itoa(rep.AuditPruned),
	}
	if rep.Error != "" {
		details["error"] = rep.Error
	}
	c.d.Audit.Record(ctx, domain.AuditEvent{Actor: "system", Action: "gc.run", ResourceType: "server", ResourceID: "gc", Details: details})
}
