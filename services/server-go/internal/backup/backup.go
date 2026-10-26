// Package backup creates and restores server backups of the metadata
// database and objects (docs/ENGINEERING_PLAN.md §15.2, §16.11,
// crates/worktree-protocol/specs/storage/Backends.md).
//
// A backup is one <id>.wtbak archive (see archive.go) holding a consistent
// bbolt snapshot, every object and a manifest. It works with any object
// store that can list its objects (disk and S3). Archives are written to
// <data>/backups and then kept there (the disk target) or uploaded to a
// bucket (the S3 target, `--backup-target s3`; target.go). Backups are
// indexed in the doc collection "backups" (key = id); archives found in the
// target without an index entry are listed too, so a restored server still
// sees its archives. Restore is offline (Restore, `worktree-server
// restore`).
package backup

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
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
// pruned by the keep count and the maximum age.
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
	// Dir is the local backup directory (<data>/backups): the disk target
	// and the staging directory of uploads.
	Dir string
	// Target keeps finished archives (nil = a DiskTarget on Dir).
	Target Target
	// Interval is the period of scheduled backups (0 = off).
	Interval time.Duration
	// Keep is how many scheduled backups survive pruning (0 keeps all).
	Keep int
	// MaxAge prunes scheduled backups older than this (0 = no age limit).
	MaxAge time.Duration
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

