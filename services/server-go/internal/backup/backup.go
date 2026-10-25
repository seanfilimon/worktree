// Package backup creates and restores server backups of the metadata
// database and objects (docs/ENGINEERING_PLAN.md §15.2,
// crates/worktree-protocol/specs/storage/Backends.md).
//
// A backup is one <data>/backups/<id>.wtbak file (see archive.go) holding a
// consistent bbolt snapshot, every object and a manifest. It works with any
// object store that can list its objects (disk and S3). Backups are
// indexed in the doc collection "backups" (key = id); files found in the
// backup directory without an index entry are listed too, so a restored
// server still sees its archives. Restore is offline (Restore,
// `worktree-server restore`).
package backup

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/seanfilimon/worktree/services/server-go/internal/audit"
	"github.com/seanfilimon/worktree/services/server-go/internal/domain"
	"github.com/seanfilimon/worktree/services/server-go/internal/objstore"
)

// Collection is the doc collection indexing backups (key = id).
const Collection = "backups"

// Audit actions.
const (
	ActionCreate = "backup.create"
	ActionDelete = "backup.delete"
)

// ScheduledBy is BackupInfo.CreatedBy of scheduled backups; only those are
// pruned by the keep count.
const ScheduledBy = audit.ActorSystem

var (
	// ErrRunning is returned by Create while another backup is written.
	ErrRunning = errors.New("backup: a backup is already running")
	// ErrUnsupported is returned when the metadata store cannot be
	// snapshotted or the object store cannot list its objects.
	ErrUnsupported = errors.New("backup: not supported by this server's stores")
	// ErrNotFound is returned for unknown or malformed backup ids.
	ErrNotFound = fmt.Errorf("backup: %w", domain.ErrNotFound)
)

// Deps are the collaborators of the backup package. Store and Objects are
// required for backups; Store must implement Snapshotter (meta.Bolt) and
// Objects objstore.ListStore.
type Deps struct {
	Store   domain.MetaStore
	Objects objstore.Store
	History domain.History
	Authz   domain.Authorizer
	Bus     domain.EventBus
	Audit   domain.AuditLog
	Logger  *slog.Logger
}

// Options configure the service (server flags).
type Options struct {
	// Dir holds the .wtbak files (<data>/backups).
	Dir string
	// Interval is the period of scheduled backups (0 = off).
	Interval time.Duration
	// Keep is how many scheduled backups survive pruning (0 keeps all).
	Keep int
	// Version is the server version recorded in manifests.
	Version string
}

// Service creates, lists and deletes backups and runs the schedule.
type Service struct {
	d   Deps
	now func() time.Time

	mu      sync.Mutex
	o       Options
	nextRun time.Time
	lastErr string

	run sync.Mutex // held while a backup is written
}

// New returns a Service. Call Configure before use.
func New(d Deps) *Service {
	if d.Logger == nil {
		d.Logger = slog.Default()
	}
	return &Service{d: d, now: time.Now}
}

// Configure sets the options (call before Start).
func (s *Service) Configure(o Options) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.o = o
}

func (s *Service) opts() Options {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.o
}

// StoreInfo describes the object store being backed up.
type StoreInfo struct {
	// Kind is "disk", "s3" or "other".
	Kind string `json:"kind"`
	// Root is the disk store's data directory.
	Root string `json:"root,omitempty"`
	// Endpoint, Bucket, Prefix, Region and Secure describe an S3 store.
	Endpoint string `json:"endpoint,omitempty"`
	Bucket   string `json:"bucket,omitempty"`
	Prefix   string `json:"prefix,omitempty"`
	Region   string `json:"region,omitempty"`
	Secure   bool   `json:"secure,omitempty"`
	// Compress reports whether new objects are stored WTZ1-compressed.
	Compress bool `json:"compress"`
}

// DescribeStore returns StoreInfo for s.
func DescribeStore(s objstore.Store) StoreInfo {
	switch st := s.(type) {
	case *objstore.DiskStore:
		return StoreInfo{Kind: "disk", Root: st.Root(), Compress: st.Compresses()}
	case *objstore.S3Store:
		return StoreInfo{Kind: "s3", Endpoint: st.Endpoint(), Bucket: st.Bucket(), Prefix: st.Prefix(),
			Region: st.Region(), Secure: st.Secure(), Compress: st.Compresses()}
	default:
		return StoreInfo{Kind: "other"}
	}
}