// target returns the configured target (a DiskTarget on Dir by default).
func (s *Service) target() Target {
	o := s.opts()
	if o.Target != nil {
		return o.Target
	}
	return NewDiskTarget(o.Dir)
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
	// Dir is the local backup (staging) directory on the server.
	Dir string `json:"dir"`
	// Target is where archives are kept: "disk" or "s3" (--backup-target).
	Target string `json:"target"`
	// Location describes the target: the directory, or s3://bucket/prefix/.
	Location string `json:"location"`
	// Interval is the schedule ("0s" = off) and IntervalSeconds the same
	// in seconds.
	Interval        string `json:"interval"`
	IntervalSeconds int64  `json:"interval_seconds"`
	// Keep is how many scheduled backups are kept (0 keeps all).
	Keep int `json:"keep"`
	// MaxAge prunes scheduled backups older than this ("0s" = no limit,
	// --backup-max-age); MaxAgeSeconds is the same in seconds.
	MaxAge        string `json:"max_age"`
	MaxAgeSeconds int64  `json:"max_age_seconds"`
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

// Describe returns the configuration as reported by the API.
func (s *Service) Describe() Config {
	o := s.opts()
	t := s.target()
	return Config{
		Dir: o.Dir, Target: t.Kind(), Location: t.Location(),
		Interval: o.Interval.String(), IntervalSeconds: int64(o.Interval.Seconds()), Keep: o.Keep,
		MaxAge: o.MaxAge.String(), MaxAgeSeconds: int64(o.MaxAge.Seconds()),
	}
}

// Status returns the configuration, schedule and every backup.
func (s *Service) Status(ctx context.Context) (*Status, error) {
	list, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	st := &Status{
		Config: s.Describe(),
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

// IDTime returns the creation time encoded in a valid id.
func IDTime(id string) (time.Time, bool) {
	if !ValidID(id) {
		return time.Time{}, false
	}
	t, err := time.Parse("20060102T150405Z", id[3:19])
	return t, err == nil
}

func newID(at time.Time) string {
	var b [3]byte
	_, _ = rand.Read(b[:])
	return "bk-" + at.UTC().Format("20060102T150405Z") + "-" + hex.EncodeToString(b[:])
}

// Create writes a new backup, moves it into the target and indexes it.
// createdBy is the username (or ScheduledBy). It returns ErrRunning when
// another backup is in progress and ErrUnsupported when the stores cannot
// be backed up.
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
	target := s.target()

	started := s.now().UTC()
	id := newID(started)
	if err := os.MkdirAll(o.Dir, 0o750); err != nil {
		return nil, fmt.Errorf("backup: create backup directory: %w", err)
	}
	partial := filepath.Join(o.Dir, id+FileExt+".partial")
	f, err := os.OpenFile(partial, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return nil, fmt.Errorf("backup: create file: %w", err)
	}
	closed := false
	defer func() {
		if !closed {
			_ = f.Close()
		}
		_ = os.Remove(partial) // gone after a successful Store
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
	fi, err := f.Stat()
	if err != nil {
		return nil, fmt.Errorf("backup: stat: %w", err)
	}
	closed = true
	if err := f.Close(); err != nil {
		return nil, fmt.Errorf("backup: close: %w", err)
	}
	// The upload outlives a client that disconnects.
	if err := target.Store(context.WithoutCancel(ctx), id, partial); err != nil {
		return nil, err
	}
	info := &domain.BackupInfo{
		ID: id, Path: target.Path(id), SizeBytes: fi.Size(), Objects: m.Objects,
		CreatedAt: started, CreatedBy: createdBy, Note: note,
	}
	if err := s.d.Store.PutDoc(context.WithoutCancel(ctx), Collection, id, info); err != nil {
		return nil, fmt.Errorf("backup: index: %w", err)
	}
	s.d.Logger.Info("backup created", slog.String("id", id), slog.String("target", target.Location()),
		slog.Int64("objects", m.Objects), slog.Int64("bytes", fi.Size()), slog.String("by", createdBy),
		slog.Duration("took", time.Since(started)))
	return info, nil
}

// List returns every backup in the target, newest first: indexed backups
// whose archive still exists plus unindexed archives (e.g. archives copied
// in, or created after the snapshot a server was restored from).
func (s *Service) List(ctx context.Context) ([]domain.BackupInfo, error) {
	if s.opts().Dir == "" && s.opts().Target == nil {
		return []domain.BackupInfo{}, nil
	}
	target := s.target()
	objs, err := target.List(ctx)
	if err != nil {
		return nil, err
	}
	found := make(map[string]TargetObject, len(objs))
	for _, o := range objs {
		found[o.ID] = o
	}
	raws, err := s.d.Store.ListDocs(ctx, Collection, "")
	if err != nil {
		return nil, err
	}
	out := []domain.BackupInfo{}
	seen := map[string]bool{}
	for _, raw := range raws {
		var b domain.BackupInfo
		if err := json.Unmarshal(raw, &b); err != nil || !ValidID(b.ID) {
			continue
		}
		// The directory may have moved (restored data dir): trust the id.
		obj, ok := found[b.ID]
		if !ok {
			continue
		}
		b.Path, b.SizeBytes = target.Path(b.ID), obj.Size
		seen[b.ID] = true
		out = append(out, b)
	}
	for _, obj := range objs {
		if seen[obj.ID] {
			continue
		}
		created := obj.Modified
		if t, ok := IDTime(obj.ID); ok {
			created = t
		}
		out = append(out, domain.BackupInfo{ID: obj.ID, Path: target.Path(obj.ID), SizeBytes: obj.Size, CreatedAt: created})
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

// Open opens the archive of backup id for reading (download). The reader
// is an *os.File (seekable) for the disk target.
func (s *Service) Open(ctx context.Context, id string) (io.ReadCloser, *domain.BackupInfo, error) {
	info, err := s.Get(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	r, obj, err := s.target().Open(ctx, id)
	if err != nil {
		return nil, nil, err
	}
	info.SizeBytes = obj.Size
	return r, info, nil
}

// Delete removes the archive and the index entry of backup id;
// ErrNotFound when neither exists.
func (s *Service) Delete(ctx context.Context, id string) error {
	if !ValidID(id) {
		return ErrNotFound
	}
	found := false
	err := s.target().Delete(ctx, id)
	switch {
	case err == nil:
		found = true
	case errors.Is(err, ErrNotFound):
	default:
		return err
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

// Prune deletes scheduled backups beyond the newest Keep (0 = no count
// limit) and those older than MaxAge (0 = no age limit), and returns the
// deleted ids. Backups created by people are never pruned.
func (s *Service) Prune(ctx context.Context) ([]string, error) {
	o := s.opts()
	if o.Keep <= 0 && o.MaxAge <= 0 {
		return nil, nil
	}
	list, err := s.List(ctx)
	if err != nil {
		return nil, err
	}
	cutoff := s.now().Add(-o.MaxAge)
	var deleted []string
	n := 0
	for _, b := range list { // newest first
		if b.CreatedBy != ScheduledBy {
			continue
		}
		n++
		tooMany := o.Keep > 0 && n > o.Keep
		tooOld := o.MaxAge > 0 && b.CreatedAt.Before(cutoff)
		if !tooMany && !tooOld {
			continue
		}
		if err := s.Delete(ctx, b.ID); err != nil && !errors.Is(err, ErrNotFound) {
			return deleted, err
		}
		deleted = append(deleted, b.ID)
	}
	return deleted, nil
}

// PruneAudited runs Prune and records every deletion in the audit log
// (reason "retention").
func (s *Service) PruneAudited(ctx context.Context) ([]string, error) {
	pruned, err := s.Prune(ctx)
	if s.d.Audit != nil {
		for _, id := range pruned {
			s.d.Audit.Record(ctx, audit.System(ActionDelete, "backup", id, map[string]string{"reason": "retention"}))
		}
	}
	return pruned, err
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