// Config is the backup configuration as reported by the API.
type Config struct {
	// Dir is where backups are written on the server.
	Dir string `json:"dir"`
	// Interval is the schedule ("0s" = off) and IntervalSeconds the same
	// in seconds.
	Interval        string `json:"interval"`
	IntervalSeconds int64  `json:"interval_seconds"`
	// Keep is how many scheduled backups are kept (0 keeps all).
	Keep int `json:"keep"`
}

// Status is the response of GET /api/v1/admin/backups.
type Status struct {
	Config Config    `json:"config"`
	Store  StoreInfo `json:"store"`
	// Supported is false when this server's stores cannot be backed up.
	Supported bool `json:"supported"`
	// Running reports whether a backup is being written now.
	Running bool `json:"running"`
	// NextRunAt is the next scheduled backup (absent when off).
	NextRunAt *time.Time `json:"next_run_at,omitempty"`
	// LastError is the failure of the last scheduled backup, if it failed.
	LastError string `json:"last_error,omitempty"`
	// Backups are newest first.
	Backups []domain.BackupInfo `json:"backups"`
}

// Supported reports whether backups can be created with the configured
// stores.
func (s *Service) Supported() bool {
	_, ok1 := s.d.Store.(Snapshotter)
	_, ok2 := s.d.Objects.(objectSource)
	return ok1 && ok2 && s.opts().Dir != ""
}

// Running reports whether a backup is being written.
func (s *Service) Running() bool {
	if s.run.TryLock() {
		s.run.Unlock()
		return false
	}
	return true
}

// Status returns the configuration, schedule and every backup.
func (s *Service) Status(ctx context.Context) (*Status, error) {
	list, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	o := s.opts()
	st := &Status{
		Config: Config{Dir: o.Dir, Interval: o.Interval.String(), IntervalSeconds: int64(o.Interval.Seconds()), Keep: o.Keep},
		Store:  DescribeStore(s.d.Objects), Supported: s.Supported(), Running: s.Running(), Backups: list,
	}
	s.mu.Lock()
	if !s.nextRun.IsZero() {
		next := s.nextRun.UTC()
		st.NextRunAt = &next
	}
	st.LastError = s.lastErr
	s.mu.Unlock()
	return st, nil
}

var idRe = regexp.MustCompile(`^bk-[0-9]{8}T[0-9]{6}Z-[0-9a-f]{6}$`)

// ValidID reports whether id has the backup id form
// (bk-<yyyymmdd>T<hhmmss>Z-<6 hex>), which is also safe as a file name.
func ValidID(id string) bool { return idRe.MatchString(id) }

func newID(at time.Time) string {
	var b [3]byte
	_, _ = rand.Read(b[:])
	return "bk-" + at.UTC().Format("20060102T150405Z") + "-" + hex.EncodeToString(b[:])
}

func (s *Service) path(id string) string {
	return filepath.Join(s.opts().Dir, id+FileExt)
}

// Create writes a new backup and indexes it. createdBy is the username (or
// ScheduledBy). It returns ErrRunning when another backup is in progress
// and ErrUnsupported when the stores cannot be backed up.
func (s *Service) Create(ctx context.Context, createdBy, note string) (*domain.BackupInfo, error) {
	snap, ok := s.d.Store.(Snapshotter)
	objs, ok2 := s.d.Objects.(objectSource)
	o := s.opts()
	if !ok || !ok2 || o.Dir == "" {
		return nil, ErrUnsupported
	}
	if !s.run.TryLock() {
		return nil, ErrRunning
	}
	defer s.run.Unlock()

	started := s.now().UTC()
	id := newID(started)
	if err := os.MkdirAll(o.Dir, 0o750); err != nil {
		return nil, fmt.Errorf("backup: create backup directory: %w", err)
	}
	final := s.path(id)
	partial := final + ".partial"
	f, err := os.OpenFile(partial, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return nil, fmt.Errorf("backup: create file: %w", err)
	}
	done := false
	defer func() {
		if !done {
			_ = f.Close()
			_ = os.Remove(partial)
		}
	}()

	m, err := writeArchive(ctx, f, snap, objs, Manifest{
		Format: FormatName, Version: FormatVersion, ID: id, CreatedAt: started, CreatedBy: createdBy,
		Note: note, ServerVersion: o.Version, ObjectStore: DescribeStore(s.d.Objects).Kind,
	})
	if err != nil {
		return nil, err
	}
	if err := f.Sync(); err != nil {
		return nil, fmt.Errorf("backup: sync: %w", err)
	}
	if err := f.Close(); err != nil {
		return nil, fmt.Errorf("backup: close: %w", err)
	}
	done = true
	if err := os.Rename(partial, final); err != nil {
		_ = os.Remove(partial)
		return nil, fmt.Errorf("backup: finish file: %w", err)
	}
	fi, err := os.Stat(final)
	if err != nil {
		return nil, fmt.Errorf("backup: stat: %w", err)
	}
	abs, err := filepath.Abs(final)
	if err != nil {
		abs = final
	}
	info := &domain.BackupInfo{
		ID: id, Path: abs, SizeBytes: fi.Size(), Objects: m.Objects,
		CreatedAt: started, CreatedBy: createdBy, Note: note,
	}
	if err := s.d.Store.PutDoc(context.WithoutCancel(ctx), Collection, id, info); err != nil {
		return nil, fmt.Errorf("backup: index: %w", err)
	}
	s.d.Logger.Info("backup created", slog.String("id", id), slog.Int64("objects", m.Objects),
		slog.Int64("bytes", fi.Size()), slog.String("by", createdBy), slog.Duration("took", time.Since(started)))
	return info, nil
}

// List returns every backup, newest first: indexed backups whose file
// still exists plus unindexed .wtbak files in the backup directory (e.g.
// archives copied in, or created after the snapshot a server was restored
// from).
func (s *Service) List(ctx context.Context) ([]domain.BackupInfo, error) {
	o := s.opts()
	raws, err := s.d.Store.ListDocs(ctx, Collection, "")
	if err != nil {
		return nil, err
	}
	seen := map[string]bool{}
	out := []domain.BackupInfo{}
	for _, raw := range raws {
		var b domain.BackupInfo
		if err := json.Unmarshal(raw, &b); err != nil || !ValidID(b.ID) {
			continue
		}
		if o.Dir == "" {
			continue
		}
		// The directory may have moved (restored data dir): trust the id.
		fi, err := os.Stat(s.path(b.ID))
		if err != nil || !fi.Mode().IsRegular() {
			continue
		}
		b.Path, b.SizeBytes = absPath(s.path(b.ID)), fi.Size()
		seen[b.ID] = true
		out = append(out, b)
	}
	if o.Dir != "" {
		entries, err := os.ReadDir(o.Dir)
		if err != nil && !errors.Is(err, fs.ErrNotExist) {
			return nil, fmt.Errorf("backup: read backup directory: %w", err)
		}
		for _, e := range entries {
			id, ok := strings.CutSuffix(e.Name(), FileExt)
			if !ok || !ValidID(id) || seen[id] || !e.Type().IsRegular() {
				continue
			}
			fi, err := e.Info()
			if err != nil {
				continue
			}
			created := fi.ModTime().UTC()
			if t, err := time.Parse("20060102T150405Z", id[3:19]); err == nil {
				created = t
			}
			out = append(out, domain.BackupInfo{ID: id, Path: absPath(s.path(id)), SizeBytes: fi.Size(), CreatedAt: created})
		}
	}
	slices.SortFunc(out, func(a, b domain.BackupInfo) int {
		if c := b.CreatedAt.Compare(a.CreatedAt); c != 0 {
			return c
		}
		return strings.Compare(b.ID, a.ID)
	})
	return out, nil
}

func absPath(p string) string {
	if abs, err := filepath.Abs(p); err == nil {
		return abs
	}
	return p
}

// Get returns one backup; ErrNotFound when unknown.
func (s *Service) Get(ctx context.Context, id string) (*domain.BackupInfo, error) {
	if !ValidID(id) {
		return nil, ErrNotFound
	}
	list, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	for i := range list {
		if list[i].ID == id {
			return &list[i], nil
		}
	}
	return nil, ErrNotFound
}

// Open opens the archive of backup id for reading (download).
func (s *Service) Open(ctx context.Context, id string) (*os.File, *domain.BackupInfo, error) {
	info, err := s.Get(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	f, err := os.Open(s.path(id))
	if errors.Is(err, fs.ErrNotExist) {
		return nil, nil, ErrNotFound
	}
	if err != nil {
		return nil, nil, fmt.Errorf("backup: open: %w", err)
	}
	return f, info, nil
}

// Delete removes the archive and the index entry of backup id;
// ErrNotFound when neither exists.
func (s *Service) Delete(ctx context.Context, id string) error {
	if !ValidID(id) {
		return ErrNotFound
	}
	found := false
	err := os.Remove(s.path(id))
	switch {
	case err == nil:
		found = true
	case errors.Is(err, fs.ErrNotExist):
	default:
		return fmt.Errorf("backup: delete file: %w", err)
	}
	err = s.d.Store.DeleteDoc(ctx, Collection, id)
	switch {
	case err == nil:
		found = true
	case errors.Is(err, domain.ErrNotFound):
	default:
		return err
	}
	if !found {
		return ErrNotFound
	}
	return nil
}

// Prune deletes scheduled backups beyond the newest Keep (no-op when Keep
// is 0) and returns the deleted ids. Backups created by people are never
// pruned.
func (s *Service) Prune(ctx context.Context) ([]string, error) {
	keep := s.opts().Keep
	if keep <= 0 {
		return nil, nil
	}
	list, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	var deleted []string
	n := 0
	for _, b := range list { // newest first
		if b.CreatedBy != ScheduledBy {
			continue
		}
		n++
		if n <= keep {
			continue
		}
		if err := s.Delete(ctx, b.ID); err != nil && !errors.Is(err, ErrNotFound) {
			return deleted, err
		}
		deleted = append(deleted, b.ID)
	}
	return deleted, nil
}

// RunScheduled creates one scheduled backup, records it in the audit log
// and prunes old scheduled backups.
func (s *Service) RunScheduled(ctx context.Context) (*domain.BackupInfo, error) {
	info, err := s.Create(ctx, ScheduledBy, "scheduled")
	s.mu.Lock()
	if err != nil && !errors.Is(err, ErrRunning) {
		s.lastErr = err.Error()
	} else if err == nil {
		s.lastErr = ""
	}
	s.mu.Unlock()
	if err != nil {
		return nil, err
	}
	if s.d.Audit != nil {
		s.d.Audit.Record(ctx, audit.System(ActionCreate, "backup", info.ID, map[string]string{
			"objects": fmt.Sprint(info.Objects), "size_bytes": fmt.Sprint(info.SizeBytes), "scheduled": "true"}))
	}
	pruned, err := s.Prune(ctx)
	for _, id := range pruned {
		if s.d.Audit != nil {
			s.d.Audit.Record(ctx, audit.System(ActionDelete, "backup", id, map[string]string{"reason": "keep"}))
		}
	}
	if err != nil {
		s.d.Logger.Warn("backup: prune failed", slog.Any("error", err))
	}
	return info, nil
}

// Start runs scheduled backups every Interval until ctx is done or stop is
// called. It is a no-op when Interval is 0 or backups are unsupported.
func (s *Service) Start(ctx context.Context) (stop func()) {
	interval := s.opts().Interval
	if interval <= 0 {
		return func() {}
	}
	if !s.Supported() {
		s.d.Logger.Warn("backup: scheduled backups are not supported by this server's stores")
		return func() {}
	}
	ctx, cancel := context.WithCancel(ctx)
	done := make(chan struct{})
	go func() {
		defer close(done)
		t := time.NewTicker(interval)
		defer t.Stop()
		for {
			s.mu.Lock()
			s.nextRun = s.now().Add(interval)
			s.mu.Unlock()
			select {
			case <-ctx.Done():
				return
			case <-t.C:
			}
			if _, err := s.RunScheduled(ctx); err != nil && !errors.Is(err, ErrRunning) && ctx.Err() == nil {
				s.d.Logger.Error("backup: scheduled backup failed", slog.Any("error", err))
			}
		}
	}()
	return func() {
		cancel()
		<-done
		s.mu.Lock()
		s.nextRun = time.Time{}
		s.mu.Unlock()
	}
}
